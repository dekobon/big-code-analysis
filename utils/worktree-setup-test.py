#!/usr/bin/env python3
"""Tests for worktree-setup.py.

The classifier decides whether a submodule gets a *destructive*
``--force``, so it is tested against real git repositories rather than a
mock: the whole question is what git's own plumbing reports for each
damaged shape, and a mock would only re-state this file's assumptions.
Each fixture builds a two-repository superproject in a tempdir, damages
the submodule in one specific way, and asserts the resulting state.

The ``INCOMPLETE`` fixture is the one that matters most — it reproduces
the interrupted checkout from #1171 and also pins the claim that
motivated the whole script: a plain ``git submodule update --init`` over
that state exits 0 and restores nothing.

Run with:
    python3 -m unittest -q utils/worktree-setup-test.py
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

# `update()` and `main()` shell out to plain `git` with no `-c` flags of
# their own, so the fixture's local (`file://`) submodule origin has to be
# re-enabled through the environment instead — `protocol.file` is denied
# by default since CVE-2022-39253. Applied per test, never to any config.
FILE_PROTOCOL_ENV = {
    "GIT_CONFIG_COUNT": "1",
    "GIT_CONFIG_KEY_0": "protocol.file.allow",
    "GIT_CONFIG_VALUE_0": "always",
}

UTILS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = UTILS_DIR.parent
SCRIPT_SRC = UTILS_DIR / "worktree-setup.py"


def _load_module():  # type: ignore[no-untyped-def]
    spec = importlib.util.spec_from_file_location("worktree_setup", SCRIPT_SRC)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


SETUP = _load_module()

# Identity and protocol settings the fixtures need. `protocol.file` is
# denied by default since CVE-2022-39253 and a local submodule clone is
# exactly the use it was denied for, so it is re-enabled per invocation
# rather than written into any config.
GIT_ENV_ARGS = [
    "-c",
    "user.email=test@example.invalid",
    "-c",
    "user.name=worktree-setup test",
    "-c",
    "protocol.file.allow=always",
]


def run_git(cwd: pathlib.Path, *args: str) -> str:
    return subprocess.run(
        ["git", *GIT_ENV_ARGS, *args],
        cwd=cwd,
        check=True,
        capture_output=True,
        text=True,
    ).stdout


class SuperprojectFixture:
    """A superproject with one submodule at `vendor/sub`, in a tempdir."""

    SUBMODULE_PATH = "vendor/sub"

    def __init__(self, tmp: pathlib.Path) -> None:
        self.sub_origin = tmp / "origin"
        self.root = tmp / "super"
        for path in (self.sub_origin, self.root):
            path.mkdir(parents=True)
            run_git(path, "init", "-q", "-b", "main")

        # Two files, one nested, so a partial deletion is expressible.
        (self.sub_origin / "a.txt").write_text("a\n")
        (self.sub_origin / "snapshots").mkdir()
        (self.sub_origin / "snapshots" / "s.snap").write_text("s\n")
        run_git(self.sub_origin, "add", "-A")
        run_git(self.sub_origin, "commit", "-qm", "sub")

        (self.root / "root.txt").write_text("root\n")
        run_git(self.root, "add", "-A")
        run_git(self.root, "commit", "-qm", "root")
        run_git(
            self.root,
            "submodule",
            "add",
            "-q",
            str(self.sub_origin),
            self.SUBMODULE_PATH,
        )
        run_git(self.root, "commit", "-qm", "add submodule")

    @property
    def work(self) -> pathlib.Path:
        return self.root / self.SUBMODULE_PATH

    def classify(self) -> object:
        return SETUP.classify(self.root, self.SUBMODULE_PATH)


class ClassifyTest(unittest.TestCase):
    """One damaged shape per test, each with a distinct expected state."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.fixture = SuperprojectFixture(pathlib.Path(self._tmp.name))

    def test_freshly_added_submodule_is_ready(self) -> None:
        self.assertIs(self.fixture.classify(), SETUP.State.READY)

    def test_deinitialized_submodule_is_missing(self) -> None:
        run_git(self.fixture.root, "submodule", "deinit", "-f", "--", self.fixture.SUBMODULE_PATH)
        self.assertIs(self.fixture.classify(), SETUP.State.MISSING)

    def test_emptied_worktree_is_incomplete(self) -> None:
        run_git(self.fixture.work, "rm", "-rq", ".")
        self.assertIs(self.fixture.classify(), SETUP.State.INCOMPLETE)

    def test_partially_emptied_worktree_is_incomplete(self) -> None:
        # The #1171 tell: "a corpus directory containing only snapshots/".
        run_git(self.fixture.work, "rm", "-q", "a.txt")
        self.assertIs(self.fixture.classify(), SETUP.State.INCOMPLETE)
        self.assertTrue((self.fixture.work / "snapshots" / "s.snap").exists())

    def test_deletion_plus_modification_is_blocked(self) -> None:
        run_git(self.fixture.work, "rm", "-q", "a.txt")
        (self.fixture.work / "snapshots" / "s.snap").write_text("locally accepted\n")
        self.assertIs(self.fixture.classify(), SETUP.State.BLOCKED)

    def test_modification_alone_is_ready(self) -> None:
        # Accepting a snapshot in the integration-snapshot submodule is
        # routine (AGENTS.md); it must not read as damage, or the setup
        # target would offer to overwrite work in progress.
        (self.fixture.work / "snapshots" / "s.snap").write_text("locally accepted\n")
        self.assertIs(self.fixture.classify(), SETUP.State.READY)

    def test_untracked_file_alone_is_ready(self) -> None:
        # `.snap.new` files land here constantly during a metric change.
        (self.fixture.work / "snapshots" / "s.snap.new").write_text("pending\n")
        self.assertIs(self.fixture.classify(), SETUP.State.READY)

    def test_wrong_revision_is_stale(self) -> None:
        (self.fixture.work / "a.txt").write_text("moved on\n")
        run_git(self.fixture.work, "add", "-A")
        run_git(self.fixture.work, "commit", "-qm", "advance submodule")
        self.assertIs(self.fixture.classify(), SETUP.State.STALE)


class RepairTest(unittest.TestCase):
    """The behaviour the script exists for: plain re-run vs `--force`.

    `update()` is called for real rather than having its argv spelled out
    here. A test that assembles its own `git submodule update --force`
    proves git can repair the tree; it never proves this script asks git
    to — the production bookkeeping and a test's replica of it are two
    different things (`.claude/rules/testing.md`).
    """

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.fixture = SuperprojectFixture(pathlib.Path(self._tmp.name))
        run_git(self.fixture.work, "rm", "-rq", ".")
        patcher = unittest.mock.patch.dict(os.environ, FILE_PROTOCOL_ENV)
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_plain_update_is_a_silent_no_op(self) -> None:
        # Deliberately git's own argv, not `update()`: the claim under
        # test is about git's behaviour, and it is the premise the whole
        # script rests on (#1171).
        run_git(
            self.fixture.root,
            "submodule",
            "update",
            "--init",
            "--",
            self.fixture.SUBMODULE_PATH,
        )
        self.assertFalse((self.fixture.work / "a.txt").exists())
        self.assertIs(self.fixture.classify(), SETUP.State.INCOMPLETE)

    def test_forced_update_restores_the_worktree(self) -> None:
        SETUP.update(self.fixture.root, self.fixture.SUBMODULE_PATH, force=True)
        self.assertTrue((self.fixture.work / "a.txt").exists())
        self.assertIs(self.fixture.classify(), SETUP.State.READY)

    def test_unforced_update_restores_nothing(self) -> None:
        # The other side of `if force:`. Asserting only the forced case
        # leaves the branch half-observable: an inverted condition still
        # passes some `--force` somewhere.
        SETUP.update(self.fixture.root, self.fixture.SUBMODULE_PATH, force=False)
        self.assertFalse((self.fixture.work / "a.txt").exists())
        self.assertIs(self.fixture.classify(), SETUP.State.INCOMPLETE)


class MainTest(unittest.TestCase):
    """`main()` end to end, against a fixture standing in for the repo.

    Patching the two module-level path constants is what makes the exit
    code reachable: `make worktree-setup` consumes it, and neither the
    BLOCKED refusal nor the repair path is observable from `classify()`
    alone.
    """

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        # Resolved up front: `verify_repo_root` compares the patched root
        # against git's `--show-toplevel`, and a tempdir reached through
        # a symlink would fail that comparison for the wrong reason.
        self.fixture = SuperprojectFixture(pathlib.Path(self._tmp.name).resolve())
        for patcher in (
            unittest.mock.patch.object(SETUP, "REPO_ROOT", self.fixture.root),
            unittest.mock.patch.object(
                SETUP, "GITMODULES", self.fixture.root / ".gitmodules"
            ),
            unittest.mock.patch.dict(os.environ, FILE_PROTOCOL_ENV),
        ):
            patcher.start()
            self.addCleanup(patcher.stop)

    def run_main(self) -> tuple[int, str]:
        """`main()`'s exit code and what it wrote to stderr."""
        err = io.StringIO()
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(err):
            code = SETUP.main()
        return code, err.getvalue()

    def test_main_repairs_an_incomplete_checkout_and_exits_zero(self) -> None:
        run_git(self.fixture.work, "rm", "-rq", ".")
        code, err = self.run_main()
        self.assertEqual(code, 0, f"stderr was: {err}")
        self.assertTrue((self.fixture.work / "a.txt").exists())
        self.assertIs(self.fixture.classify(), SETUP.State.READY)

    def test_main_refuses_a_blocked_submodule_and_exits_one(self) -> None:
        run_git(self.fixture.work, "rm", "-q", "a.txt")
        modified = self.fixture.work / "snapshots" / "s.snap"
        modified.write_text("locally accepted\n")
        code, err = self.run_main()
        self.assertEqual(code, 1, "a refusal must not exit green")
        self.assertIn(self.fixture.SUBMODULE_PATH, err)
        self.assertIn(SETUP.FORCE_RATIONALE, err)
        self.assertEqual(
            modified.read_text(),
            "locally accepted\n",
            "the refusal exists to keep this file; it must survive",
        )
        self.assertFalse((self.fixture.work / "a.txt").exists())


class RepoGuardTest(unittest.TestCase):
    """The root-resolution guard around the destructive `--force`."""

    def test_submodule_paths_reject_an_escaping_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            run_git(root, "init", "-q", "-b", "main")
            (root / ".gitmodules").write_text(
                '[submodule "evil"]\n\tpath = ../outside\n\turl = https://example.invalid\n'
            )
            with self.assertRaises(SystemExit):
                SETUP.submodule_paths(root)

    def test_real_repository_root_is_accepted(self) -> None:
        SETUP.verify_repo_root()
        self.assertTrue((REPO_ROOT / ".gitmodules").is_file())

    def test_a_non_checkout_gets_the_tailored_refusal(self) -> None:
        # `git()` runs with `check=False`, so the old
        # `except CalledProcessError` arm was unreachable and this case
        # exited through git()'s generic "git rev-parse ... failed"
        # preamble instead of naming the actual problem.
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp).resolve()
            (root / ".gitmodules").write_text("")
            with (
                unittest.mock.patch.object(SETUP, "REPO_ROOT", root),
                unittest.mock.patch.object(SETUP, "GITMODULES", root / ".gitmodules"),
                self.assertRaises(SystemExit) as caught,
            ):
                SETUP.verify_repo_root()
        self.assertIn("is not a git checkout", str(caught.exception))

    def test_a_missing_git_binary_is_diagnosed_as_such(self) -> None:
        # The only input that ever reached the old handler, where
        # "is not a git checkout" was the wrong diagnosis.
        with (
            unittest.mock.patch.object(
                SETUP.subprocess, "run", side_effect=FileNotFoundError("git")
            ),
            self.assertRaises(SystemExit) as caught,
        ):
            SETUP.git(["rev-parse", "--show-toplevel"], cwd=REPO_ROOT)
        self.assertIn("git is not on PATH", str(caught.exception))


if __name__ == "__main__":
    unittest.main()
