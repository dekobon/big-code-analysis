#!/usr/bin/env python3
"""Self-tests for `utils/coverage-diff.py`: Codecov's glob semantics,
the zero-context diff parser, llvm-cov's line table from segments, the
tallies and verdict, and an end-to-end run on a scratch repository
with synthetic exports.

Run: ``python3 -B -m unittest -q utils/coverage-diff-test.py``.
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

UTILS_DIR = pathlib.Path(__file__).resolve().parent


def _load(name: str, filename: str):
    spec = importlib.util.spec_from_file_location(name, UTILS_DIR / filename)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


cd = _load("coverage_diff", "coverage-diff.py")


def _seg(line: int, col: int, count: int, has: bool = True, entry: bool = True, gap: bool = False) -> list:
    return [line, col, count, has, entry, gap]


DIFF = """diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,0 +2,3 @@
+x
+y
+z
@@ -10 +13 @@
+w
diff --git a/src/gone.rs b/src/gone.rs
--- a/src/gone.rs
+++ /dev/null
@@ -1,5 +0,0 @@
diff --git a/src/old.rs b/src/new.rs
similarity index 90%
rename from src/old.rs
rename to src/new.rs
--- a/src/old.rs
+++ b/src/new.rs
@@ -4 +4 @@
+q
"""

# Covered, missed, partial, gap, wrapped and non-executable lines.
SEGMENTS = [
    _seg(1, 1, 5),  # line 1 covered, wraps to line 2
    _seg(3, 1, 0),  # line 3: a zero region starts while 5 wraps in → partial
    _seg(3, 10, 5),  # back to 5, wraps into line 4
    _seg(5, 1, 0, gap=True),  # gap entry: not a region start; line 5 mapped by the wrap
    _seg(6, 1, 0),  # line 6 missed
    _seg(7, 1, 0, has=False),  # skipped-region start: line 7 unmapped
    _seg(9, 1, 2),  # line 9 covered; line 8 unmapped (wrapped has no count)
]


class UnitTests(unittest.TestCase):
    def test_globs_follow_codecov_semantics(self) -> None:
        pats = [cd.glob_to_regex(g) for g in ["**/tests/**", "xtask/**", "enums/**", "**/build.rs"]]
        for rel in ("tests/parity/x.rs", "big-code-analysis-ast/tests/a.rs", "xtask/src/main.rs", "build.rs", "a/b/build.rs"):
            self.assertTrue(cd.ignored(rel, pats), rel)
        for rel in ("src/metrics/abc.rs", "src/test_support.rs"):
            self.assertFalse(cd.ignored(rel, pats), rel)

    def test_diff_parser_handles_added_modified_deleted_and_renamed(self) -> None:
        self.assertEqual(cd.parse_diff(DIFF), {"src/a.rs": {2, 3, 4, 13}, "src/new.rs": {4}})

    def test_line_table_follows_llvm_cov(self) -> None:
        cov = cd.line_counts(SEGMENTS)
        self.assertEqual(cov.counts, {1: 5, 2: 5, 3: 5, 4: 5, 5: 5, 6: 0, 9: 2})
        self.assertEqual(cov.partial_lines, {3})
        self.assertFalse({7, 8} & set(cov.counts))

    def test_tallies_verdict_and_markdown(self) -> None:
        cov = cd.line_counts(SEGMENTS)
        head = {
            "src/a.rs": cov,
            "tests/t.rs": cd.FileCov(counts={1: 0, 2: 0}),
            "src/doc.rs": cd.FileCov(counts={}),
        }
        base = {"src/a.rs": cd.FileCov(counts={1: 1, 2: 1, 3: 1, 4: 0})}
        changed = {
            "src/a.rs": {3, 5, 6},
            "src/doc.rs": {1},
            "src/missing.rs": {1},
            "tests/t.rs": {1},
            "README.md": {1},
        }
        r = cd.compute(base, head, changed, ["**/tests/**"], "B", "H")
        self.assertEqual(r["project"]["base"]["executable"], 4)
        self.assertAlmostEqual(r["project"]["base"]["pct"], 75.0)
        self.assertEqual(r["project"]["head"]["executable"], 7)
        self.assertAlmostEqual(r["project"]["head"]["pct"], 85.714, places=2)
        pt = r["patch"]["total"]
        # Patch over {3, 5, 6}: 3 partial, 5 covered (wrapped), 6 missed.
        self.assertEqual((pt["executable"], pt["covered"], pt["partial"], pt["missed"]), (3, 2, 1, 1))
        self.assertEqual(r["patch"]["unmapped"], ["src/missing.rs"])
        self.assertEqual(r["patch"]["non_executable"], ["src/doc.rs"])
        self.assertEqual(sorted(r["patch"]["skipped"]), ["README.md", "tests/t.rs"])
        self.assertFalse(r["verdict"])
        self.assertFalse(r["verdict_conservative"])
        md = cd.render_markdown(r)
        self.assertIn("NOT MET", md)
        self.assertIn("`src/missing.rs`", md)
        self.assertIn("Missed changed lines", md)
        # Raise the patch to all covered → the verdict flips.
        head["src/a.rs"] = cd.FileCov(counts={1: 5, 2: 5, 3: 5, 4: 5, 5: 5, 6: 5})
        r2 = cd.compute(base, head, {"src/a.rs": {3, 5, 6}}, [], "B", "H")
        self.assertTrue(r2["verdict"])
        self.assertAlmostEqual(r2["patch"]["total"]["pct"], 100.0)

    def test_covered_count_is_compared_not_percentage(self) -> None:
        # The head grew by two uncovered lines and one covered one: the
        # percentage falls while the covered count rises, and the report
        # carries both so the count can be read.
        base = {"src/a.rs": cd.FileCov(counts={1: 1, 2: 1})}
        head = {"src/a.rs": cd.FileCov(counts={1: 1, 2: 1, 3: 1, 4: 0, 5: 0})}
        r = cd.compute(base, head, {"src/a.rs": {3, 4, 5}}, [], "B", "H")
        self.assertEqual(r["project"]["base"]["covered"], 2)
        self.assertEqual(r["project"]["head"]["covered"], 3)
        self.assertGreater(r["project"]["base"]["pct"], r["project"]["head"]["pct"])
        self.assertEqual(r["patch"]["files"][0]["missed_lines"], [4, 5])
        self.assertEqual(r["files"], [{
            "file": "src/a.rs", "only_in": None,
            "base_executable": 2, "base_covered": 2,
            "head_executable": 5, "head_covered": 3,
            "delta_covered": 1, "delta_executable": 3,
        }])

    def test_per_file_counts_are_compared_so_a_swap_between_files_is_visible(self) -> None:
        # Two files trade two covered lines: the project totals and the
        # patch are identical either way, and only a per-file comparison
        # can tell the swapped base from the unswapped one (C1 on #1744).
        base = {
            "src/a.rs": cd.FileCov(counts={1: 1, 2: 1, 3: 1}),
            "src/b.rs": cd.FileCov(counts={1: 1, 2: 0, 3: 0}),
            "tests/t.rs": cd.FileCov(counts={1: 1}),
        }
        swapped = {"src/a.rs": base["src/b.rs"], "src/b.rs": base["src/a.rs"], "tests/t.rs": base["tests/t.rs"]}
        head = {
            "src/a.rs": cd.FileCov(counts={1: 1, 2: 1, 3: 1}),
            "src/b.rs": cd.FileCov(counts={1: 1, 2: 0, 3: 0}),
            "src/new.rs": cd.FileCov(counts={1: 1, 2: 0}),
        }
        changed = {"src/a.rs": {1}, "src/new.rs": {1, 2}}
        r = cd.compute(base, head, changed, ["**/tests/**"], "B", "H")
        r_swapped = cd.compute(swapped, head, changed, ["**/tests/**"], "B", "H")
        self.assertEqual(r["project"], r_swapped["project"])
        self.assertEqual(r["patch"], r_swapped["patch"])
        self.assertNotEqual(r["files"], r_swapped["files"])
        by_file = {row["file"]: row for row in r["files"]}
        self.assertEqual(sorted(by_file), ["src/a.rs", "src/b.rs", "src/new.rs"], "ignored files are out")
        self.assertEqual((by_file["src/a.rs"]["base_covered"], by_file["src/a.rs"]["head_covered"], by_file["src/a.rs"]["delta_covered"]), (3, 3, 0))
        self.assertEqual(by_file["src/new.rs"]["only_in"], "head")
        self.assertEqual((by_file["src/new.rs"]["base_covered"], by_file["src/new.rs"]["head_covered"]), (0, 1))
        self.assertEqual(r["covered_count_regressions"], [])
        swapped_by_file = {row["file"]: row for row in r_swapped["files"]}
        self.assertEqual(swapped_by_file["src/a.rs"]["delta_covered"], 2)
        self.assertEqual(swapped_by_file["src/b.rs"]["delta_covered"], -2)
        self.assertEqual(r_swapped["covered_count_regressions"], ["src/b.rs"])
        md = cd.render_markdown(r_swapped)
        self.assertIn("| `src/b.rs` | 3 | 3 | 3 | 1 | -2 |", md)
        self.assertIn("| `src/new.rs` (head only) | 0 | 0 | 2 | 1 | +1 |", md)
        self.assertIn("Files whose covered count fell: 1", md)
        # A deleted file is a base-only row with the head side at zero.
        gone = cd.compute(base, {"src/a.rs": head["src/a.rs"]}, {}, [], "B", "H")
        gone_by_file = {row["file"]: row for row in gone["files"]}
        self.assertEqual(gone_by_file["src/b.rs"]["only_in"], "base")
        self.assertEqual(gone_by_file["src/b.rs"]["head_covered"], 0)

    def test_codecov_ignores_reads_the_block_without_yaml(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            p = pathlib.Path(d) / "codecov.yml"
            p.write_text('coverage:\n  status: x\nignore:\n  - "**/tests/**"  # comment\n  - xtask/**\nother:\n  - not-a-glob\n')
            self.assertEqual(cd.codecov_ignores(p), ["**/tests/**", "xtask/**"])


class EndToEndTests(unittest.TestCase):
    """`main` over a scratch repository: the diff comes from git, the
    exports from synthetic llvm-cov JSON, and both files land."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self._tmp.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        env = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t", "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t"}
        git = lambda *a: subprocess.run(["git", *a], cwd=self.repo, check=True, env=env, capture_output=True, text=True).stdout.strip()
        git("init", "-q", "-b", "main")
        git("config", "commit.gpgsign", "false")
        (self.repo / "src").mkdir()
        (self.repo / "src" / "a.rs").write_text("fn a() {}\n")
        (self.repo / "codecov.yml").write_text('ignore:\n  - "**/tests/**"\n')
        git("add", "-A")
        git("commit", "-q", "-m", "base")
        self.base_rev = git("rev-parse", "HEAD")
        (self.repo / "src" / "a.rs").write_text("fn a() {}\nfn b() {\n    1\n}\n")
        git("add", "-A")
        git("commit", "-q", "-m", "head")
        self.head_rev = git("rev-parse", "HEAD")

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def _export(self, name: str, segments: list[list], root: pathlib.Path | None = None) -> pathlib.Path:
        p = self.root / name
        p.write_text(json.dumps({"data": [{"files": [{"filename": str((root or self.repo) / "src" / "a.rs"), "segments": segments, "summary": {"regions": {"count": 2, "covered": 1}}}]}]}))
        return p

    def _argv(self, base: pathlib.Path, head: pathlib.Path, out: pathlib.Path, *extra: str) -> list[str]:
        return ["--base-json", str(base), "--head-json", str(head), "--base-rev", self.base_rev, "--head-rev", self.head_rev, "--repo", str(self.repo), "--codecov", str(self.repo / "codecov.yml"), "--out", str(out), *extra]

    def test_main_reports_the_patch_against_the_project(self) -> None:
        base = self._export("base.json", [_seg(1, 1, 1), _seg(1, 10, 0, has=False)])
        # Line 1 covered and closed; a zero region spans lines 2–3 and a
        # skipped region starts on line 4, so of the changed lines 2–4
        # the first two are missed and the closing brace is unmapped.
        head = self._export("head.json", [_seg(1, 1, 1), _seg(1, 10, 0, has=False), _seg(2, 1, 0), _seg(4, 2, 0, has=False)])
        out = self.root / "out"
        with contextlib.redirect_stdout(io.StringIO()):
            rc = cd.main(self._argv(base, head, out))
        self.assertEqual(rc, 1, "a fully-missed patch does not meet the floor")
        result = json.loads((out / "coverage-diff.json").read_text())
        self.assertEqual(result["patch"]["files"][0]["file"], "src/a.rs")
        self.assertEqual(result["patch"]["files"][0]["missed_lines"], [2, 3])
        self.assertEqual(result["patch"]["files"][0]["regions_covered"], 1)
        self.assertEqual([r["file"] for r in result["files"]], ["src/a.rs"])
        self.assertIn("NOT MET", (out / "coverage-diff.md").read_text())

    def test_the_diff_is_parsed_whatever_the_users_git_config(self) -> None:
        # The driver pins its git config as patch-coverage.py does (C3 on
        # #1744). `core.quotePath` is the one a commit-vs-commit diff
        # shows: at its default a non-ASCII path renders as
        # `+++ "b/\303\274.rs"`, which would be keyed as a quoted octal
        # string. `diff.noprefix` is tolerated by the parser either way
        # and `diff.mnemonicPrefix` does not apply to a commit diff, so
        # both are pinned for the record and invisible here.
        (self.repo / "src" / "\u00fc.rs").write_text("fn u() {}\n")
        env = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t", "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t"}
        subprocess.run(["git", "add", "-A"], cwd=self.repo, check=True)
        subprocess.run(["git", "commit", "-q", "-m", "unicode"], cwd=self.repo, check=True, env=env)
        head = subprocess.run(["git", "rev-parse", "HEAD"], cwd=self.repo, check=True, capture_output=True, text=True).stdout.strip()
        for key, value in (("core.quotePath", "true"), ("diff.noprefix", "true")):
            subprocess.run(["git", "config", key, value], cwd=self.repo, check=True)
            changed = cd.changed_lines(self.repo, self.base_rev, head)
            self.assertEqual(changed, {"src/a.rs": {2, 3, 4}, "src/\u00fc.rs": {1}}, key)
            subprocess.run(["git", "config", "--unset", key], cwd=self.repo, check=True)

    def test_a_base_export_from_another_worktree_takes_base_repo(self) -> None:
        other = self.root / "worktree-base"
        base = self._export("base.json", [_seg(1, 1, 1)], root=other)
        head = self._export("head.json", [_seg(1, 1, 1), _seg(2, 1, 0), _seg(4, 2, 0, has=False)])
        out = self.root / "out"
        # Without --base-repo no base file is under --repo: refused, not
        # silently read as a different denominator (C2 on #1744).
        with contextlib.redirect_stdout(io.StringIO()), self.assertRaises(SystemExit) as cm:
            cd.main(self._argv(base, head, out))
        self.assertIn("no file is under", str(cm.exception))
        self.assertIn("--base-repo", str(cm.exception))
        with contextlib.redirect_stdout(io.StringIO()):
            rc = cd.main(self._argv(base, head, out, "--base-repo", str(other)))
        self.assertEqual(rc, 1)
        result = json.loads((out / "coverage-diff.json").read_text())
        self.assertEqual(result["project"]["base"]["executable"], 1)
        self.assertEqual([(r["file"], r["only_in"]) for r in result["files"]], [("src/a.rs", None)])


if __name__ == "__main__":
    unittest.main()
