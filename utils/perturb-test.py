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

ORIGINAL = "fn f() -> u8 {\n    1\n}\n"


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
        ):
            subprocess.run(argv, cwd=root, check=True, env=env)
        (root / "src").mkdir()
        self.target = root / "src" / "a.rs"
        self.target.write_text(ORIGINAL)
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
        self.repo.target.write_text(ORIGINAL + "// uncommitted\n")
        results, rc, _, err = self._sweep(self.repo.manifest("", 0, [_case()]))
        self.assertEqual(rc, 2)
        self.assertEqual(results, [])
        self.assertIn("dirty or untracked", err)
        self.assertIn("src/a.rs", err)
        self.assertEqual(self.repo.target.read_text(), ORIGINAL + "// uncommitted\n")
        self.assertEqual(list(self.backup.iterdir()), [], "nothing was written")

    def test_refuses_an_untracked_target(self) -> None:
        new = self.repo.root / "src" / "b.rs"
        new.write_text(ORIGINAL)
        case = {**_case(), "file": "src/b.rs"}
        results, rc, _, err = self._sweep(self.repo.manifest("", 0, [case]))
        self.assertEqual(rc, 2)
        self.assertEqual(results, [])
        self.assertIn("?? src/b.rs", err)

    def test_refuses_when_git_status_fails(self) -> None:
        case = {**_case(), "file": "../outside.rs"}
        results, rc, _, err = self._sweep(self.repo.manifest("", 0, [case]))
        self.assertEqual((results, rc), ([], 2))
        self.assertIn("git status failed", err)

    def test_rejects_an_anchor_equal_to_its_replacement(self) -> None:
        results, rc, out, _ = self._sweep(self.repo.manifest("", 0, [_case(new="1\n")]))
        self.assertEqual(rc, 1)
        self.assertEqual(results[0]["verdict"], "BAD-ANCHOR")
        self.assertIn("equals `new`", out)
        self.assertEqual(self.repo.target.read_text(), ORIGINAL)

    def test_rejects_an_anchor_that_does_not_occur_exactly_once(self) -> None:
        for old, n in (("missing", 0), ("\n", 3)):
            results, rc, out, _ = self._sweep(self.repo.manifest("", 0, [_case(old=old, new="x")]))
            self.assertEqual(rc, 1, old)
            self.assertEqual(results[0]["verdict"], "BAD-ANCHOR")
            self.assertIn(f"occurs {n} times", out)

    def test_caught_when_a_test_fails_and_the_status_agrees(self) -> None:
        stdout = "test a::b ... FAILED\\n---- a::b stdout ----\\nerror: test failed\\n"
        results, rc, _, _ = self._sweep(self.repo.manifest(stdout, 101, [_case(expect=["a::b"])]))
        self.assertEqual(rc, 0)
        self.assertEqual(results[0]["verdict"], "caught")
        self.assertEqual(results[0]["failed"], ["a::b"])
        self.assertEqual(results[0]["missing_expected"], [])

    def test_caught_by_the_wrong_test_is_reported_as_missing(self) -> None:
        stdout = "test a::other ... FAILED\\n"
        results, rc, out, _ = self._sweep(self.repo.manifest(stdout, 101, [_case(expect=["a::b"])]))
        self.assertEqual(rc, 1)
        self.assertEqual(results[0]["missing_expected"], ["a::b"])
        self.assertIn("MISSING=['a::b']", out)

    def test_uncaught_when_nothing_fails(self) -> None:
        results, rc, _, _ = self._sweep(self.repo.manifest("ok\\n", 0, [_case()]))
        self.assertEqual(rc, 1)
        self.assertEqual(results[0]["verdict"], "UNCAUGHT")

    def test_build_error_is_classified_by_error_bracket_not_bare_error(self) -> None:
        self.assertEqual(perturb.classify(101, "error[E0425]: cannot find\n")[0], "BUILD-ERROR")
        self.assertEqual(perturb.classify(101, "error: could not compile `x`\n")[0], "BUILD-ERROR")
        # An ordinary red run says `error: test failed` and is a catch.
        verdict, failed = perturb.classify(101, "test a::b ... FAILED\nerror: test failed\n")
        self.assertEqual((verdict, failed), ("caught", {"a::b"}))

    def test_status_and_parsed_failures_disagreeing_is_a_harness_error(self) -> None:
        # Nonzero exit, no failure line: the parser missed (`cargo test
        # -q` dots, #1238) or the harness died.
        self.assertEqual(perturb.classify(101, "...F..\n")[0], "HARNESS-ERROR")
        # A failure line with a zero exit: the status was masked.
        self.assertEqual(perturb.classify(0, "test a::b ... FAILED\n")[0], "HARNESS-ERROR")
        results, rc, _, _ = self._sweep(self.repo.manifest("....F\\n", 101, [_case()]))
        self.assertEqual((rc, results[0]["verdict"]), (1, "HARNESS-ERROR"))

    def test_restores_from_memory_and_keeps_an_on_disk_backup(self) -> None:
        seen = self.repo.root / "seen.txt"
        # The stub records the mutated file as it ran.
        self.repo.stub.write_text(
            f"import pathlib, sys\npathlib.Path({str(seen)!r}).write_text(pathlib.Path({str(self.repo.target)!r}).read_text())\nsys.stdout.write('test a ... FAILED\\n')\nsys.exit(101)\n"
        )
        results, rc, _, _ = self._sweep(self.repo.manifest("", 0, [_case()]))
        self.assertEqual(rc, 0)
        self.assertEqual(seen.read_text(), ORIGINAL.replace("1\n", "2\n"))
        self.assertEqual(self.repo.target.read_text(), ORIGINAL)
        self.assertEqual((self.backup / "src__a.rs").read_text(), ORIGINAL)

    def test_restores_when_the_test_run_raises(self) -> None:
        manifest = self.repo.manifest("", 0, [_case()])
        manifest["cargo"] = [str(self.repo.root / "no-such-binary")]
        with self.assertRaises(OSError):
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                perturb.sweep(manifest, self.backup)
        self.assertEqual(self.repo.target.read_text(), ORIGINAL)

    def test_main_uses_a_mktemp_backup_and_writes_the_record(self) -> None:
        manifest = self.repo.manifest("test a::b ... FAILED\\n", 101, [_case(expect=["a::b"])])
        manifest["record"] = "out/results.json"
        path = self.repo.root / "m.json"
        path.write_text(json.dumps(manifest))
        (self.repo.root / "out").mkdir()
        err = io.StringIO()
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(err):
            rc = perturb.main(["perturb.py", str(path)])
        self.assertEqual(rc, 0)
        backup_line = next(line for line in err.getvalue().splitlines() if line.startswith("on-disk backups: "))
        backup = pathlib.Path(backup_line.split(": ", 1)[1])
        self.assertTrue(backup.name.startswith("bca-perturb-backup."), backup)
        self.assertEqual((backup / "src__a.rs").read_text(), ORIGINAL)
        record = json.loads((self.repo.root / "out" / "results.json").read_text())
        self.assertEqual(record[0]["verdict"], "caught")
        # The scratch directory is the caller's to keep or drop; an
        # interrupted run needs it, so main never removes it.
        for p in sorted(backup.rglob("*"), reverse=True):
            p.unlink()
        backup.rmdir()


if __name__ == "__main__":
    unittest.main()
