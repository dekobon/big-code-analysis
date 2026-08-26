#!/usr/bin/env python3
"""Tests for check-safety-doc-pin.py.

Three kinds of test, matching the check-diagnostic-prefix-test.py
pattern:

* Unit tests over the two extractors, weighted toward the false-clean
  direction: a module doc the scanner fails to read reports no citations
  and, if the "no citation" branch were ever softened, would report a
  clean tree. How much of the file the scanner can see is therefore the
  property under test, and since #1345 the answer is "all of it" — the
  cases pin that nothing (blank line, single- or multi-line inner
  attribute, code, a nested ``mod``) truncates the scan, and that ``///``
  item docs stay out of scope.
* ``main()`` tests over a synthetic repository root, covering the clean,
  stale, and citation-dropped branches plus the unreadable-manifest exit.
* A smoke test running the real gate against the real repository,
  asserting a clean tree reports OK.

Run with:
    python3 -m unittest -q utils/check-safety-doc-pin-test.py
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
SCRIPT_SRC = UTILS_DIR / "check-safety-doc-pin.py"


def _load_module():  # type: ignore[no-untyped-def]
    spec = importlib.util.spec_from_file_location("check_safety_doc_pin", SCRIPT_SRC)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


GATE = _load_module()

# A module doc shaped like node.rs's: prose, a blank `//!` line, and the
# sendability claim carrying the version literal.
DOC_SOURCE = """\
//! Lazy `Node` handle over the tree retained by [`PyAst`].
//!
//! `tree_sitter::Tree`, `Node`, and `TreeCursor` are `Send + Sync` under
//! the pinned `=0.26.12`, so the pyclasses are sendable.

use pyo3::prelude::*;

/// Item doc citing `=9.9.9`, deliberately out of scope.
fn f() {}
"""

# The same doc with the version literal moved below a *multi-line* inner
# attribute, whose continuation and closing lines are ordinary non-doc
# code. This is the #1345 shape: the leading-block scanner stopped at
# `clippy::pedantic` and never saw the citation at all.
MULTILINE_ATTR_DOC = """\
//! Lazy `Node` handle over the tree retained by [`PyAst`].

#![allow(
    clippy::pedantic
)]

//! `tree_sitter::Tree`, `Node`, and `TreeCursor` are `Send + Sync` under
//! the pinned `=0.26.12`, so the pyclasses are sendable.

use pyo3::prelude::*;
"""


class ModuleDocLines(unittest.TestCase):
    """What the gate can and cannot see."""

    def test_collects_every_module_doc_line(self) -> None:
        lines = GATE.module_doc_lines(DOC_SOURCE)
        self.assertEqual([n for n, _ in lines], [1, 2, 3, 4])
        self.assertTrue(lines[3][1].endswith("the pyclasses are sendable."))

    def test_item_doc_below_is_out_of_scope(self) -> None:
        # The `///` citation must not be collected: the gate's contract
        # is the module-level soundness argument, and an item doc may
        # legitimately name another crate's version.
        bodies = [body for _, body in GATE.module_doc_lines(DOC_SOURCE)]
        self.assertFalse(any("9.9.9" in body for body in bodies))

    def test_inner_attribute_does_not_end_the_block(self) -> None:
        # A `#![allow(...)]` between doc paragraphs must not truncate the
        # scan: everything after it would silently stop being gated.
        source = "//! first\n#![allow(clippy::pedantic)]\n//! `=1.2.3`\n\nfn f() {}\n"
        self.assertEqual(
            GATE.cited_versions(GATE.module_doc_lines(source)), [(3, "1.2.3")]
        )

    def test_multi_line_inner_attribute_does_not_end_the_block(self) -> None:
        # The #1345 case the single-line rule above did not cover: the
        # continuation and `)]` lines of a wrapped `#![allow(…)]` are
        # ordinary non-doc code. A scanner that stops there hides every
        # citation below it *and still prints OK*, which is the exact
        # false-clean this gate exists to prevent.
        source = "//! first\n#![allow(\n    clippy::pedantic\n)]\n//! `=1.2.3`\n"
        self.assertEqual(
            GATE.cited_versions(GATE.module_doc_lines(source)), [(5, "1.2.3")]
        )

    def test_code_does_not_end_the_block(self) -> None:
        # Contract change in #1345: the scan has no stop condition, so a
        # `//!` below code — legal only as a nested module's own doc — is
        # in scope too. For this one file that is the desired rule: any
        # `=X.Y.Z` in a module doc in `node.rs` is a claim about the pin,
        # wherever it sits. The alternative was lexing `#![…]` bracket
        # depth to keep a stop condition that has now been wrong twice.
        source = (
            "//! `=1.2.3`\n"
            "\n"
            "use pyo3::prelude::*;\n"
            "\n"
            "mod inner {\n"
            "    //! `=4.5.6`\n"
            "}\n"
        )
        self.assertEqual(
            GATE.cited_versions(GATE.module_doc_lines(source)),
            [(1, "1.2.3"), (6, "4.5.6")],
        )


class CitedVersions(unittest.TestCase):
    def test_reads_the_backticked_literal(self) -> None:
        self.assertEqual(
            GATE.cited_versions(GATE.module_doc_lines(DOC_SOURCE)), [(4, "0.26.12")]
        )

    def test_unbackticked_version_is_ignored(self) -> None:
        # Prose such as "since 0.26.0 the cursor …" is not a citation of
        # the pin and must not fail the gate.
        source = "//! behaviour changed in =0.26.0 upstream\n"
        self.assertEqual(GATE.cited_versions(GATE.module_doc_lines(source)), [])

    def test_every_citation_is_reported(self) -> None:
        source = "//! `=1.2.3` and later `=1.2.4`\n"
        self.assertEqual(
            GATE.cited_versions(GATE.module_doc_lines(source)),
            [(1, "1.2.3"), (1, "1.2.4")],
        )


class PinnedVersion(unittest.TestCase):
    def test_string_form(self) -> None:
        manifest = '[workspace.dependencies]\ntree-sitter = "=0.26.12"\n'
        self.assertEqual(GATE.pinned_version(manifest), "0.26.12")

    def test_table_form(self) -> None:
        manifest = (
            "[workspace.dependencies]\n"
            'tree-sitter = { version = "=0.26.12", default-features = false }\n'
        )
        self.assertEqual(GATE.pinned_version(manifest), "0.26.12")

    def test_caret_range_is_rejected(self) -> None:
        manifest = '[workspace.dependencies]\ntree-sitter = "0.26"\n'
        with self.assertRaises(GATE.ManifestError):
            GATE.pinned_version(manifest)

    def test_missing_entry_is_rejected(self) -> None:
        manifest = '[workspace.dependencies]\ntree-sitter-c = "=0.24.1"\n'
        with self.assertRaises(GATE.ManifestError):
            GATE.pinned_version(manifest)


@contextlib.contextmanager
def _synthetic_repo(manifest: str, doc_source: str):  # type: ignore[no-untyped-def]
    """Point the gate at a throwaway root and capture its stderr/stdout."""
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        (root / "Cargo.toml").write_text(manifest, encoding="utf-8")
        doc = root / "big-code-analysis-py" / "src" / "node.rs"
        doc.parent.mkdir(parents=True)
        doc.write_text(doc_source, encoding="utf-8")
        out, err = io.StringIO(), io.StringIO()
        with (
            mock.patch.object(GATE, "REPO_ROOT", root),
            mock.patch.object(GATE, "ROOT_MANIFEST", root / "Cargo.toml"),
            mock.patch.object(GATE, "SAFETY_DOC", doc),
            contextlib.redirect_stdout(out),
            contextlib.redirect_stderr(err),
        ):
            yield lambda: (GATE.main(), out.getvalue(), err.getvalue())


PINNED = '[workspace.dependencies]\ntree-sitter = "=0.26.12"\n'


class Main(unittest.TestCase):
    def test_matching_citation_passes(self) -> None:
        with _synthetic_repo(PINNED, DOC_SOURCE) as run:
            code, out, err = run()
        self.assertEqual(code, 0)
        self.assertIn("safety-doc-pin: OK", out)
        self.assertEqual(err, "")

    def test_stale_citation_fails_and_names_the_line(self) -> None:
        stale = DOC_SOURCE.replace("=0.26.12", "=0.26.9")
        with _synthetic_repo(PINNED, stale) as run:
            code, _out, err = run()
        self.assertEqual(code, 1)
        self.assertIn("node.rs:4: cites `=0.26.9`, pin is `=0.26.12`", err)
        # The remediation must send the reader back to the argument, not
        # only to the literal — re-reading it is the point of the gate.
        self.assertIn("Re-read that", err)

    def test_matching_citation_below_a_multi_line_attribute_passes(self) -> None:
        # The other #1345 direction: when the *only* citation sits below
        # the attribute, a truncating scan sees none and fails the "no
        # longer cites" branch on a file that is correct.
        with _synthetic_repo(PINNED, MULTILINE_ATTR_DOC) as run:
            code, out, err = run()
        self.assertEqual(code, 0, err)
        self.assertIn("safety-doc-pin: OK", out)
        self.assertEqual(err, "")

    def test_stale_citation_below_a_multi_line_attribute_fails(self) -> None:
        stale = MULTILINE_ATTR_DOC.replace("=0.26.12", "=0.26.9")
        with _synthetic_repo(PINNED, stale) as run:
            code, _out, err = run()
        self.assertEqual(code, 1)
        # Asserted on the message, not just the exit code: a truncating
        # scan also exits 1 here, via the "no longer cites" branch. Only
        # the stale-citation line discriminates the two.
        self.assertIn("node.rs:8: cites `=0.26.9`, pin is `=0.26.12`", err)

    def test_dropping_the_citation_fails(self) -> None:
        symbolic = DOC_SOURCE.replace("the pinned `=0.26.12`", "the pinned version")
        with _synthetic_repo(PINNED, symbolic) as run:
            code, _out, err = run()
        self.assertEqual(code, 1)
        # Spelled out rather than matched loosely: the crate name is
        # not itself a version requirement, so `=tree-sitter` would be
        # a nonsense literal for the reader to go looking for.
        self.assertIn("no longer cites a `tree-sitter` version", err)
        self.assertIn("`=0.26.12`", err)

    def test_unreadable_pin_exits_two(self) -> None:
        with _synthetic_repo("[workspace.dependencies]\n", DOC_SOURCE) as run:
            code, _out, err = run()
        self.assertEqual(code, 2)
        self.assertIn("could not read the tree-sitter pin", err)

    def test_missing_safety_doc_exits_two(self) -> None:
        with _synthetic_repo(PINNED, DOC_SOURCE) as run:
            GATE.SAFETY_DOC.unlink()
            code, _out, err = run()
        self.assertEqual(code, 2)
        self.assertIn("could not read", err)


class RealRepository(unittest.TestCase):
    def test_gate_passes_on_the_checked_in_tree(self) -> None:
        result = subprocess.run(
            [sys.executable, str(SCRIPT_SRC)],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("safety-doc-pin: OK", result.stdout)


if __name__ == "__main__":
    unittest.main()
