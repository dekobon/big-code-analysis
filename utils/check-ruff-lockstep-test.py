#!/usr/bin/env python3
"""Tests for check-ruff-lockstep.py.

Three kinds of test, matching the check-excluded-manifests-test.py
pattern:

* Unit tests against synthetic file contents, weighted toward the shapes
  a line-oriented YAML read can get wrong — a reordered config, a
  neighbouring repo's ``rev:``, a quoted or comment-trailed scalar — and
  toward the malformed inputs on which a version gate must fail loudly
  rather than pass.
* ``main()`` tests over a synthetic repository root, covering each drift
  branch and the remediation text it prints.
* A smoke test running the real gate against the real repository,
  asserting a clean tree reports OK.

Run with:
    python3 -m unittest -q utils/check-ruff-lockstep-test.py
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
SCRIPT_SRC = UTILS_DIR / "check-ruff-lockstep.py"


def _load_module():  # type: ignore[no-untyped-def]
    spec = importlib.util.spec_from_file_location("check_ruff_lockstep", SCRIPT_SRC)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


GATE = _load_module()

# Deliberately non-alphabetical repo order, with a `rev:` on either side
# of the ruff block: the gate must anchor on the URL, not on position.
PRE_COMMIT_SAMPLE = """\
repos:
-   repo: https://github.com/pre-commit/pre-commit-hooks
    rev: v5.0.0
    hooks:
    -   id: check-yaml
-   repo: https://github.com/astral-sh/ruff-pre-commit
    # Bump in lockstep with uv.lock.
    rev: v0.16.2
    hooks:
    -   id: ruff-check
        args: [--fix]
-   repo: https://github.com/marco-c/taskcluster_yml_validator
    rev: v0.0.12
    hooks:
      - id: taskcluster_yml
"""

UV_LOCK_SAMPLE = """\
version = 1

[[package]]
name = "big-code-analysis"
source = { editable = "." }

[package.metadata]
requires-dist = [
    { name = "mypy", marker = "extra == 'dev'", specifier = ">=2.3" },
    { name = "ruff", marker = "extra == 'dev'", specifier = ">=0.13,<0.17" },
]

[[package]]
name = "ruff"
version = "0.16.2"
source = { registry = "https://pypi.org/simple" }
"""

REQUIREMENTS_SAMPLE = """\
mypy==2.3.0 ; python_full_version >= '3.10' \\
    --hash=sha256:aaaa
ruff==0.16.2 ; python_full_version < '3.15' and sys_platform != 'emscripten' \\
    --hash=sha256:bbbb \\
    --hash=sha256:cccc
"""

PYPROJECT_SAMPLE = """\
[project]
name = "big-code-analysis"

[project.optional-dependencies]
dev = [
  "pytest>=8.0",
  "mypy>=2.3",
  "ruff>=0.13,<0.17",
]
"""


class PreCommitRevTest(unittest.TestCase):
    """Reading `rev:` out of `.pre-commit-config.yaml` by line."""

    def test_the_ruff_blocks_rev_is_returned(self) -> None:
        self.assertEqual(GATE.read_pre_commit_rev(PRE_COMMIT_SAMPLE), "v0.16.2")

    def test_a_neighbouring_repos_rev_is_not_returned(self) -> None:
        # The whole point of anchoring on the URL. `v5.0.0` sits above
        # the ruff block and `v0.0.12` below it; a position-based read
        # would return one of them the moment the file is reordered.
        rev = GATE.read_pre_commit_rev(PRE_COMMIT_SAMPLE)
        self.assertNotIn(rev, {"v5.0.0", "v0.0.12"})

    def test_reordering_the_config_does_not_change_the_answer(self) -> None:
        blocks = PRE_COMMIT_SAMPLE.split("-   repo:")
        reordered = "-   repo:".join([blocks[0], blocks[2], blocks[1], blocks[3]])
        self.assertEqual(GATE.read_pre_commit_rev(reordered), "v0.16.2")

    def test_a_quoted_rev_is_unquoted(self) -> None:
        text = PRE_COMMIT_SAMPLE.replace("rev: v0.16.2", 'rev: "v0.16.2"')
        self.assertEqual(GATE.read_pre_commit_rev(text), "v0.16.2")

    def test_a_trailing_comment_is_stripped(self) -> None:
        text = PRE_COMMIT_SAMPLE.replace("rev: v0.16.2", "rev: v0.16.2  # pinned")
        self.assertEqual(GATE.read_pre_commit_rev(text), "v0.16.2")

    def test_the_compact_dash_spelling_is_accepted(self) -> None:
        # This config mixes `-   repo:` and `- repo:` (the local block
        # uses the latter). Both must reach the same parser.
        text = PRE_COMMIT_SAMPLE.replace(
            "-   repo: https://github.com/astral-sh",
            "- repo: https://github.com/astral-sh",
        )
        self.assertEqual(GATE.read_pre_commit_rev(text), "v0.16.2")

    def test_crlf_line_endings_are_handled(self) -> None:
        self.assertEqual(
            GATE.read_pre_commit_rev(PRE_COMMIT_SAMPLE.replace("\n", "\r\n")),
            "v0.16.2",
        )

    def test_a_missing_ruff_block_is_a_hard_error(self) -> None:
        text = PRE_COMMIT_SAMPLE.replace("astral-sh/ruff-pre-commit", "example/other")
        with self.assertRaises(SystemExit) as caught:
            GATE.read_pre_commit_rev(text)
        self.assertIn("found 0", str(caught.exception))

    def test_a_ruff_block_without_a_rev_is_a_hard_error(self) -> None:
        text = PRE_COMMIT_SAMPLE.replace("    rev: v0.16.2\n", "")
        with self.assertRaises(SystemExit) as caught:
            GATE.read_pre_commit_rev(text)
        self.assertIn("found 0", str(caught.exception))

    def test_two_ruff_blocks_are_a_hard_error(self) -> None:
        # Ambiguous rather than wrong: picking either silently would let
        # a stale duplicate govern what pre-commit actually runs.
        text = PRE_COMMIT_SAMPLE + (
            "-   repo: https://github.com/astral-sh/ruff-pre-commit\n"
            "    rev: v0.15.14\n"
        )
        with self.assertRaises(SystemExit) as caught:
            GATE.read_pre_commit_rev(text)
        self.assertIn("found 2", str(caught.exception))

    def test_an_empty_config_is_a_hard_error(self) -> None:
        with self.assertRaises(SystemExit):
            GATE.read_pre_commit_rev("")


class ScalarTest(unittest.TestCase):
    def test_a_bare_token_is_returned_unchanged(self) -> None:
        self.assertEqual(GATE._scalar("v0.16.2"), "v0.16.2")

    def test_a_comment_only_value_reduces_to_empty(self) -> None:
        # Not a crash: the caller compares the result and reports drift.
        self.assertEqual(GATE._scalar("# nothing here"), "")

    def test_single_and_double_quotes_are_both_stripped(self) -> None:
        self.assertEqual(GATE._scalar("'v1'"), "v1")
        self.assertEqual(GATE._scalar('"v1"'), "v1")


class LockVersionTest(unittest.TestCase):
    def test_the_ruff_package_version_is_returned(self) -> None:
        self.assertEqual(GATE.read_lock_version(UV_LOCK_SAMPLE), "0.16.2")

    def test_a_lock_without_ruff_is_a_hard_error(self) -> None:
        text = UV_LOCK_SAMPLE.replace(
            'name = "ruff"\nversion', 'name = "rust"\nversion'
        )
        with self.assertRaises(SystemExit) as caught:
            GATE.read_lock_version(text)
        self.assertIn("found 0", str(caught.exception))

    def test_two_ruff_packages_are_a_hard_error(self) -> None:
        text = UV_LOCK_SAMPLE + '\n[[package]]\nname = "ruff"\nversion = "0.15.22"\n'
        with self.assertRaises(SystemExit) as caught:
            GATE.read_lock_version(text)
        self.assertIn("found 2", str(caught.exception))

    def test_a_versionless_ruff_entry_is_a_hard_error_not_a_crash(self) -> None:
        # uv writes a `version` for every registry package, so this is
        # malformed input — and malformed input must reach the gate's
        # own diagnostic rather than a KeyError traceback, which reads
        # as a bug in whatever the contributor was changing.
        text = UV_LOCK_SAMPLE.replace(
            'name = "ruff"\nversion = "0.16.2"', 'name = "ruff"'
        )
        with self.assertRaises(SystemExit) as caught:
            GATE.read_lock_version(text)
        self.assertIn("found 0", str(caught.exception))

    def test_invalid_toml_is_a_hard_error(self) -> None:
        with self.assertRaises(SystemExit) as caught:
            GATE.read_lock_version("[[package\n")
        self.assertIn("not valid TOML", str(caught.exception))

    def test_a_lock_with_no_package_array_is_a_hard_error(self) -> None:
        with self.assertRaises(SystemExit) as caught:
            GATE.read_lock_version("version = 1\n")
        self.assertIn("no [[package]] entries", str(caught.exception))


class LockSpecifierTest(unittest.TestCase):
    def test_the_dev_extra_bound_is_returned(self) -> None:
        self.assertEqual(GATE.read_lock_specifier(UV_LOCK_SAMPLE), ">=0.13,<0.17")

    def test_a_double_quoted_marker_still_names_the_extra(self) -> None:
        text = UV_LOCK_SAMPLE.replace("extra == 'dev'", 'extra == \\"dev\\"')
        self.assertEqual(GATE.read_lock_specifier(text), ">=0.13,<0.17")

    def test_a_ruff_row_under_another_extra_is_ignored(self) -> None:
        text = UV_LOCK_SAMPLE.replace(
            '{ name = "ruff", marker = "extra == \'dev\'"',
            '{ name = "ruff", marker = "extra == \'examples\'"',
        )
        with self.assertRaises(SystemExit) as caught:
            GATE.read_lock_specifier(text)
        self.assertIn("found 0", str(caught.exception))

    def test_a_package_without_metadata_contributes_nothing(self) -> None:
        # The `ruff` package entry itself has no `metadata` table; the
        # walk must skip it rather than raise.
        self.assertEqual(GATE._requires_dist({"name": "ruff"}), [])

    def test_a_non_list_requires_dist_contributes_nothing(self) -> None:
        self.assertEqual(GATE._requires_dist({"metadata": {"requires-dist": "x"}}), [])

    def test_non_mapping_rows_are_dropped(self) -> None:
        self.assertEqual(
            GATE._requires_dist(
                {"metadata": {"requires-dist": ["ruff", {"name": "a"}]}}
            ),
            [{"name": "a"}],
        )

    def test_a_non_string_marker_does_not_name_an_extra(self) -> None:
        self.assertFalse(GATE._names_extra(None, "dev"))

    def test_a_different_extra_does_not_match(self) -> None:
        self.assertFalse(GATE._names_extra("extra == 'examples'", "dev"))


class RequirementsVersionTest(unittest.TestCase):
    def test_the_pinned_version_is_returned(self) -> None:
        self.assertEqual(GATE.read_requirements_version(REQUIREMENTS_SAMPLE), "0.16.2")

    def test_a_file_without_ruff_is_a_hard_error(self) -> None:
        with self.assertRaises(SystemExit) as caught:
            GATE.read_requirements_version("mypy==2.3.0\n")
        self.assertIn("found 0", str(caught.exception))

    def test_two_ruff_pins_are_a_hard_error(self) -> None:
        with self.assertRaises(SystemExit) as caught:
            GATE.read_requirements_version("ruff==0.16.2\nruff==0.15.22\n")
        self.assertIn("found 2", str(caught.exception))

    def test_a_similarly_named_distribution_is_not_matched(self) -> None:
        # `ruff-lsp` shares the prefix. Requiring `==` immediately after
        # the name is what keeps it out.
        self.assertEqual(
            GATE.read_requirements_version("ruff-lsp==0.0.62\nruff==0.16.2\n"),
            "0.16.2",
        )

    def test_an_indented_mention_is_not_a_pin(self) -> None:
        # A `uv export` is mostly continuation and `# via` lines, all
        # indented. Without the start-of-line anchor a comment naming an
        # old version counts as a second pin and the gate hard-errors on
        # a perfectly good export — or, with one real pin removed,
        # reports the comment's version as the installed one.
        self.assertEqual(
            GATE.read_requirements_version(
                "ruff==0.16.2 ; python_full_version < '3.15' \\\n"
                "    --hash=sha256:bbbb\n"
                "    # was ruff==0.15.22 before #1222\n"
            ),
            "0.16.2",
        )


class PyprojectSpecifierTest(unittest.TestCase):
    def test_the_dev_extra_bound_is_returned(self) -> None:
        self.assertEqual(
            GATE.read_pyproject_specifier(PYPROJECT_SAMPLE), ">=0.13,<0.17"
        )

    def test_the_name_is_normalised_before_matching(self) -> None:
        # PEP 503 treats `Ruff` and `ruff` as one project.
        text = PYPROJECT_SAMPLE.replace('"ruff>=0.13', '"Ruff>=0.13')
        self.assertEqual(GATE.read_pyproject_specifier(text), ">=0.13,<0.17")

    def test_a_distribution_merely_containing_ruff_is_not_matched(self) -> None:
        # `ruff-lsp` normalises to a name *containing* `ruff`. Matching
        # on containment rather than equality would collect two
        # specifiers here and hard-error on a legal manifest.
        text = PYPROJECT_SAMPLE.replace(
            '  "ruff>=0.13,<0.17",', '  "ruff-lsp>=0.0.62",\n  "ruff>=0.13,<0.17",'
        )
        self.assertEqual(GATE.read_pyproject_specifier(text), ">=0.13,<0.17")

    def test_an_unbounded_requirement_yields_an_empty_specifier(self) -> None:
        text = PYPROJECT_SAMPLE.replace('"ruff>=0.13,<0.17"', '"ruff"')
        self.assertEqual(GATE.read_pyproject_specifier(text), "")

    def test_non_string_entries_are_skipped(self) -> None:
        text = PYPROJECT_SAMPLE.replace('  "pytest>=8.0",', "  1,")
        self.assertEqual(GATE.read_pyproject_specifier(text), ">=0.13,<0.17")

    def test_a_missing_project_table_is_a_hard_error(self) -> None:
        with self.assertRaises(SystemExit) as caught:
            GATE.read_pyproject_specifier("[tool.ruff]\nline-length = 100\n")
        self.assertIn("optional-dependencies", str(caught.exception))

    def test_a_missing_dev_extra_is_a_hard_error(self) -> None:
        with self.assertRaises(SystemExit) as caught:
            GATE.read_pyproject_specifier(
                '[project]\nname = "x"\n\n'
                '[project.optional-dependencies]\nexamples = ["jupyter"]\n'
            )
        self.assertIn("optional-dependencies", str(caught.exception))

    def test_a_dev_extra_without_ruff_is_a_hard_error(self) -> None:
        text = PYPROJECT_SAMPLE.replace('  "ruff>=0.13,<0.17",\n', "")
        with self.assertRaises(SystemExit) as caught:
            GATE.read_pyproject_specifier(text)
        self.assertIn("found 0", str(caught.exception))

    def test_two_ruff_requirements_are_a_hard_error(self) -> None:
        text = PYPROJECT_SAMPLE.replace(
            '  "ruff>=0.13,<0.17",', '  "ruff>=0.13,<0.17",\n  "ruff<0.20",'
        )
        with self.assertRaises(SystemExit) as caught:
            GATE.read_pyproject_specifier(text)
        self.assertIn("found 2", str(caught.exception))

    def test_invalid_toml_is_a_hard_error(self) -> None:
        with self.assertRaises(SystemExit) as caught:
            GATE.read_pyproject_specifier("[project\n")
        self.assertIn("not valid TOML", str(caught.exception))


class ClauseSetTest(unittest.TestCase):
    def test_whitespace_is_insignificant(self) -> None:
        self.assertEqual(
            GATE.clause_set(">= 0.13, <0.17"), GATE.clause_set(">=0.13,<0.17")
        )

    def test_clause_order_is_insignificant(self) -> None:
        self.assertEqual(
            GATE.clause_set("<0.17,>=0.13"), GATE.clause_set(">=0.13,<0.17")
        )

    def test_a_different_bound_is_a_different_set(self) -> None:
        self.assertNotEqual(
            GATE.clause_set(">=0.13,<0.17"), GATE.clause_set(">=0.13,<0.16")
        )

    def test_an_empty_specifier_yields_an_empty_set(self) -> None:
        # `"".split(",")` is `[""]`, so without the filter this would be
        # a one-element set and an unbounded requirement would compare
        # equal to nothing at all.
        self.assertEqual(GATE.clause_set(""), frozenset())


class MainTest(unittest.TestCase):
    """`main()` over a synthetic repository root."""

    def _repo(
        self,
        pre_commit: str = PRE_COMMIT_SAMPLE,
        uv_lock: str = UV_LOCK_SAMPLE,
        requirements: str = REQUIREMENTS_SAMPLE,
        pyproject: str = PYPROJECT_SAMPLE,
    ) -> None:
        root = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        (root / "big-code-analysis-py" / "requirements").mkdir(parents=True)
        (root / GATE.PRE_COMMIT_CONFIG).write_text(pre_commit, encoding="utf-8")
        (root / GATE.UV_LOCK).write_text(uv_lock, encoding="utf-8")
        (root / GATE.REQUIREMENTS_DEV).write_text(requirements, encoding="utf-8")
        (root / GATE.PYPROJECT).write_text(pyproject, encoding="utf-8")
        self.enterContext(mock.patch.object(GATE, "REPO_ROOT", root))

    def _run(self) -> tuple[int, str, str]:
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = GATE.main()
        return code, out.getvalue(), err.getvalue()

    def test_an_aligned_tree_passes(self) -> None:
        self._repo()
        code, out, _ = self._run()
        self.assertEqual(code, 0)
        self.assertIn("ruff lockstep OK", out)
        # The version, not just the word OK: a pass that names the wrong
        # version is the drift this gate exists to report.
        self.assertIn("ruff 0.16.2", out)

    def test_a_drifted_pre_commit_rev_fails(self) -> None:
        # The exact shape #1230 documents: rev v0.15.14 against a
        # lockfile resolving 0.16.2.
        self._repo(pre_commit=PRE_COMMIT_SAMPLE.replace("v0.16.2", "v0.15.14"))
        code, _, err = self._run()
        self.assertEqual(code, 1)
        self.assertIn(".pre-commit-config.yaml", err)
        self.assertIn("v0.15.14", err)
        self.assertIn("expected v0.16.2", err)

    def test_a_stale_requirements_export_fails(self) -> None:
        self._repo(
            requirements=REQUIREMENTS_SAMPLE.replace("ruff==0.16.2", "ruff==0.15.22")
        )
        code, _, err = self._run()
        self.assertEqual(code, 1)
        self.assertIn("requirements/dev.txt", err)
        self.assertIn("make py-relock", err)

    def test_a_bound_edited_without_a_relock_fails(self) -> None:
        self._repo(pyproject=PYPROJECT_SAMPLE.replace("<0.17", "<0.18"))
        code, _, err = self._run()
        self.assertEqual(code, 1)
        self.assertIn("pyproject.toml", err)
        self.assertIn(">=0.13,<0.18", err)

    def test_a_cosmetically_respelled_bound_still_passes(self) -> None:
        # uv normalises the specifier it writes back, so a human's
        # spacing must not read as drift.
        self._repo(
            pyproject=PYPROJECT_SAMPLE.replace(">=0.13,<0.17", ">= 0.13, < 0.17")
        )
        code, _, _ = self._run()
        self.assertEqual(code, 0)

    def test_every_drifted_site_is_reported_in_one_run(self) -> None:
        # Reporting only the first would make fixing this a three-round
        # trip through a gate that takes minutes in `make pre-commit`.
        self._repo(
            pre_commit=PRE_COMMIT_SAMPLE.replace("v0.16.2", "v0.15.14"),
            requirements=REQUIREMENTS_SAMPLE.replace("ruff==0.16.2", "ruff==0.15.22"),
            pyproject=PYPROJECT_SAMPLE.replace("<0.17", "<0.18"),
        )
        code, _, err = self._run()
        self.assertEqual(code, 1)
        self.assertIn(".pre-commit-config.yaml", err)
        self.assertIn("requirements/dev.txt", err)
        self.assertIn("pyproject.toml", err)

    def test_an_absent_input_file_is_a_hard_error(self) -> None:
        self._repo()
        (GATE.REPO_ROOT / GATE.PRE_COMMIT_CONFIG).unlink()
        with self.assertRaises(SystemExit) as caught:
            self._run()
        self.assertIn("cannot read .pre-commit-config.yaml", str(caught.exception))


class RealRepositoryTest(unittest.TestCase):
    def test_clean_tree_passes(self) -> None:
        result = subprocess.run(
            [sys.executable, str(SCRIPT_SRC)],
            capture_output=True,
            text=True,
            cwd=REPO_ROOT,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("ruff lockstep OK", result.stdout)
        # The bound is echoed so a pass distinguishes "all four agree"
        # from "the gate found nothing to compare".
        self.assertIn("3 sites checked", result.stdout)

    def test_it_runs_from_an_unrelated_cwd(self) -> None:
        # `REPO_ROOT` is derived from the script's own location, so the
        # gate must not depend on being invoked from the repository root
        # (AGENTS.md, "Project layout").
        with tempfile.TemporaryDirectory() as elsewhere:
            result = subprocess.run(
                [sys.executable, str(SCRIPT_SRC)],
                capture_output=True,
                text=True,
                cwd=elsewhere,
            )
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
