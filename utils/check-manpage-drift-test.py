#!/usr/bin/env python3
"""Tests for check-manpage-drift.py.

The gate this exercises had gone green for two years over the one state
it most needed to catch — a man page ``cargo xtask`` had just written
and nobody had committed (#1249). A gate that silently stops gating
looks exactly like a clean tree, which is why every direction it claims
to cover is pinned here against a scratch repository.

The table below is the measured behaviour of the three candidate git
mechanisms; the tests are that table, and swapping the implementation
to either rejected alternative turns one of them red:

===========================  =========  ==================  ===========
state                        git diff   ls-files --others   porcelain
===========================  =========  ==================  ===========
clean                        pass       pass                pass
new page, untracked          **pass**   fail                fail
new page, staged             pass       pass                **fail**
tracked page modified        fail       pass                fail
tracked page deleted         fail       pass                fail
page removed from index      **pass**   fail                fail
===========================  =========  ==================  ===========

There is deliberately **no** smoke test against the real repository.
``man/*.1`` is rewritten in place by the ``_pc-manpages`` stage, which
``make pre-commit`` runs concurrently with this test's own stage, so an
assertion about the real tree's cleanliness would be a race rather than
an invariant.

Run with:
    python3 -m unittest -q utils/check-manpage-drift-test.py
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import os
import pathlib
import subprocess
import tempfile
import unittest
import unittest.mock

# The gate under test is a sibling in `utils/`.
UTILS_DIR = pathlib.Path(__file__).resolve().parent
SCRIPT_SRC = UTILS_DIR / "check-manpage-drift.py"


def _load_module():  # type: ignore[no-untyped-def]
    spec = importlib.util.spec_from_file_location(
        "check_manpage_drift", SCRIPT_SRC
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


DRIFT = _load_module()

# Identity for the fixture commits, passed per invocation rather than
# written into any config.
GIT_ENV_ARGS = [
    "-c",
    "user.email=test@example.invalid",
    "-c",
    "user.name=manpage-drift test",
]

# Host config must not reach the fixtures. `GIT_CONFIG_*` alone is not
# enough: git falls back to `$XDG_CONFIG_HOME/git/ignore` (defaulting to
# `$HOME/.config/git/ignore`) for excludes, which no config variable
# suppresses, so a contributor whose global ignore covers `*.1` would
# otherwise silently change what the untracked assertions measure.
# `_isolated_env` re-points both at the per-test tempdir.
BASE_GIT_ENV = {
    "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_CONFIG_SYSTEM": os.devnull,
}


def run_git(cwd: pathlib.Path, *args: str) -> str:
    return subprocess.run(
        ["git", *GIT_ENV_ARGS, *args],
        cwd=cwd,
        check=True,
        capture_output=True,
        text=True,
    ).stdout


class ManDirFixture(unittest.TestCase):
    """A scratch repo with a committed `man/` tree.

    Pages are named after real ones so a failure message reads like the
    situation it models. `man/*.log` is gitignored, standing in for the
    editor droppings the gate must tolerate.
    """

    PAGES = ("bca.1", "bca-check.1", "bca-web.1")

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = pathlib.Path(self._tmp.name) / "repo"
        (self.root / "man").mkdir(parents=True)

        home = pathlib.Path(self._tmp.name) / "home"
        home.mkdir()
        env = unittest.mock.patch.dict(
            os.environ,
            {**BASE_GIT_ENV, "HOME": str(home), "XDG_CONFIG_HOME": str(home)},
        )
        env.start()
        self.addCleanup(env.stop)

        run_git(self.root, "init", "-q", "-b", "main")
        (self.root / ".gitignore").write_text("man/*.log\n", encoding="utf-8")
        for page in self.PAGES:
            self.page(page).write_text(f".TH {page}\n", encoding="utf-8")
        run_git(self.root, "add", "-A")
        run_git(self.root, "commit", "-qm", "man pages")

    def page(self, name: str) -> pathlib.Path:
        return self.root / "man" / name

    def gate(self) -> tuple[int, str, str]:
        """Run the real `main()` against the fixture; (code, out, err)."""
        out, err = io.StringIO(), io.StringIO()
        with unittest.mock.patch.object(DRIFT, "REPO_ROOT", self.root):
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(
                err
            ):
                code = DRIFT.main()
        return code, out.getvalue(), err.getvalue()

    def assertPasses(self, why: str) -> None:
        code, out, err = self.gate()
        self.assertEqual(code, 0, f"{why}: expected pass, got {code}\n{err}")
        self.assertIn("manpage-drift: OK", out)

    def assertFails(self, why: str, *named: str) -> None:
        code, _out, err = self.gate()
        self.assertEqual(code, 1, f"{why}: expected failure, got {code}")
        for name in named:
            self.assertIn(name, err, f"{why}: report did not name {name}")


class CleanTreeTest(ManDirFixture):
    def test_committed_tree_passes(self) -> None:
        self.assertPasses("a freshly committed man/ tree")

    def test_non_page_dropping_is_not_drift(self) -> None:
        # The added-page half is scoped to `man/*.1`, so an editor
        # dropping under man/ is not drift — which is what buys the
        # right to ignore the ignore rules (see the next test).
        (self.root / "man" / "scratch.log").write_text("x\n", encoding="utf-8")
        self.assertPasses("a non-page file under man/")

    def test_changes_outside_man_are_not_drift(self) -> None:
        # Pins the pathspec scope. Widening either half to the whole
        # repository turns `make manpages-check` red on any dirty
        # tree — and it runs inside `make pre-commit`, where an
        # untracked file elsewhere is the normal state.
        (self.root / "untracked.txt").write_text("x\n", encoding="utf-8")
        (self.root / "src").mkdir()
        (self.root / "src" / "main.rs").write_text("fn main() {}\n")
        run_git(self.root, "add", "src/main.rs")
        run_git(self.root, "commit", "-qm", "src")
        (self.root / "src" / "main.rs").write_text("fn main() {;}\n")
        self.assertPasses("changes outside man/")


class AddedPageTest(ManDirFixture):
    """#1249: the direction `git diff` cannot see."""

    def test_untracked_new_page_fails(self) -> None:
        # Exactly what `cargo xtask` leaves behind for a subcommand
        # added to the clap tree but never committed.
        self.page("bca-newsub.1").write_text(".TH new\n", encoding="utf-8")
        self.assertFails("an untracked new page", "man/bca-newsub.1")

    def test_git_diff_alone_would_have_passed(self) -> None:
        # Pins the bug rather than only the fix: if a future edit drops
        # the `ls-files` half, this states plainly what is lost.
        self.page("bca-newsub.1").write_text(".TH new\n", encoding="utf-8")
        self.assertEqual(DRIFT.tracked_drift(self.root), "")
        self.assertEqual(
            DRIFT.untracked_pages(self.root), ["man/bca-newsub.1"]
        )

    def test_gitignored_new_page_still_fails(self) -> None:
        # An ignore rule must not be able to hide a generated page.
        # `--exclude-standard` would consult the host's global ignore
        # too, where a `*.1` line silently reinstates #1249 and no
        # in-repo config can override it. Written to `info/exclude`
        # because that is the one exclude source a test can set
        # without depending on the host at all.
        (self.root / ".git" / "info" / "exclude").write_text(
            "man/bca-newsub.1\n", encoding="utf-8"
        )
        self.page("bca-newsub.1").write_text(".TH new\n", encoding="utf-8")
        self.assertEqual(
            run_git(
                self.root, "status", "--porcelain", "--", "man/"
            ).strip(),
            "",
            "fixture no longer models an ignored page",
        )
        self.assertFails("a gitignored new page", "man/bca-newsub.1")

    def test_staged_new_page_passes(self) -> None:
        # The mechanism decision. `git status --porcelain` reports
        # `A  man/bca-newsub.1` here and would fail — but staging is
        # the remedy the gate's own message prescribes, and the
        # `pre-commit` framework runs this gate against the staged
        # tree, so rejecting it would break every man-page commit.
        self.page("bca-newsub.1").write_text(".TH new\n", encoding="utf-8")
        run_git(self.root, "add", "man/bca-newsub.1")
        self.assertIn(
            "A  man/bca-newsub.1",
            run_git(self.root, "status", "--porcelain", "--", "man/"),
            "fixture no longer models the staged-new-page state",
        )
        self.assertPasses("a staged but uncommitted new page")


class ModifiedPageTest(ManDirFixture):
    def test_modified_tracked_page_fails(self) -> None:
        self.page("bca-check.1").write_text(".TH changed\n", encoding="utf-8")
        self.assertFails("a modified tracked page", "drift from the clap")

    def test_modified_page_diff_is_printed(self) -> None:
        # The old gate printed `git diff`'s output; keep that, so a
        # reviewer sees which lines moved without re-running anything.
        self.page("bca-check.1").write_text(".TH changed\n", encoding="utf-8")
        _code, out, _err = self.gate()
        self.assertIn("man/bca-check.1", out)
        self.assertIn("+.TH changed", out)

    @unittest.skipUnless(
        os.path.exists("/bin/true"), "needs a silent POSIX diff driver"
    )
    def test_external_diff_driver_cannot_report_clean(self) -> None:
        # A configured `diff.external` / GIT_EXTERNAL_DIFF may print
        # nothing at all, so a gate that reads emptiness as cleanliness
        # goes green over a modified page. Two flags answer that and
        # each is asserted here: `--exit-code` decides the verdict, and
        # `--no-ext-diff` keeps the printed evidence git's own — drop
        # the latter and the failure names no lines.
        self.page("bca-check.1").write_text(".TH changed\n", encoding="utf-8")
        with unittest.mock.patch.dict(
            os.environ, {"GIT_EXTERNAL_DIFF": "/bin/true"}
        ):
            code, out, err = self.gate()
        self.assertEqual(code, 1, "silent diff driver read as a clean tree")
        self.assertIn("drift", err)
        self.assertIn("+.TH changed", out)

    def test_staged_modification_passes(self) -> None:
        self.page("bca-check.1").write_text(".TH changed\n", encoding="utf-8")
        run_git(self.root, "add", "man/bca-check.1")
        self.assertPasses("a staged modification")


class RemovedPageTest(ManDirFixture):
    def test_page_deleted_from_worktree_fails(self) -> None:
        # What `xtask`'s `sweep_orphans` does to a retired subcommand.
        self.page("bca-check.1").unlink()
        self.assertFails("a page deleted from the worktree", "drift")

    def test_page_removed_from_index_only_fails(self) -> None:
        # `git rm --cached` leaves the file on disk and untracked, so
        # `git diff` is clean and only the `ls-files` half reports it.
        run_git(self.root, "rm", "-q", "--cached", "man/bca-check.1")
        self.assertEqual(DRIFT.tracked_drift(self.root), "")
        self.assertFails("a page removed from the index", "man/bca-check.1")


class CaseOnlyRenameTest(ManDirFixture):
    """A case-only command rename, on a case-sensitive filesystem.

    `xtask` writes `bca-Check.1` and `sweep_orphans` — which compares
    names case-sensitively — deletes the now-unexpected `bca-check.1`.
    Both directions fire at once, and the old gate saw only the
    deletion, so its report never named the page that had to be added.

    On a case-*insensitive* filesystem the same rename makes
    `sweep_orphans` delete the page it just wrote; that is #1250, a
    bug in the sweep rather than in this gate.
    """

    def test_report_names_both_the_removal_and_the_addition(self) -> None:
        self.page("bca-check.1").unlink()
        self.page("bca-Check.1").write_text(".TH Check\n", encoding="utf-8")
        self.assertFails(
            "a case-only rename",
            "man/bca-Check.1",  # the added half — invisible before #1249
            "drift from the clap",  # the removed half
        )


class GitFailureTest(unittest.TestCase):
    """A git error must not read as a clean tree."""

    @staticmethod
    def _run(root: pathlib.Path) -> tuple[int, str]:
        err = io.StringIO()
        with unittest.mock.patch.object(DRIFT, "REPO_ROOT", root):
            with contextlib.redirect_stderr(err):
                code = DRIFT.main()
        return code, err.getvalue()

    def test_non_repository_exits_two(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp) / "not-a-repo"
            root.mkdir()
            code, err = self._run(root)
            self.assertEqual(code, 2)
            self.assertIn("error:", err)

    def test_untracked_half_failing_alone_exits_two(self) -> None:
        # `main()` calls `tracked_drift` first, so the test above never
        # reaches `untracked_pages`. Swallowing its GitError — the
        # "a git failure reads as clean" mode `_git` exists to
        # prevent — would otherwise go unnoticed.
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp) / "not-a-repo"
            root.mkdir()
            with unittest.mock.patch.object(
                DRIFT, "tracked_drift", lambda _root: ""
            ):
                code, err = self._run(root)
            self.assertEqual(code, 2)
            self.assertIn("ls-files", err)


class AnnotationTest(ManDirFixture):
    """CI keeps the `::error::` annotation the inline step used to emit."""

    def _run_with_actions(self, value: str | None) -> str:
        self.page("bca-newsub.1").write_text(".TH new\n", encoding="utf-8")
        env = {} if value is None else {"GITHUB_ACTIONS": value}
        with unittest.mock.patch.dict(os.environ, env, clear=False):
            if value is None:
                os.environ.pop("GITHUB_ACTIONS", None)
            _code, out, _err = self.gate()
        return out

    def test_annotation_emitted_on_a_runner(self) -> None:
        self.assertIn("::error::", self._run_with_actions("true"))

    def test_no_annotation_locally(self) -> None:
        self.assertNotIn("::error::", self._run_with_actions(None))


if __name__ == "__main__":
    unittest.main()
