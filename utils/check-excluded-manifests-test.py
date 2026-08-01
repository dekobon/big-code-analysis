#!/usr/bin/env python3
"""Tests for check-excluded-manifests.py.

Two kinds of test, matching the check-versions-test.py pattern:

* Unit tests against synthetic manifests, including the exact shapes
  that must **not** count as a workspace root — ``[workspace.package]``
  and ``[workspace.dependencies]`` both start with the same eleven
  characters, and neither terminates cargo's upward search.
* A smoke test running the real gate against the real repository,
  asserting a clean tree reports OK.

Run with:
    python3 -m unittest -q utils/check-excluded-manifests-test.py
"""

from __future__ import annotations

import importlib.util
import pathlib
import subprocess
import sys
import tempfile
import unittest

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

    def test_workspace_package_does_not_count_as_a_workspace_root(self) -> None:
        # The failure this gate exists to catch: `[workspace.package]`
        # and `[workspace.dependencies]` share a prefix with the bare
        # table but do not stop cargo's upward search.
        for header in ("[workspace.package]", "[workspace.dependencies]"):
            with self.subTest(header=header):
                root = self._root_with(f'[package]\nname = "g"\n\n{header}\nx = 1\n')
                self.assertEqual(
                    GATE.missing_workspace_table(["grammar"], root), ["grammar"]
                )

    def test_missing_manifest_is_a_hard_error(self) -> None:
        root = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        with self.assertRaises(SystemExit):
            GATE.missing_workspace_table(["absent"], root)


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
            GATE.unpinned_grammar_deps('tree-sitter-perl="1.1.2"\n'),
            [("tree-sitter-perl", "1.1.2")],
        )

    def test_the_language_shim_is_exempt_from_pinning(self) -> None:
        # `tree-sitter-language` must stay caret-ranged: `=0.1.0` makes
        # the workspace unresolvable against `tree-sitter-irules`'
        # `^0.1.7`, and an `=` pin on a shared shim in a *published*
        # crate breaks downstream resolution (#1151).
        for spelling in ('tree-sitter-language="0.1.0"', 'tree-sitter-language = "0.1.0"'):
            with self.subTest(spelling=spelling):
                self.assertEqual(GATE.unpinned_grammar_deps(spelling + "\n"), [])

    def test_caret_range_with_spaces_is_caught(self) -> None:
        self.assertEqual(
            GATE.unpinned_grammar_deps('tree-sitter-cpp = "0.23.4"\n'),
            [("tree-sitter-cpp", "0.23.4")],
        )

    def test_inline_table_without_a_pin_is_caught(self) -> None:
        self.assertEqual(
            GATE.unpinned_grammar_deps(
                'tree-sitter-tcl = { path = "../x", version = "2.1.0" }\n'
            ),
            [("tree-sitter-tcl", "2.1.0")],
        )

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


if __name__ == "__main__":
    unittest.main()
