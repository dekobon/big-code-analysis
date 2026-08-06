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

    def test_both_offenders_on_one_line_are_reported(self) -> None:
        # `re.search` stops at the first match, so the author fixes the
        # one named, re-runs the gate, and it fails again on the same
        # line — with a header that had undercounted.
        text = 'let a = "Warning: one"; let b = "Error: two";\n'
        self.assertEqual(
            [(1, "Warning"), (1, "Error")], [(n, w) for n, w, _ in GATE.scan_text(text)]
        )

    def test_a_single_line_raw_string_is_still_scanned(self) -> None:
        # Only the *interior* of a multi-line raw string is skipped: a
        # one-line `r#"…"#` is as plausible a diagnostic as any other
        # literal, so the fixture carve-out must not swallow it.
        self.assertEqual(
            [(1, "Error")],
            [(n, w) for n, w, _ in GATE.scan_text('die(r#"Error: x"#);\n')],
        )

    def test_scanning_resumes_after_a_raw_string_closes(self) -> None:
        text = 'let f = r#"\n  int x;\n"#; eprintln!("Error: real");\n'
        self.assertEqual([(3, "Error")], [(n, w) for n, w, _ in GATE.scan_text(text)])


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

    def test_escaped_inner_quote_is_prose_not_a_prefix(self) -> None:
        # An escaped quote never *starts* a literal, so what follows it
        # is prose. Anchoring on any `"` reported this as an offender
        # with no correct remediation.
        self.assertEqual(GATE.scan_text('let m = "he said \\"Error: no\\"";\n'), [])

    def test_multiline_raw_string_fixture_is_skipped(self) -> None:
        # The embedded-source-fixture shape this workspace is built on.
        # Neither opt-out position is reachable inside a raw string — both
        # would land in the fixture and change what the metric test
        # measures — so flagging it left no correct remediation.
        text = 'let src = r#"\n    std::cerr << "Warning: low memory";\n"#;\n'
        self.assertEqual(GATE.scan_text(text), [])

    def test_an_escaped_cr_does_not_open_a_raw_string(self) -> None:
        # `\r"` and `"r"` both end in the raw-string opener's characters
        # without being one. Reading either as an open skips every line
        # until the next quote, and the offender hiding in there is a
        # false *clean* — the one outcome this gate exists to prevent.
        # Both shapes are live in this tree (`src/tools.rs:409`,
        # `src/languages/language_ruby.rs:481`).
        text = 'let l = s.strip_suffix(b"\\r").unwrap_or(s);\neprintln!("Error: x");\n'
        self.assertEqual([(2, "Error")], [(n, w) for n, w, _ in GATE.scan_text(text)])

    def test_a_string_holding_only_r_does_not_open_a_raw_string(self) -> None:
        text = 'Ruby::R => "r",\neprintln!("Error: x");\n'
        self.assertEqual([(2, "Error")], [(n, w) for n, w, _ in GATE.scan_text(text)])

    def test_a_string_ending_in_r_does_not_open_a_raw_string(self) -> None:
        # `"dir/r"` closes an ordinary string, but its final `r"` is a
        # textbook raw-string opener and `/` is a legitimate context for
        # one, so the old regex read it as opening a multi-line raw
        # string and skipped everything to the next quote (#1219). No
        # lookbehind can express the difference — what distinguishes this
        # case is that the `"` *closes* a literal, which is state.
        text = 'let p = "dir/r";\neprintln!("Warning: hidden");\n'
        self.assertEqual([(2, "Warning")], [(n, w) for n, w, _ in GATE.scan_text(text)])

    def test_a_string_ending_in_a_plain_letter_is_the_control(self) -> None:
        # The near miss that discriminates. It passes against the old
        # scanner too — that is the point: it is what stops the case
        # above from being satisfied by a gate that had simply given up
        # on raw strings altogether, which would "fix" #1219 by removing
        # the skip that the multi-line-fixture rule depends on.
        text = 'let p = "dir/x";\neprintln!("Warning: hidden");\n'
        self.assertEqual([(2, "Warning")], [(n, w) for n, w, _ in GATE.scan_text(text)])

    def test_a_trailing_line_comment_cannot_open_a_raw_string(self) -> None:
        # The old scanner skipped a line only when the *whole* line was a
        # comment, so an unterminated `r"` in a trailing one opened a
        # phantom span (#1219). Stripping comments by regex first is not
        # the fix — see the `http://` case below.
        text = 'let x = 1; // e.g. r"foo\neprintln!("Warning: hidden");\n'
        self.assertEqual([(2, "Warning")], [(n, w) for n, w, _ in GATE.scan_text(text)])

    def test_a_block_comment_cannot_open_a_raw_string(self) -> None:
        # The third window of the same class, which the issue did not
        # name: block comments were invisible to the line-oriented
        # scanner in either position, whole-line or trailing.
        text = 'let x = 1; /* r"foo */\neprintln!("Warning: hidden");\n'
        self.assertEqual([(2, "Warning")], [(n, w) for n, w, _ in GATE.scan_text(text)])

    def test_a_url_in_a_literal_is_not_read_as_a_comment(self) -> None:
        # Passes against the old scanner as well; it guards the *fix*
        # rather than the bug. The cheap route to #1219's window 2 is to
        # strip trailing comments with a regex, and that route cuts
        # `"http://x"` mid-literal, leaving an unbalanced quote that
        # opens a phantom span of its own — one false-clean window traded
        # for another. This fails the moment anyone reaches for it.
        text = 'let u = "http://x";\neprintln!("Error: y");\n'
        self.assertEqual([(2, "Error")], [(n, w) for n, w, _ in GATE.scan_text(text)])

    def test_a_char_literal_holding_a_quote_does_not_open_a_string(self) -> None:
        # Inherited from the ported lexer rather than fixed here: the
        # old scanner passed this too, having no notion of a char literal
        # but also no string-span tracking for one to corrupt. Pinned
        # because the port *introduced* the machinery that can get it
        # wrong — `b'"'` holds an unpaired double quote, and #1192 is the
        # sibling gate shipping exactly that bug.
        text = "let q = b'\"';\neprintln!(\"Error: z\");\n"
        self.assertEqual([(2, "Error")], [(n, w) for n, w, _ in GATE.scan_text(text)])

    def test_a_lifetime_does_not_open_a_char_span(self) -> None:
        # The other half of #1192's char-literal rule, and likewise a
        # pin on the new machinery rather than a #1219 regression guard:
        # a lifetime has the opening quote and no terminator, so reading
        # it as a literal swallows the rest of the file the other way.
        #
        # Three lifetimes, not two, and a real char literal after the
        # offender — the quote count before it is the whole discriminator.
        # A greedy variant that scans from one `'` to the next pairs them
        # off, so with an *even* number ahead of the offender every bogus
        # span closes before reaching it and the test passes against the
        # bug it names. With three, the last pairs with the `'z'` below
        # and the span swallows the offender. Copied deliberately from
        # `check-snapshot-anchors-test.py`, whose version of this test
        # records the same trap.
        text = (
            "fn f<'a>(x: &'a str, y: &'a str) {\n"
            '    eprintln!("Error: w");\n'
            "    let c = 'z';\n"
            "}\n"
        )
        self.assertEqual([(2, "Error")], [(n, w) for n, w, _ in GATE.scan_text(text)])

    def test_an_escaped_quote_does_not_hide_a_later_literal(self) -> None:
        # `regular_string_end` consumes `\"` as one escaped character, so
        # `"a\"b"` is a single span. Without that branch the span closes
        # at the inner quote and the *next* one reopens it, swallowing
        # everything to the following quote — a false clean of the same
        # class as #1219's windows. The obvious candidate for this
        # coverage, `test_escaped_inner_quote_is_prose_not_a_prefix`,
        # does not provide it: under the perturbation its input merely
        # splits into two spans that also yield no hit.
        text = 'let s = "a\\"b";\neprintln!("Error: real");\n'
        self.assertEqual([(2, "Error")], [(n, w) for n, w, _ in GATE.scan_text(text)])

    def test_severity_inside_a_trailing_comment_is_not_a_prefix(self) -> None:
        # A widening the lexer brings with it: before, only a *whole-line*
        # comment was skipped, so a severity quoted in a trailing one was
        # reported and needed a `diag-prefix-ok` marker. A comment is
        # never a diagnostic, so this is the same direction the whole-line
        # rule already took.
        self.assertEqual(GATE.scan_text('let x = 1; // never write "Warning: x"\n'), [])

    def test_severity_inside_a_block_comment_is_not_a_prefix(self) -> None:
        self.assertEqual(GATE.scan_text('/* never write "Warning: x" */\n'), [])

    def test_hash_count_must_match_to_close_a_raw_string(self) -> None:
        # A bare `"#` inside an `r##"…"##` fixture does not terminate it,
        # so the lines after it are still fixture data.
        text = 'let src = r##"\n  let a = "#;\n  eprintln!("Warning: fixture");\n"##;\n'
        self.assertEqual(GATE.scan_text(text), [])

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


class PortedLexerHelpers(unittest.TestCase):
    """Direct tests on the three helpers ported from check-snapshot-anchors.

    ``_scan_literals``' docstring tells the next reader to fix a lexing
    bug in one gate and check the other. That instruction needs something
    behind it: end-to-end ``scan_text`` cases do not fail when block-comment
    nesting, the unterminated-raw rule, or the lifetime rejection is
    perturbed in *this* copy, because no current input distinguishes them.
    These mirror the donor's ``CharLiteralEndTest`` / ``RawStringEndTest``
    so a divergence introduced here is caught here.
    """

    def test_char_literal_end_accepts_real_literals(self) -> None:
        for source, expected in (
            ("'a'", 3),
            ("'\"'", 3),
            ("'\\''", 4),
            ("'\\\\'", 4),
            ("'\\n'", 4),
            ("'\\x41'", 6),
            ("'\\u{1F600}'", 11),
        ):
            with self.subTest(source=source):
                self.assertEqual(GATE.char_literal_end(source, 0), expected)

    def test_char_literal_end_rejects_lifetimes_and_labels(self) -> None:
        # Returning None is what stops a lifetime opening a span that
        # swallows the rest of the file (#1192).
        for source in ("'a>", "'_,", "'outer:", "'static ", "'"):
            with self.subTest(source=source):
                self.assertIsNone(GATE.char_literal_end(source, 0))

    def test_raw_string_end_covers_every_spelling(self) -> None:
        for source, expected in (
            ('r"x"', 4),
            ('r#"x"#', 6),
            ('r##"x"##', 8),
            ('br"x"', 5),
            ('br##"x"##', 9),
            # A `"##` inside an `r###"…"###` does not close it.
            ('r###"a "## b"###', 16),
        ):
            with self.subTest(source=source):
                self.assertEqual(GATE.raw_string_end(source, 0), expected)

    def test_raw_string_end_rejects_non_raw_openers(self) -> None:
        for source in ('b"x"', "rust", "bar", "r", "r#"):
            with self.subTest(source=source):
                self.assertIsNone(GATE.raw_string_end(source, 0))

    def test_raw_string_end_runs_to_eof_when_unterminated(self) -> None:
        # The deliberate "skip to the close" behaviour the line-oriented
        # scanner had, now reachable only from real code position.
        source = 'r#"never closed'
        self.assertEqual(GATE.raw_string_end(source, 0), len(source))

    def test_regular_string_end_consumes_escapes(self) -> None:
        for source, expected in (
            ('"x"', 3),
            ('"a\\"b"', 6),
            ('"a\\\\"', 5),
            ('"\\n"', 4),
        ):
            with self.subTest(source=source):
                self.assertEqual(GATE.regular_string_end(source, 0), expected)

    def test_block_comments_nest(self) -> None:
        # Rust allows nesting; stopping at the first `*/` would leave the
        # trailing ` */` as code and a stray quote in it could open a span.
        text = '/* outer /* inner */ still comment "Error: x" */\neprintln!("Error: y");\n'
        self.assertEqual([(2, "Error")], [(n, w) for n, w, _ in GATE.scan_text(text)])


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
