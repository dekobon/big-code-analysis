#!/usr/bin/env python3
"""Self-tests for `utils/perturb.py`: each hardening item of #1744 on a
scratch git repository and a stub `cargo`, so none can regress without
a pull request going red.

Run: ``python3 -B -m unittest -q utils/perturb-test.py``.
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


perturb = _load("perturb", "perturb.py")

# A CRLF line among LF ones: a text-mode read would normalise it, so the
# restore and the on-disk backup are only byte-exact if the driver reads
# and writes bytes (P1 on #1744).
ORIGINAL = b"fn f() -> u8 {\r\n    1\n}\n"
RESULT = "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\\n"
RED = "test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\\n"


class ScratchRepo:
    """A committed git repository holding `src/a.rs` and a stub `cargo`
    whose output and exit status the manifest's argv selects."""

    def __init__(self, root: pathlib.Path) -> None:
        self.root = root
        root.mkdir()
        env = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t", "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t"}
        for argv in (
            ["git", "init", "-q", "-b", "main"],
            ["git", "config", "commit.gpgsign", "false"],
            # The fixture carries a CRLF line on purpose; a host-wide
            # autocrlf would rewrite it on add and warn on every commit.
            ["git", "config", "core.autocrlf", "false"],
        ):
            subprocess.run(argv, cwd=root, check=True, env=env)
        (root / "src").mkdir()
        self.target = root / "src" / "a.rs"
        self.target.write_bytes(ORIGINAL)
        self.stub = root / "stub-cargo.py"
        self.stub.write_text(
            "import sys\n"
            "stdout, code = sys.argv[1], int(sys.argv[2])\n"
            "sys.stdout.write(stdout.encode().decode('unicode_escape'))\n"
            "sys.exit(code)\n"
        )
        for argv in (["git", "add", "-A"], ["git", "commit", "-q", "-m", "init"]):
            subprocess.run(argv, cwd=root, check=True, env=env)

    def manifest(self, stdout: str, code: int, cases: list[dict]) -> dict:
        return {
            "cwd": str(self.root),
            "cargo": [sys.executable, "-B", str(self.stub), stdout, str(code)],
            "cases": cases,
        }


def _case(old: str = "1\n", new: str = "2\n", **extra: object) -> dict:
    return {"name": "c", "file": "src/a.rs", "old": old, "new": new, **extra}


class PerturbTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.repo = ScratchRepo(pathlib.Path(self._tmp.name) / "repo")
        self.backup = pathlib.Path(self._tmp.name) / "backup"
        self.backup.mkdir()

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def _sweep(self, manifest: dict) -> tuple[list[dict], int, str, str]:
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            results, rc = perturb.sweep(manifest, self.backup)
        return results, rc, out.getvalue(), err.getvalue()

    def test_refuses_a_dirty_target_before_any_write(self) -> None:
        self.repo.target.write_bytes(ORIGINAL + b"// uncommitted\n")
        results, rc, _, err = self._sweep(self.repo.manifest("", 0, [_case()]))
        self.assertEqual(rc, 2)
        self.assertEqual(results, [])
        self.assertIn("not tracked and clean", err)
        self.assertIn(" M src/a.rs", err)
        self.assertEqual(self.repo.target.read_bytes(), ORIGINAL + b"// uncommitted\n")
        self.assertEqual(list(self.backup.iterdir()), [], "nothing was written")

    def test_refuses_an_untracked_target(self) -> None:
        new = self.repo.root / "src" / "b.rs"
        new.write_bytes(ORIGINAL)
        case = {**_case(), "file": "src/b.rs"}
        results, rc, _, err = self._sweep(self.repo.manifest("", 0, [case]))
        self.assertEqual(rc, 2)
        self.assertEqual(results, [])
        self.assertIn("pathspec 'src/b.rs' did not match", err)

    def test_refuses_an_ignored_target(self) -> None:
        # `git status --porcelain` prints nothing for an ignored path, so
        # the tracked check is what refuses it.
        (self.repo.root / ".gitignore").write_text("src/ignored.rs\n")
        ignored = self.repo.root / "src" / "ignored.rs"
        ignored.write_bytes(ORIGINAL)
        case = {**_case(), "file": "src/ignored.rs"}
        results, rc, _, err = self._sweep(self.repo.manifest("", 0, [case]))
        self.assertEqual((results, rc), ([], 2))
        self.assertIn("pathspec 'src/ignored.rs' did not match", err)
        self.assertEqual(ignored.read_bytes(), ORIGINAL)

    def test_refuses_a_nonexistent_target_before_any_case_runs(self) -> None:
        # The missing file is the second case; without the tracked check
        # the first would spend a full test run before the traceback.
        cases = [_case(), {**_case(), "name": "d", "file": "src/none.rs"}]
        results, rc, _, err = self._sweep(self.repo.manifest(RED, 101, cases))
        self.assertEqual((results, rc), ([], 2))
        self.assertIn("pathspec 'src/none.rs' did not match", err)

    def test_refuses_when_git_cannot_answer(self) -> None:
        case = {**_case(), "file": "../outside.rs"}
        results, rc, _, err = self._sweep(self.repo.manifest("", 0, [case]))
        self.assertEqual((results, rc), ([], 2))
        self.assertIn("outside repository", err)

    def test_rejects_an_anchor_equal_to_its_replacement(self) -> None:
        results, rc, out, _ = self._sweep(self.repo.manifest("", 0, [_case(new="1\n")]))
        self.assertEqual(rc, 1)
        self.assertEqual(results[0]["verdict"], "BAD-ANCHOR")
        self.assertIn("equals `new`", out)
        self.assertEqual(self.repo.target.read_bytes(), ORIGINAL)

    def test_rejects_an_anchor_that_does_not_occur_exactly_once(self) -> None:
        for old, n in (("missing", 0), ("\n", 3)):
            results, rc, out, _ = self._sweep(self.repo.manifest("", 0, [_case(old=old, new="x")]))
            self.assertEqual(rc, 1, old)
            self.assertEqual(results[0]["verdict"], "BAD-ANCHOR")
            self.assertIn(f"occurs {n} times", out)

    def test_caught_when_a_test_fails_and_the_status_agrees(self) -> None:
        stdout = "test a::b ... FAILED\\n---- a::b stdout ----\\n" + RED + "error: test failed\\n"
        results, rc, _, _ = self._sweep(self.repo.manifest(stdout, 101, [_case(expect=["a::b"])]))
        self.assertEqual(rc, 0)
        self.assertEqual(results[0]["verdict"], "caught")
        self.assertEqual(results[0]["failed"], ["a::b"])
        self.assertEqual(results[0]["missing_expected"], [])

    def test_caught_by_the_wrong_test_is_reported_as_missing(self) -> None:
        stdout = "test a::other ... FAILED\\n" + RED
        results, rc, out, _ = self._sweep(self.repo.manifest(stdout, 101, [_case(expect=["a::b"])]))
        self.assertEqual(rc, 1)
        self.assertEqual(results[0]["missing_expected"], ["a::b"])
        self.assertIn("MISSING=['a::b']", out)

    def test_uncaught_when_nothing_fails(self) -> None:
        results, rc, _, _ = self._sweep(self.repo.manifest(RESULT, 0, [_case()]))
        self.assertEqual(rc, 1)
        self.assertEqual(results[0]["verdict"], "UNCAUGHT")

    def test_zero_tests_ran_is_a_harness_error_not_uncaught(self) -> None:
        # A filter matching nothing: `running 0 tests … ok`, exit 0. Read
        # as UNCAUGHT it is the uniform-zero tell for every case.
        none = "running 0 tests\\n\\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 7 filtered out\\n"
        results, rc, out, _ = self._sweep(self.repo.manifest(none, 0, [_case()]))
        self.assertEqual((rc, results[0]["verdict"]), (1, "HARNESS-ERROR"))
        self.assertIn("0 tests ran", out)
        # Several targets: the sum is what counts, so one empty target
        # beside a populated one is still a run.
        two = none + "running 2 tests\\n" + RESULT
        results, _, _, _ = self._sweep(self.repo.manifest(two, 0, [_case()]))
        self.assertEqual(results[0]["verdict"], "UNCAUGHT")
        # No `test result:` line at all (a nextest argv, or a build that
        # never reached a target) is a harness error either way.
        self.assertEqual(perturb.classify(0, "ok\n")[0], "HARNESS-ERROR")
        self.assertEqual(perturb.classify(100, "test a::b ... FAILED\n")[0], "HARNESS-ERROR")

    def test_build_error_is_classified_by_error_bracket_not_bare_error(self) -> None:
        self.assertEqual(perturb.classify(101, "error[E0425]: cannot find\n")[0], "BUILD-ERROR")
        self.assertEqual(perturb.classify(101, "error: could not compile `x`\n")[0], "BUILD-ERROR")
        # An ordinary red run says `error: test failed` and is a catch.
        verdict, failed, _ = perturb.classify(101, "test a::b ... FAILED\n" + RED.replace("\\n", "\n") + "error: test failed\n")
        self.assertEqual((verdict, failed), ("caught", {"a::b"}))

    def test_status_and_parsed_failures_disagreeing_is_a_harness_error(self) -> None:
        # Nonzero exit, no failure line: the parser missed (`cargo test
        # -q` dots, #1238) or the harness died.
        self.assertEqual(perturb.classify(101, "...F..\n")[0], "HARNESS-ERROR")
        # A failure line with a zero exit: the status was masked.
        self.assertEqual(perturb.classify(0, "test a::b ... FAILED\n")[0], "HARNESS-ERROR")
        results, rc, _, _ = self._sweep(self.repo.manifest("....F\\n" + RED, 101, [_case()]))
        self.assertEqual((rc, results[0]["verdict"]), (1, "HARNESS-ERROR"))

    def test_restores_from_memory_and_keeps_an_on_disk_backup(self) -> None:
        seen = self.repo.root / "seen.txt"
        # The stub records the mutated file as it ran.
        self.repo.stub.write_text(
            f"import pathlib, sys\npathlib.Path({str(seen)!r}).write_bytes(pathlib.Path({str(self.repo.target)!r}).read_bytes())\nsys.stdout.write('test a ... FAILED\\n{RED}')\nsys.exit(101)\n"
        )
        results, rc, _, _ = self._sweep(self.repo.manifest("", 0, [_case()]))
        self.assertEqual(rc, 0)
        self.assertEqual(seen.read_bytes(), ORIGINAL.replace(b"1\n", b"2\n"))
        # Byte-exact: the CRLF line survives the restore and the backup.
        self.assertEqual(self.repo.target.read_bytes(), ORIGINAL)
        self.assertEqual((self.backup / "src__a.rs").read_bytes(), ORIGINAL)

    def test_restores_when_the_test_run_raises(self) -> None:
        manifest = self.repo.manifest("", 0, [_case()])
        manifest["cargo"] = [str(self.repo.root / "no-such-binary")]
        with self.assertRaises(OSError):
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                perturb.sweep(manifest, self.backup)
        self.assertEqual(self.repo.target.read_bytes(), ORIGINAL)

    def _main(self, manifest: dict) -> tuple[int, pathlib.Path, pathlib.Path]:
        """Runs `main`; returns the status, the backup directory it
        printed and the record path it printed."""
        path = self.repo.root / "m.json"
        path.write_text(json.dumps(manifest))
        err = io.StringIO()
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(err):
            rc = perturb.main([str(path)])
        lines = dict(line.split(": ", 1) for line in err.getvalue().splitlines() if ": " in line)
        return rc, pathlib.Path(lines["on-disk backups"]), pathlib.Path(lines["results"])

    def _drop(self, backup: pathlib.Path) -> None:
        # The scratch directory is the caller's to keep or drop; an
        # interrupted run needs it, so main never removes it.
        for p in sorted(backup.rglob("*"), reverse=True):
            p.unlink()
        backup.rmdir()

    def test_main_uses_a_mktemp_backup_and_writes_the_record(self) -> None:
        manifest = self.repo.manifest("test a::b ... FAILED\\n" + RED, 101, [_case(expect=["a::b"])])
        manifest["record"] = "out/results.json"
        (self.repo.root / "out").mkdir()
        rc, backup, record = self._main(manifest)
        self.assertEqual(rc, 0)
        self.assertTrue(backup.name.startswith("bca-perturb-backup."), backup)
        self.assertEqual((backup / "src__a.rs").read_bytes(), ORIGINAL)
        self.assertEqual(record, self.repo.root / "out" / "results.json")
        self.assertEqual(json.loads(record.read_text())[0]["verdict"], "caught")
        self._drop(backup)

    def test_record_defaults_into_the_backup_directory_not_the_repo(self) -> None:
        manifest = self.repo.manifest("test a::b ... FAILED\\n" + RED, 101, [_case(expect=["a::b"])])
        rc, backup, record = self._main(manifest)
        self.assertEqual(rc, 0)
        self.assertEqual(record, backup / "perturb-results.json")
        self.assertEqual(json.loads(record.read_text())[0]["verdict"], "caught")
        self.assertFalse((self.repo.root / "perturb-results.json").exists(), "no litter in the repo")
        self._drop(backup)

    def test_help_and_a_missing_manifest_are_usage_not_a_traceback(self) -> None:
        for argv in (["--help"], []):
            err, out = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err), self.assertRaises(SystemExit) as cm:
                perturb.main(argv)
            self.assertEqual(cm.exception.code, 0 if argv else 2, argv)
            self.assertIn("usage: ", out.getvalue() + err.getvalue())


if __name__ == "__main__":
    unittest.main()
