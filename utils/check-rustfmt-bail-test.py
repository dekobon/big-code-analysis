#!/usr/bin/env python3
"""Tests for check-rustfmt-bail.py.

Three kinds of test, matching the check-versions-test.py pattern:

* Unit tests against synthetic Rust snippets, weighted toward the text
  the arm regex mis-read before span filtering was added: JS and C#
  fixtures inside Rust string literals, a Groovy spaceship operator, a
  commented-out arm.
* End-to-end probe tests that actually shell out to rustfmt, pinning
  both directions of the property the gate exists to measure — a clean
  match reports zero, the same match with a comment inside the pattern
  reports non-zero.
* ``main()`` tests over a synthetic tree, covering the pass, regress and
  ratchet branches, plus a smoke test asserting the real repository is
  clean against its checked-in baseline.

Run with:
    python3 -m unittest -q utils/check-rustfmt-bail-test.py
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import pathlib
import shutil
import sys
import tempfile
import unittest

UTILS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = UTILS_DIR.parent
SCRIPT_SRC = UTILS_DIR / "check-rustfmt-bail.py"

HAVE_RUSTFMT = shutil.which("rustfmt") is not None


def _load_module():  # type: ignore[no-untyped-def]
    spec = importlib.util.spec_from_file_location("check_rustfmt_bail", SCRIPT_SRC)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


gate = _load_module()


CLEAN_MATCH = """\
fn f(x: u32) -> u32 {
    match x {
        // A comment above the arm is fine and formats normally.
        1 | 2 => 10,
        _ => 0,
    }
}
"""

# The defect: a line comment between `|` alternatives, inside the
# pattern. rustfmt emits the whole `match` verbatim and says nothing.
IN_PATTERN_COMMENT = """\
fn f(x: u32) -> u32 {
    match x {
        1
        // Why 2 belongs here.
        | 2 => 10,
        _ => 0,
    }
}
"""

# The other spelling: a block comment between the pattern and its `=>`.
BLOCK_COMMENT_BEFORE_FAT_ARROW = """\
fn f(x: u32) -> u32 {
    match x {
        1 /* and 2, historically */ => 10,
        _ => 0,
    }
}
"""


class FindArmLinesTest(unittest.TestCase):
    """The arm detector must see real arms and nothing else."""

    def test_finds_real_arms(self) -> None:
        self.assertEqual(gate.find_arm_lines(CLEAN_MATCH), [3, 4])

    def test_arm_inside_a_string_literal_is_not_an_arm(self) -> None:
        # A JS fixture embedded in a Rust test. rustfmt never rewrites
        # string contents, so counting these makes every fixture file a
        # permanent, unfixable baseline entry (the naive probe reported
        # 15 of them in src/metrics/nom.rs alone).
        source = 'const SRC: &str = "const add = (a, b) => a + b;";\n'
        self.assertEqual(gate.find_arm_lines(source), [])

    def test_arm_inside_a_raw_string_is_not_an_arm(self) -> None:
        source = 'let s = r#"\n    public int W => _w;\n"#;\n'
        self.assertEqual(gate.find_arm_lines(source), [])

    def test_spaceship_operator_in_a_fixture_is_not_an_arm(self) -> None:
        source = 'let groovy = "    def ship = a <=> b";\n'
        self.assertEqual(gate.find_arm_lines(source), [])

    def test_commented_out_arm_is_not_an_arm(self) -> None:
        source = "fn f() {\n    match x {\n        // _ => 0,\n        _ => 1,\n    }\n}\n"
        self.assertEqual(gate.find_arm_lines(source), [3])

    def test_macro_rules_matcher_line_is_not_an_arm(self) -> None:
        source = "macro_rules! m {\n    ($a:ident, $b:ty) => {\n        1\n    };\n}\n"
        self.assertEqual(gate.find_arm_lines(source), [])


@unittest.skipUnless(HAVE_RUSTFMT, "rustfmt not installed")
class ProbeTest(unittest.TestCase):
    """Both directions of the measurement, against the real rustfmt.

    A gate that only ever reports zero is indistinguishable from a
    working one until the day it matters, so the negative case
    (``CLEAN_MATCH`` → 0) is asserted alongside the positive.
    """

    def test_clean_match_reports_no_bail(self) -> None:
        status, count, message = gate.probe_source(CLEAN_MATCH)
        self.assertEqual((status, count, message), ("ok", 0, ""))

    def test_line_comment_between_alternatives_bails(self) -> None:
        status, count, _ = gate.probe_source(IN_PATTERN_COMMENT)
        self.assertEqual(status, "ok")
        self.assertEqual(count, 2)

    def test_block_comment_before_fat_arrow_bails(self) -> None:
        status, count, _ = gate.probe_source(BLOCK_COMMENT_BEFORE_FAT_ARROW)
        self.assertEqual(status, "ok")
        self.assertEqual(count, 2)

    def test_hoisting_the_comment_clears_the_bail(self) -> None:
        # The prescribed fix, pinned end to end: the same comment, moved
        # above the arm rather than deleted, formats clean.
        hoisted = IN_PATTERN_COMMENT.replace(
            "        1\n        // Why 2 belongs here.\n        | 2 => 10,",
            "        // Why 2 belongs here.\n        1 | 2 => 10,",
        )
        self.assertNotEqual(hoisted, IN_PATTERN_COMMENT)
        self.assertEqual(gate.probe_source(hoisted)[:2], ("ok", 0))

    def test_no_arms_reports_skip(self) -> None:
        self.assertEqual(gate.probe_source("fn f() {}\n")[:2], ("SKIP", 0))

    def test_unparseable_input_reports_error_not_a_bail(self) -> None:
        # Swallowing stderr here is how a parse failure gets recorded as
        # a bail; the status must stay distinct.
        status, count, message = gate.probe_source("fn f( {\n    _ => 1,\n}\n")
        self.assertEqual(status, "ERROR")
        self.assertEqual(count, 0)
        self.assertTrue(message)

    def test_mod_declaration_does_not_error(self) -> None:
        # Probing via a path would make rustfmt resolve `mod`, which
        # fails outside the module's own tree. On stdin it must not.
        source = "mod nonexistent_module_xyz;\n" + CLEAN_MATCH
        self.assertEqual(gate.probe_source(source)[:2], ("ok", 0))


@unittest.skipUnless(HAVE_RUSTFMT, "rustfmt not installed")
class MainTest(unittest.TestCase):
    """The gate's exit codes over a synthetic tree."""

    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        self.addCleanup(self.tmp.cleanup)
        self.src = self.root / "src"
        self.src.mkdir()
        self.baseline = self.root / ".rustfmt-bail-baseline.txt"

    def _run(self, *argv: str) -> tuple[int, str, str]:
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = gate.main(
                ["--root", str(self.root), "--baseline", str(self.baseline), *argv]
            )
        return code, out.getvalue(), err.getvalue()

    def test_missing_baseline_is_an_error_with_remediation(self) -> None:
        (self.src / "a.rs").write_text(CLEAN_MATCH, encoding="utf-8")
        code, _, err = self._run()
        self.assertEqual(code, 2)
        self.assertIn("--update", err)

    def test_clean_tree_passes(self) -> None:
        (self.src / "a.rs").write_text(CLEAN_MATCH, encoding="utf-8")
        self.assertEqual(self._run("--update")[0], 0)
        code, out, _ = self._run()
        self.assertEqual(code, 0)
        self.assertIn("rustfmt-bail: OK", out)

    def test_new_in_pattern_comment_fails_the_gate(self) -> None:
        target = self.src / "a.rs"
        target.write_text(CLEAN_MATCH, encoding="utf-8")
        self.assertEqual(self._run("--update")[0], 0)
        target.write_text(IN_PATTERN_COMMENT, encoding="utf-8")
        code, _, err = self._run()
        self.assertEqual(code, 1)
        self.assertIn("src/a.rs", err)
        self.assertIn("baseline 0, current 2", err)
        self.assertIn("Hoist it above the arm", err)

    def test_decrease_is_silent_and_ratchets_on_update(self) -> None:
        target = self.src / "a.rs"
        target.write_text(IN_PATTERN_COMMENT, encoding="utf-8")
        self.assertEqual(self._run("--update")[0], 0)
        self.assertIn("src/a.rs 2", self.baseline.read_text(encoding="utf-8"))
        target.write_text(CLEAN_MATCH, encoding="utf-8")
        self.assertEqual(self._run()[0], 0)
        self.assertEqual(self._run("--update")[0], 0)
        self.assertNotIn("src/a.rs", self.baseline.read_text(encoding="utf-8"))

    def test_update_preserves_hand_written_notes(self) -> None:
        # The "this bail is deliberate, here is why" decisions live in
        # the baseline as comments; --update must not eat them.
        (self.src / "a.rs").write_text(IN_PATTERN_COMMENT, encoding="utf-8")
        self.assertEqual(self._run("--update")[0], 0)
        text = self.baseline.read_text(encoding="utf-8")
        self.baseline.write_text(
            text.replace("\nsrc/a.rs", "\n# note: src/a.rs reads worse hoisted.\nsrc/a.rs"),
            encoding="utf-8",
        )
        self.assertEqual(self._run("--update")[0], 0)
        self.assertIn(
            "# note: src/a.rs reads worse hoisted.",
            self.baseline.read_text(encoding="utf-8"),
        )

    def test_malformed_baseline_line_exits_two(self) -> None:
        (self.src / "a.rs").write_text(CLEAN_MATCH, encoding="utf-8")
        self.baseline.write_text("src/a.rs notanumber\n", encoding="utf-8")
        with self.assertRaises(SystemExit) as caught:
            self._run()
        self.assertEqual(caught.exception.code, 2)


@unittest.skipUnless(HAVE_RUSTFMT, "rustfmt not installed")
class RealRepositoryTest(unittest.TestCase):
    def test_repository_is_clean_against_its_baseline(self) -> None:
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = gate.main([])
        self.assertEqual(code, 0, err.getvalue())
        self.assertIn("rustfmt-bail: OK", out.getvalue())


if __name__ == "__main__":
    unittest.main(module=None, argv=[sys.argv[0], *sys.argv[1:]])
