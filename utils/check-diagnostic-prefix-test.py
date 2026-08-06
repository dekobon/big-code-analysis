#!/usr/bin/env python3
"""Tests for check-diagnostic-prefix.py.

Three kinds of test, matching the check-excluded-manifests-test.py
pattern:

* Unit tests over synthetic Rust snippets, weighted toward the two ways
  a source-scanning gate goes wrong: a hit it should have reported
  (every emitting shape, every spelling, raw and hash-delimited
  literals) and a hit it should not (prose containing the word, a
  comment explaining the rule, an opted-out fixture).
* ``main()`` tests over a synthetic repository root, covering the clean
  and the offending branch plus the remediation text each prints.
* A smoke test running the real gate against the real repository,
  asserting a clean tree reports OK.

The false-clean direction is the one that matters: a gate that reports
"OK" over a tree it failed to read is indistinguishable from a tree with
nothing to find, which is precisely the outcome the gate exists to
prevent (see `.claude/rules/tool-output.md`).

Run with:
    python3 -m unittest -q utils/check-diagnostic-prefix-test.py
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import pathlib
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

UTILS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = UTILS_DIR.parent
SCRIPT_SRC = UTILS_DIR / "check-diagnostic-prefix.py"


def _load_module():  # type: ignore[no-untyped-def]
    spec = importlib.util.spec_from_file_location("check_diagnostic_prefix", SCRIPT_SRC)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


GATE = _load_module()


class ScanTextOffenders(unittest.TestCase):
    """Shapes that must be reported."""

    def test_capitalised_eprintln_is_reported(self) -> None:
        found = GATE.scan_text('    eprintln!("Warning: skipping {p}");\n')
        self.assertEqual(len(found), 1)
        line_no, word, line = found[0]
        self.assertEqual(line_no, 1)
        self.assertEqual(word, "Warning")
        self.assertTrue(line.startswith("eprintln!"))

    def test_literal_on_its_own_line_is_reported(self) -> None:
        # The wrapped shape every offender #1199 collected was written
        # in: the macro on one line, the literal on the next. A gate
        # matching `eprintln!` and the literal on the *same* line would
        # have reported the whole tree clean.
        text = 'eprintln!(\n    "Warning: not a regular file: {}",\n    p\n);\n'
        found = GATE.scan_text(text)
        self.assertEqual([(2, "Warning")], [(n, w) for n, w, _ in found])

    def test_every_severity_and_case_is_reported(self) -> None:
        for word in ("Warning", "WARNING", "Error", "ERROR", "Note", "NOTE"):
            with self.subTest(word=word):
                found = GATE.scan_text(f'let m = "{word}: bad";\n')
                self.assertEqual([w for _, w, _ in found], [word])

    def test_raw_and_hash_delimited_literals_are_reported(self) -> None:
        for literal in ('r"Error: x"', 'r#"Error: x"#', 'r##"Error: x"##'):
            with self.subTest(literal=literal):
                self.assertEqual(len(GATE.scan_text(f"let m = {literal};\n")), 1)

    def test_line_numbers_survive_an_exotic_separator_in_a_literal(self) -> None:
        # `str.splitlines()` breaks on U+2028 and the vertical-tab
        # family; rustc does not. One inside a string literal would
        # shift every reported line number after it by one, sending the
        # reader to the wrong line.
        text = 'let sep = " ";\neprintln!("Error: x");\n'
        self.assertEqual([(2, "Error")], [(n, w) for n, w, _ in GATE.scan_text(text)])

    def test_every_offending_line_is_reported_not_just_the_first(self) -> None:
        text = 'let a = "Warning: one";\nlet b = "Error: two";\n'
        self.assertEqual([(1, "Warning"), (2, "Error")], [(n, w) for n, w, _ in GATE.scan_text(text)])


class ScanTextNonOffenders(unittest.TestCase):
    """Shapes that must not be reported."""

    def test_lowercase_prefix_is_the_house_style(self) -> None:
        self.assertEqual(GATE.scan_text('eprintln!("warning: {msg}");\n'), [])

    def test_word_inside_prose_is_not_a_prefix(self) -> None:
        # Anchoring at the literal's start is what keeps ordinary prose
        # out: the gate is about a prefix, not about the word.
        self.assertEqual(GATE.scan_text('let m = "retry on Error: yes";\n'), [])

    def test_word_without_colon_is_not_a_prefix(self) -> None:
        self.assertEqual(GATE.scan_text('let m = "Warning about x";\n'), [])

    def test_comment_lines_are_skipped(self) -> None:
        # This gate's own explanatory comments quote the banned shape;
        # so does the CHANGELOG entry pasted into a doc comment.
        text = '// never write "Warning: x"\n/// Renders "Error: y" historically.\n'
        self.assertEqual(GATE.scan_text(text), [])

    def test_marker_on_the_same_line_opts_out(self) -> None:
        text = 'assert!(!out.contains("Error:")); // diag-prefix-ok: asserted absent\n'
        self.assertEqual(GATE.scan_text(text), [])

    def test_marker_anywhere_in_the_comment_block_above_opts_out(self) -> None:
        # The live opt-out in `check_thresholds.rs` is a two-line block
        # with the marker on the first line; a rule that only looked at
        # the immediately preceding line would miss it.
        text = (
            "        // diag-prefix-ok: the capitalised spelling is the\n"
            "        // thing asserted absent here (#1199).\n"
            '        .stderr(predicate::str::contains("Error:").not());\n'
        )
        self.assertEqual(GATE.scan_text(text), [])

    def test_marker_below_the_line_does_not_opt_out(self) -> None:
        # Only the line itself and the block above count, so a marker
        # attached to the *next* statement cannot silently cover this
        # one.
        text = 'eprintln!("Error: x");\n// diag-prefix-ok\n'
        self.assertEqual(len(GATE.scan_text(text)), 1)

    def test_blank_line_breaks_the_comment_block(self) -> None:
        text = "// diag-prefix-ok\n\neprintln!(\"Error: x\");\n"
        self.assertEqual(len(GATE.scan_text(text)), 1)


class MainOverSyntheticRoot(unittest.TestCase):
    """``main()`` end-to-end over a temporary tree."""

    @contextlib.contextmanager
    def _root_with(self, files: dict[str, str]):  # type: ignore[no-untyped-def]
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            for rel, text in files.items():
                path = root / rel
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text, encoding="utf-8")
            with mock.patch.object(GATE, "REPO_ROOT", root):
                yield root

    def _run_main(self) -> tuple[int, str, str]:
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = GATE.main()
        return code, out.getvalue(), err.getvalue()

    def test_clean_tree_passes_and_reports_the_file_count(self) -> None:
        with self._root_with({"src/a.rs": 'eprintln!("warning: ok");\n'}):
            code, out, _ = self._run_main()
        self.assertEqual(code, 0)
        # The count is part of the contract: "OK (0 files checked)" is a
        # gate that found nothing because it read nothing.
        self.assertIn("1 Rust files checked", out)

    def test_offending_tree_fails_and_names_file_line_and_remedy(self) -> None:
        with self._root_with(
            {
                "src/a.rs": 'fn f() {\n    eprintln!("Warning: bad");\n}\n',
                "src/ok.rs": 'eprintln!("warning: fine");\n',
            }
        ):
            code, _, err = self._run_main()
        self.assertEqual(code, 1)
        self.assertIn("src/a.rs:2", err)
        self.assertIn("Warning:", err)
        self.assertIn("diag.rs", err)
        self.assertNotIn("src/ok.rs", err)

    def test_non_rust_files_are_not_scanned(self) -> None:
        with self._root_with({"utils/x.py": 'print("Warning: bad")\n'}):
            code, out, _ = self._run_main()
        self.assertEqual(code, 0)
        self.assertIn("0 Rust files checked", out)


class RealRepositorySmoke(unittest.TestCase):
    def test_gate_passes_on_this_repository(self) -> None:
        result = subprocess.run(
            [sys.executable, str(SCRIPT_SRC)],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(
            result.returncode,
            0,
            f"gate failed on a clean tree:\n{result.stdout}\n{result.stderr}",
        )
        self.assertIn("Diagnostic prefixes OK", result.stdout)


if __name__ == "__main__":
    unittest.main()
