#!/usr/bin/env python3
"""Tests for check-excluded-manifests.py.

Three kinds of test, matching the check-versions-test.py pattern:

* Unit tests against synthetic manifests, weighted toward the legal
  TOML shapes the gate's regex predecessor mis-read: literal strings,
  commented-out entries, inline arrays, indented keys, and the text
  ``[workspace]`` inside a multi-line string, plus the ``[lints]``-table
  rule's shapes (absent, empty, ``workspace = true``, exempt).
* ``main()`` tests over a synthetic repository root, covering both
  error branches and the remediation text they print.
* A smoke test running the real gate against the real repository,
  asserting a clean tree reports OK.

Run with:
    python3 -m unittest -q utils/check-excluded-manifests-test.py
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
SCRIPT_SRC = UTILS_DIR / "check-excluded-manifests.py"


def _load_module():  # type: ignore[no-untyped-def]
    spec = importlib.util.spec_from_file_location("check_excluded_manifests", SCRIPT_SRC)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


GATE = _load_module()

ROOT_MANIFEST_SAMPLE = """\
[workspace]
members = [
  "big-code-analysis-cli",
]
exclude = [
  "enums",
  "tree-sitter-tcl",
  # Claude Code worktrees live here.
  ".claude/worktrees",
]

[workspace.package]
version = "2.1.0"
"""


def excluded_crate(extra_table: str = "", name: str = "a") -> str:
    """A minimal excluded-crate manifest that clears every check.

    It roots its own workspace (#1145) and declares its own lint set
    (#1228), so a `main()` test can splice in `extra_table` as the one
    defect it is actually about. Passed as a whole table block, since a
    bare key appended after `[lints.clippy]` would land inside it.
    """
    return (
        f'[package]\nname = "{name}"\n'
        f"{extra_table}"
        "\n[workspace]\n"
        '\n[lints.clippy]\npedantic = { level = "warn", priority = -1 }\n'
    )


class ReadExcludedCratesTest(unittest.TestCase):
    def test_reads_crate_paths_and_drops_the_worktree_directory(self) -> None:
        self.assertEqual(
            GATE.read_excluded_crates(ROOT_MANIFEST_SAMPLE),
            ["enums", "tree-sitter-tcl"],
        )

    def test_does_not_pick_up_the_members_array(self) -> None:
        # `members` sits above `exclude` and holds a quoted entry too; a
        # regex anchored on the wrong array would return it.
        self.assertNotIn(
            "big-code-analysis-cli", GATE.read_excluded_crates(ROOT_MANIFEST_SAMPLE)
        )

    def test_absent_exclude_array_is_a_hard_error(self) -> None:
        with self.assertRaises(SystemExit):
            GATE.read_excluded_crates('[workspace]\nmembers = ["a"]\n')

    def test_literal_string_entries_are_read(self) -> None:
        # TOML literal strings are legal Cargo syntax. The regex gate
        # matched only double quotes, so a `'enums'` entry dropped out
        # of the check entirely without any signal.
        self.assertEqual(
            GATE.read_excluded_crates(
                "[workspace]\nexclude = [\n  'enums',\n  \"tree-sitter-tcl\",\n]\n"
            ),
            ["enums", "tree-sitter-tcl"],
        )

    def test_a_commented_out_entry_is_not_a_crate(self) -> None:
        # Commenting an entry out is how a retired crate leaves the
        # array. The regex gate returned the comment's quoted text as a
        # crate path and then died on the missing manifest.
        self.assertEqual(
            GATE.read_excluded_crates(
                '[workspace]\nexclude = [\n  "enums",\n  # "tree-sitter-retired",\n]\n'
            ),
            ["enums"],
        )

    def test_an_inline_array_does_not_swallow_the_next_array(self) -> None:
        # `exclude` on one line followed by any other multi-line array:
        # the regex ran non-greedily to the next line-initial `]` and
        # returned the *other* array's entries as excluded crates.
        self.assertEqual(
            GATE.read_excluded_crates(
                '[workspace]\nexclude = ["enums"]\nmembers = [\n  "cli",\n]\n'
            ),
            ["enums"],
        )

    def test_an_indented_exclude_key_is_found(self) -> None:
        # Leading whitespace before a key is valid TOML; the regex was
        # anchored at column zero and reported "could not locate".
        self.assertEqual(
            GATE.read_excluded_crates('[workspace]\n  exclude = ["enums"]\n'),
            ["enums"],
        )

    def test_a_non_string_entry_is_a_hard_error(self) -> None:
        with self.assertRaises(SystemExit):
            GATE.read_excluded_crates("[workspace]\nexclude = [1]\n")


class MissingWorkspaceTableTest(unittest.TestCase):
    def _root_with(self, manifest_body: str) -> pathlib.Path:
        root = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        (root / "grammar").mkdir()
        (root / "grammar" / "Cargo.toml").write_text(manifest_body, encoding="utf-8")
        return root

    def test_manifest_with_a_workspace_table_passes(self) -> None:
        root = self._root_with('[package]\nname = "g"\n\n[workspace]\n')
        self.assertEqual(GATE.missing_workspace_table(["grammar"], root), [])

    def test_manifest_without_one_is_reported(self) -> None:
        root = self._root_with('[package]\nname = "g"\n')
        self.assertEqual(GATE.missing_workspace_table(["grammar"], root), ["grammar"])

    def test_any_workspace_key_roots_a_workspace(self) -> None:
        # Cargo keys on the presence of a `workspace` key, not on the
        # bare header. Probed on cargo 1.95.0 against a sub-package
        # under an unrelated workspace root: with no workspace table it
        # exits 101 ("current package believes it's in a workspace when
        # it's not"), and each of these three headers alone makes
        # `cargo metadata` exit 0. An earlier revision of this test
        # asserted the opposite and encoded cargo semantics that do not
        # exist.
        for header in (
            "[workspace.package]\nversion = \"0.1.0\"",
            "[workspace.dependencies]\nserde = \"1\"",
            "[workspace.lints]\nrust.unsafe_code = \"forbid\"",
        ):
            with self.subTest(header=header.splitlines()[0]):
                root = self._root_with(f'[package]\nname = "g"\n\n{header}\n')
                self.assertEqual(GATE.missing_workspace_table(["grammar"], root), [])

    def test_a_bracketed_workspace_inside_a_string_is_not_a_table(self) -> None:
        # The regex gate matched the *text* `[workspace]` wherever it
        # sat, including inside a multi-line string, and reported a
        # manifest that roots no workspace as compliant.
        root = self._root_with(
            '[package]\nname = "g"\ndescription = """\n[workspace]\n"""\n'
        )
        self.assertEqual(GATE.missing_workspace_table(["grammar"], root), ["grammar"])

    def test_a_spaced_workspace_header_counts(self) -> None:
        # `[ workspace ]` is valid TOML; the regex demanded the exact
        # eleven characters and reported a rooted crate as an offender.
        root = self._root_with('[package]\nname = "g"\n\n[ workspace ]\n')
        self.assertEqual(GATE.missing_workspace_table(["grammar"], root), [])

    def test_missing_manifest_is_a_hard_error(self) -> None:
        root = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        with self.assertRaises(SystemExit):
            GATE.missing_workspace_table(["absent"], root)


class LintsTableTest(unittest.TestCase):
    """The `[lints]`-table invariant (#1228).

    `[workspace.lints]` reaches members only, so an excluded crate that
    omits the table builds on compiler defaults while `make enums-check`
    still runs it at `-D warnings` — a gate that reads as complete and
    is not.
    """

    def _root_with(self, crates: dict[str, str]) -> pathlib.Path:
        root = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        for name, body in crates.items():
            (root / name).mkdir(parents=True)
            (root / name / "Cargo.toml").write_text(body, encoding="utf-8")
        return root

    def test_a_clippy_table_satisfies_the_rule(self) -> None:
        self.assertIsNone(
            GATE.lints_table_problem(
                '[package]\nname = "g"\n\n[lints.clippy]\npedantic = "warn"\n',
                "g/Cargo.toml",
            )
        )

    def test_a_rust_only_table_satisfies_the_rule(self) -> None:
        # `[lints.rust]` alone is a real posture (the crate may have no
        # clippy carve-outs); the gate asserts a declared lint set, not a
        # particular one.
        self.assertIsNone(
            GATE.lints_table_problem(
                '[package]\nname = "g"\n\n[lints.rust]\nmissing_docs = "warn"\n',
                "g/Cargo.toml",
            )
        )

    def test_no_table_at_all_is_reported(self) -> None:
        self.assertEqual(
            GATE.lints_table_problem('[package]\nname = "g"\n', "g/Cargo.toml"),
            "no [lints] table",
        )

    def test_an_empty_table_is_reported(self) -> None:
        # `[lints]` with nothing under it changes no lint level, so it
        # must not buy the crate a pass.
        self.assertEqual(
            GATE.lints_table_problem(
                '[package]\nname = "g"\n\n[lints]\n', "g/Cargo.toml"
            ),
            "no [lints] table",
        )

    def test_workspace_inheritance_is_reported_distinctly(self) -> None:
        # The spelling every member crate uses, and the one most likely
        # to be copied in. An excluded crate roots its own workspace, so
        # it inherits nothing from the repository root.
        problem = GATE.lints_table_problem(
            '[package]\nname = "g"\n\n[lints]\nworkspace = true\n', "g/Cargo.toml"
        )
        self.assertIsNotNone(problem)
        assert problem is not None
        self.assertIn("workspace = true", problem)

    def test_a_non_exempt_crate_without_a_table_is_collected(self) -> None:
        root = self._root_with({"a": '[package]\nname = "a"\n'})
        self.assertEqual(GATE.unlinted_crates(["a"], root), [("a", "no [lints] table")])

    def test_an_exempt_vendored_grammar_is_skipped(self) -> None:
        # The five vendored forks hold generated binding boilerplate that
        # a regeneration replaces wholesale; the exemption is a recorded
        # decision, not an oversight.
        exempt = sorted(GATE.LINTS_EXEMPT_CRATES)[0]
        root = self._root_with({exempt: f'[package]\nname = "{exempt}"\n'})
        self.assertEqual(GATE.unlinted_crates([exempt], root), [])

    def test_the_exempt_set_names_only_the_vendored_grammars(self) -> None:
        # Non-vacuity guard: the rule would also "pass" if every excluded
        # crate were exempt. Pin that `enums` — the crate #1228 is about
        # — is the one the real repository actually checks.
        crates = GATE.read_excluded_crates(
            (REPO_ROOT / "Cargo.toml").read_text(encoding="utf-8")
        )
        self.assertEqual(
            [c for c in crates if c not in GATE.LINTS_EXEMPT_CRATES], ["enums"]
        )


class UnpinnedGrammarDepsTest(unittest.TestCase):
    def test_equals_pins_pass_in_both_spellings(self) -> None:
        manifest = (
            "[dependencies]\n"
            'tree-sitter-language="=0.1.0"\n'
            'tree-sitter-cpp = "=0.23.4"\n'
            'tree-sitter-tcl = { package = "bca-tree-sitter-tcl", '
            'path = "../tree-sitter-tcl", version = "=2.1.0" }\n'
        )
        self.assertEqual(GATE.unpinned_grammar_deps(manifest), [])

    def test_caret_range_without_spaces_is_caught(self) -> None:
        # The no-space spelling is the case that matters most: #1151's
        # table missed four entries written exactly this way.
        self.assertEqual(
            GATE.unpinned_grammar_deps('[dependencies]\ntree-sitter-perl="1.1.2"\n'),
            [("tree-sitter-perl", "1.1.2")],
        )

    def test_the_language_shim_is_exempt_from_pinning(self) -> None:
        # `tree-sitter-language` must stay caret-ranged: `=0.1.0` makes
        # the workspace unresolvable against `tree-sitter-irules`'
        # `^0.1.7`, and an `=` pin on a shared shim in a *published*
        # crate breaks downstream resolution (#1151).
        for spelling in ('tree-sitter-language="0.1.0"', 'tree-sitter-language = "0.1.0"'):
            with self.subTest(spelling=spelling):
                self.assertEqual(
                    GATE.unpinned_grammar_deps("[dependencies]\n" + spelling + "\n"), []
                )

    def test_the_runtime_crate_is_not_exempt(self) -> None:
        # `tree-sitter` itself stays pin-required, unlike the shim: it
        # has no unification pressure here (one external dependent in
        # the lockfile) and its ABI version is what each vendored
        # `parser.c` was generated against.
        self.assertEqual(
            GATE.unpinned_grammar_deps('[dev-dependencies]\ntree-sitter = "^0.26"\n'),
            [("tree-sitter", "^0.26")],
        )

    def test_caret_range_with_spaces_is_caught(self) -> None:
        self.assertEqual(
            GATE.unpinned_grammar_deps('[dependencies]\ntree-sitter-cpp = "0.23.4"\n'),
            [("tree-sitter-cpp", "0.23.4")],
        )

    def test_a_literal_string_requirement_is_read(self) -> None:
        # The regex gate matched double-quoted values only, so this
        # unpinned grammar passed the gate as if it were not a
        # dependency at all.
        self.assertEqual(
            GATE.unpinned_grammar_deps("[dependencies]\ntree-sitter-cpp = '0.23.4'\n"),
            [("tree-sitter-cpp", "0.23.4")],
        )

    def test_a_name_prefixed_grammar_is_checked(self) -> None:
        # `dekobon-tree-sitter-groovy` is a live entry in both the root
        # and `enums` manifests; the `^tree-sitter` anchor never reached
        # it, and would not reach a future `bca-tree-sitter-*` either.
        self.assertEqual(
            GATE.unpinned_grammar_deps(
                "[dependencies]\n"
                'dekobon-tree-sitter-groovy = "^0.2.2"\n'
                'bca-tree-sitter-mozjs = "2.1.0"\n'
            ),
            [("dekobon-tree-sitter-groovy", "^0.2.2"), ("bca-tree-sitter-mozjs", "2.1.0")],
        )

    def test_inline_table_without_a_pin_is_caught(self) -> None:
        self.assertEqual(
            GATE.unpinned_grammar_deps(
                '[dependencies]\ntree-sitter-tcl = { path = "../x", version = "2.1.0" }\n'
            ),
            [("tree-sitter-tcl", "2.1.0")],
        )

    def test_a_workspace_inherited_entry_carries_no_requirement(self) -> None:
        # Member crates take grammars through `workspace = true`; the
        # requirement they inherit lives in the root manifest, which the
        # gate checks directly. Flagging these would be a false hit.
        manifest = (
            "[dependencies]\n"
            "tree-sitter.workspace = true\n"
            "tree-sitter-bash = { workspace = true, optional = true }\n"
        )
        self.assertEqual(GATE.unpinned_grammar_deps(manifest), [])

    def test_non_grammar_dependencies_are_out_of_scope(self) -> None:
        # The pinning rule is about grammars; `cc`, `clap` and `askama`
        # deliberately float.
        manifest = '[build-dependencies]\ncc = "^1.2"\nclap = "^4.0"\naskama = "^0.16"\n'
        self.assertEqual(GATE.unpinned_grammar_deps(manifest), [])

    def test_udeps_ignore_entry_is_not_a_dependency(self) -> None:
        # `build = ["tree-sitter-cpp"]` under
        # [package.metadata.cargo-udeps.ignore] names a grammar but is
        # not a version requirement — matching it would be a false hit.
        manifest = (
            "[package.metadata.cargo-udeps.ignore]\nbuild = [\"tree-sitter-cpp\"]\n"
        )
        self.assertEqual(GATE.unpinned_grammar_deps(manifest), [])

    def test_a_commented_out_dependency_line_is_not_a_dependency(self) -> None:
        # The regex gate needed a `^\s*` anchor to keep a commented-out
        # line out of the dependency set, and nothing exercised it.
        # tomllib cannot see a comment at all; this test fails against
        # any return to line matching.
        manifest = '[dependencies]\n# tree-sitter-cpp = "0.23.4"\ncc = "^1.2"\n'
        self.assertEqual(GATE.unpinned_grammar_deps(manifest), [])

    def test_a_grammar_named_key_outside_a_dependency_table_is_ignored(self) -> None:
        # A version-shaped value under a non-dependency table is not a
        # requirement. The regex gate read it as one; the walk only
        # honours `dependencies` / `build-dependencies` /
        # `dev-dependencies`.
        manifest = '[package.metadata.grammar-audit]\ntree-sitter-cpp = "0.23.4"\n'
        self.assertEqual(GATE.unpinned_grammar_deps(manifest), [])

    def test_nested_dependency_tables_are_reached(self) -> None:
        # `[workspace.dependencies]` holds the root manifest's ~20
        # grammar pins, and `[target.'cfg(...)'.build-dependencies]` is
        # the deepest shape cargo permits. Both must be walked.
        manifest = (
            "[workspace.dependencies]\n"
            'tree-sitter-python = "^0.25.0"\n\n'
            "[target.'cfg(unix)'.build-dependencies]\n"
            'tree-sitter-lua = "0.5.0"\n'
        )
        self.assertEqual(
            GATE.unpinned_grammar_deps(manifest),
            [("tree-sitter-python", "^0.25.0"), ("tree-sitter-lua", "0.5.0")],
        )


class PinSuggestionTest(unittest.TestCase):
    def test_a_bare_version_is_repaired_by_prepending_equals(self) -> None:
        self.assertEqual(GATE.pin_suggestion("0.23.4"), 'should be "=0.23.4"')

    def test_a_range_is_not_repaired_by_prepending_equals(self) -> None:
        # `should be "=^0.23.4"` / `"=>=0.23, <0.24"` / `"=*"` are all
        # requirements cargo rejects; the old message printed each one.
        for requirement in ("^0.23.4", ">=0.23, <0.24", "*"):
            with self.subTest(requirement=requirement):
                message = GATE.pin_suggestion(requirement)
                self.assertEqual(
                    message, 'replace the range with an exact "=X.Y.Z" pin'
                )

    def test_a_spaced_equals_counts_as_a_pin(self) -> None:
        # `= 0.25.0` is what check-grammar-marker-sync.py reduces to the
        # bare `0.25.0`; the two gates must agree that it is a pin.
        self.assertTrue(GATE.is_exact_pin("= 0.25.0"))

    def test_a_compound_requirement_is_not_a_pin(self) -> None:
        # `=0.25.0, <0.26` names no single version, so the grammar-
        # marker baseline cannot record it. Accepting it here would let
        # a manifest through that the marker gate then reports as drift.
        self.assertFalse(GATE.is_exact_pin("=0.25.0, <0.26"))


class MainTest(unittest.TestCase):
    """`main()` over a synthetic repository root.

    Both error branches, the multi-offender join, and the remediation
    text were previously unexecuted by any test.
    """

    def _repo(self, root_manifest: str, crates: dict[str, str]) -> pathlib.Path:
        root = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        (root / "Cargo.toml").write_text(root_manifest, encoding="utf-8")
        for name, body in crates.items():
            (root / name).mkdir(parents=True)
            (root / name / "Cargo.toml").write_text(body, encoding="utf-8")
        self.enterContext(mock.patch.object(GATE, "REPO_ROOT", root))
        self.enterContext(
            mock.patch.object(GATE, "ROOT_MANIFEST", root / "Cargo.toml")
        )
        return root

    def _run(self) -> tuple[int, str]:
        err = io.StringIO()
        with contextlib.redirect_stderr(err), contextlib.redirect_stdout(io.StringIO()):
            code = GATE.main()
        return code, err.getvalue()

    def test_a_clean_tree_returns_zero(self) -> None:
        self._repo(
            '[workspace]\nexclude = ["a"]\n\n'
            '[workspace.dependencies]\ntree-sitter-cpp = "=0.23.4"\n',
            {"a": excluded_crate()},
        )
        code, err = self._run()
        self.assertEqual(code, 0, err)

    def test_an_excluded_crate_without_a_lints_table_is_reported(self) -> None:
        self._repo(
            '[workspace]\nexclude = ["a"]\n',
            {"a": '[package]\nname = "a"\n\n[workspace]\n'},
        )
        code, err = self._run()
        self.assertEqual(code, 1)
        self.assertIn("a/Cargo.toml: no [lints] table", err)
        self.assertIn("#1228", err)

    def test_a_lints_workspace_true_in_an_excluded_crate_is_reported(self) -> None:
        # Inheriting is what a *member* does; here it resolves to nothing.
        self._repo(
            '[workspace]\nexclude = ["a"]\n',
            {
                "a": '[package]\nname = "a"\n\n[workspace]\n\n[lints]\nworkspace = true\n'
            },
        )
        code, err = self._run()
        self.assertEqual(code, 1)
        self.assertIn("workspace = true", err)

    def test_an_exempt_crate_needs_no_lints_table(self) -> None:
        exempt = sorted(GATE.LINTS_EXEMPT_CRATES)[0]
        self._repo(
            f'[workspace]\nexclude = ["{exempt}"]\n',
            {exempt: f'[package]\nname = "{exempt}"\n\n[workspace]\n'},
        )
        code, err = self._run()
        self.assertEqual(code, 0, err)

    def test_missing_workspace_tables_are_listed_together(self) -> None:
        self._repo(
            '[workspace]\nexclude = ["a", "b"]\n',
            {
                "a": '[package]\nname = "a"\n',
                "b": '[package]\nname = "b"\n',
            },
        )
        code, err = self._run()
        self.assertEqual(code, 1)
        self.assertIn("a/Cargo.toml", err)
        self.assertIn("b/Cargo.toml", err)
        self.assertIn("#1145", err)

    def test_an_unpinned_grammar_in_an_excluded_crate_is_reported(self) -> None:
        self._repo(
            '[workspace]\nexclude = ["a"]\n',
            {"a": excluded_crate('\n[dependencies]\ntree-sitter-cpp = "0.23.4"\n')},
        )
        code, err = self._run()
        self.assertEqual(code, 1)
        self.assertIn('a/Cargo.toml: tree-sitter-cpp = "0.23.4"', err)
        self.assertIn('should be "=0.23.4"', err)

    def test_an_unpinned_grammar_in_the_root_manifest_is_reported(self) -> None:
        # AGENTS.md claims this gate enforces the root manifest's pins.
        # Before the root was added to the scan, loosening one there was
        # invisible.
        self._repo(
            '[workspace]\nexclude = ["a"]\n\n'
            '[workspace.dependencies]\ntree-sitter-python = "^0.25.0"\n',
            {"a": excluded_crate()},
        )
        code, err = self._run()
        self.assertEqual(code, 1)
        self.assertIn('Cargo.toml: tree-sitter-python = "^0.25.0"', err)
        self.assertIn('replace the range with an exact "=X.Y.Z" pin', err)

    def test_a_range_requirement_is_not_suggested_back_with_an_equals(self) -> None:
        self._repo(
            '[workspace]\nexclude = ["a"]\n\n'
            '[workspace.dependencies]\ntree-sitter-go = ">=0.23, <0.24"\n',
            {"a": excluded_crate()},
        )
        code, err = self._run()
        self.assertEqual(code, 1)
        self.assertNotIn('"=>=0.23, <0.24"', err)
        self.assertIn('replace the range with an exact "=X.Y.Z" pin', err)


class RealRepositoryTest(unittest.TestCase):
    def test_clean_tree_passes(self) -> None:
        result = subprocess.run(
            [sys.executable, str(SCRIPT_SRC)],
            capture_output=True,
            text=True,
            cwd=REPO_ROOT,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Excluded manifests OK", result.stdout)
        # The lint-checked tally is what distinguishes a real pass from
        # one where every excluded crate happened to be exempt.
        self.assertIn("1 lint-checked", result.stdout)


if __name__ == "__main__":
    unittest.main()
