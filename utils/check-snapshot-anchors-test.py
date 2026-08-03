#!/usr/bin/env python3
"""Tests for check-snapshot-anchors.py.

Three kinds of test, matching the check-rustfmt-bail-test.py pattern:

* Unit tests for the literal lexers, weighted toward the inputs that
  made the scanner under-count before #1192: a char literal holding a
  double quote, a lifetime that must *not* be read as one, and the
  byte-string prefixes the raw-string branch used to miss.
* Unit tests for ``count_bare_in_source`` over synthetic Rust, pinning
  both directions — an anchored call stays uncounted, a bare call is
  counted even when a quote-bearing literal precedes it.
* ``main()`` tests over a synthetic tree covering the pass, regress and
  ratchet branches, plus a smoke test asserting the real repository is
  clean against its checked-in baseline.

The gate scripts are hyphen-named and so cannot be imported normally;
the loader below mirrors the sibling test file.

Run with:
    python3 -m unittest -q utils/check-snapshot-anchors-test.py
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import pathlib
import sys
import tempfile
import unittest

UTILS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = UTILS_DIR.parent
SCRIPT_SRC = UTILS_DIR / "check-snapshot-anchors.py"


def _load_module():  # type: ignore[no-untyped-def]
    spec = importlib.util.spec_from_file_location("check_snapshot_anchors", SCRIPT_SRC)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


gate = _load_module()


class CharLiteralEndTest(unittest.TestCase):
    """Ported from check-rustfmt-bail-test.py, which hit this first.

    Both directions are pinned deliberately: the pre-#1192 behaviour
    (no char-literal branch at all) fails the quote-bearing cases, and a
    *greedy* variant that treats a lifetime as a literal fails
    ``test_lifetimes_and_labels_are_not_literals``.
    """

    def test_forms_and_their_ends(self) -> None:
        for source, expected in [
            ("'a'", 3),
            ("'_'", 3),
            (r"'\n'", 4),
            (r"'\''", 4),
            (r"'\\'", 4),
            ("'\"'", 3),
            (r"'\u{1F600}'", 11),
        ]:
            with self.subTest(source=source):
                self.assertEqual(gate.char_literal_end(source, 0), expected)

    def test_lifetimes_and_labels_are_not_literals(self) -> None:
        for source in ["'a>", "'a,", "'static ", "'_>", "'outer: loop", "'"]:
            with self.subTest(source=source):
                self.assertIsNone(gate.char_literal_end(source, 0))


class RawStringEndTest(unittest.TestCase):
    def test_raw_and_byte_raw_forms(self) -> None:
        for source, expected in [
            ('r"a"', 4),
            ('r#"a"#', 6),
            ('r##"a"##', 8),
            ('br"a"', 5),
            ('br#"a"#', 7),
        ]:
            with self.subTest(source=source):
                self.assertEqual(gate.raw_string_end(source, 0), expected)

    def test_non_raw_openers_are_rejected(self) -> None:
        # `b"…"` is deliberately not handled here: the `b` falls through
        # as an ordinary character and the `"` opens a regular literal,
        # which escapes identically.
        for source in ['b"a"', "rust", "bar", "r", "b"]:
            with self.subTest(source=source):
                self.assertIsNone(gate.raw_string_end(source, 0))


class CountBareTest(unittest.TestCase):
    def test_char_literal_holding_a_quote_does_not_hide_a_later_call(self) -> None:
        """The #1192 reproducer.

        Before the fix the `b'"'` opened a string span that ran to the
        next `"` anywhere later in the file, so the bare call between
        them was invisible and the gate reported zero.
        """
        source = """\
#[test]
fn t() {
    let quote = b'"';
    insta::assert_json_snapshot!(metric.cognitive);
    let _ = quote;
}
"""
        self.assertEqual(gate.count_bare_in_source(source), 1)

    def test_lifetime_does_not_swallow_the_call_that_follows_it(self) -> None:
        """Three lifetimes, not two, and a real char literal after the call.

        The count of quotes before the call is what makes this
        discriminate. A *greedy* variant that scans from one `'` to the
        next pairs them off, so with an even number of lifetimes ahead
        of the call every span closes before reaching it and the test
        passes against the bug. With three, the last lifetime pairs with
        the `'z'` below and the span swallows the call: 1 -> 0.
        """
        source = """\
fn f<'a>(x: &'a str, y: &'a str) {
    insta::assert_json_snapshot!(metric.x);
    let c = 'z';
}
"""
        self.assertEqual(gate.count_bare_in_source(source), 1)

    def test_byte_raw_string_does_not_hide_a_later_call(self) -> None:
        """The raw string holds an *odd* number of double quotes.

        `br#"a"#` alone proves nothing: with the byte-raw branch removed
        the inner `"a"` still lexes as a balanced regular string and the
        call stays visible. An unpaired quote is what makes the missing
        branch bite.
        """
        source = 'let s = br#"a"b"#;\ninsta::assert_json_snapshot!(metric.x);\n'
        self.assertEqual(gate.count_bare_in_source(source), 1)

    def test_char_literal_in_the_arguments_does_not_borrow_a_later_anchor(self) -> None:
        """`find_macro_call_end` carries the same lexer and the same gap.

        A quote-bearing char literal in the argument list ran the body
        scan past the real closing paren and into the *next* call, whose
        `@"…"` the bare call then claimed as its own anchor — so the
        pair counted 0 instead of 1. The second call must be anchored
        for this to discriminate; that is the anchor being stolen.
        """
        source = (
            "insta::assert_json_snapshot!(m.x(b'\"'));\n"
            'insta::assert_json_snapshot!(m.y, @"1");\n'
        )
        self.assertEqual(gate.count_bare_in_source(source), 1)

    def test_anchored_forms_stay_uncounted(self) -> None:
        for name, source in [
            ("non-raw inline", 'insta::assert_json_snapshot!(m.x, @"123");'),
            ("raw inline", 'insta::assert_json_snapshot!(m.x, @r###"1"###);'),
            ("assert_eq above", "assert_eq!(m.branches(), 2);\ninsta::assert_json_snapshot!(m.x);"),
            ("expected comment", "// expected: 2 branches\ninsta::assert_json_snapshot!(m.x);"),
            ("line-commented out", "// insta::assert_json_snapshot!(m.x);\n"),
            ("block-commented out", "/* insta::assert_json_snapshot!(m.x); */"),
        ]:
            with self.subTest(name=name):
                self.assertEqual(gate.count_bare_in_source(source), 0)

    def test_bare_call_is_counted(self) -> None:
        self.assertEqual(gate.count_bare_in_source("insta::assert_json_snapshot!(m.x);"), 1)

    def test_slashes_in_a_string_do_not_mask_a_bare_call(self) -> None:
        source = 'let url = "http://x"; insta::assert_json_snapshot!(m.x);\n'
        self.assertEqual(gate.count_bare_in_source(source), 1)


class BaselineTest(unittest.TestCase):
    """`main()` over a synthetic tree: pass, regress and ratchet."""

    def _run(self, argv: list[str]) -> tuple[int, str, str]:
        out, err = io.StringIO(), io.StringIO()
        old = sys.argv
        sys.argv = ["check-snapshot-anchors.py", *argv]
        try:
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                code = gate.main()
        finally:
            sys.argv = old
        return code, out.getvalue(), err.getvalue()

    def test_pass_regress_and_ratchet(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            target = root / "m.rs"
            baseline = root / "baseline.txt"
            target.write_text("insta::assert_json_snapshot!(m.x);\n", encoding="utf-8")

            code, _, _ = self._run([str(target), "--baseline", str(baseline), "--update"])
            self.assertEqual(code, 0)
            self.assertIn("1", baseline.read_text(encoding="utf-8"))

            code, _, _ = self._run([str(target), "--baseline", str(baseline)])
            self.assertEqual(code, 0, "unchanged tree must pass")

            target.write_text(
                "insta::assert_json_snapshot!(m.x);\ninsta::assert_json_snapshot!(m.y);\n",
                encoding="utf-8",
            )
            code, _, err = self._run([str(target), "--baseline", str(baseline)])
            self.assertEqual(code, 1, "an increase must fail")
            self.assertIn("baseline 1, current 2", err)

            target.write_text('insta::assert_json_snapshot!(m.x, @"1");\n', encoding="utf-8")
            code, _, _ = self._run([str(target), "--baseline", str(baseline)])
            self.assertEqual(code, 0, "a decrease must pass silently")

    def test_zero_counts_are_omitted_from_the_written_baseline(self) -> None:
        """An unlisted file is allowed zero via `.get(rel, 0)`.

        Pinned because the omission is what keeps the baseline a list of
        debt after #1192 made the scan recursive over 126 more files.
        """
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            clean = root / "clean.rs"
            baseline = root / "baseline.txt"
            clean.write_text("fn f() {}\n", encoding="utf-8")

            self._run([str(clean), "--baseline", str(baseline), "--update"])
            self.assertNotIn("clean.rs", baseline.read_text(encoding="utf-8"))

            clean.write_text("insta::assert_json_snapshot!(m.x);\n", encoding="utf-8")
            code, _, err = self._run([str(clean), "--baseline", str(baseline)])
            self.assertEqual(code, 1, "an unlisted file must still be held at zero")
            self.assertIn("baseline 0, current 1", err)


class RepositoryTest(unittest.TestCase):
    def test_default_targets_reach_the_metric_subdirectories(self) -> None:
        """#1192: the scan was non-recursive and missed 126 files.

        Asserts a positive property rather than a count, so splitting or
        merging a per-language module cannot make it stale.
        """
        targets = gate.default_targets()
        self.assertTrue(targets, "no metric files found")
        nested = [p for p in targets if p.parent != gate.METRICS_DIR]
        self.assertTrue(nested, "subdirectories of src/metrics/ are unscanned")

    def test_repository_is_clean_against_its_baseline(self) -> None:
        code, _, err = BaselineTest()._run([])
        self.assertEqual(code, 0, f"repository gate failed:\n{err}")


if __name__ == "__main__":
    unittest.main()
