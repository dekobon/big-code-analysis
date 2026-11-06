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
import shutil
import tempfile
import time
import unittest
from unittest import mock

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


def failing(*names: str, escaped: bool = True) -> str:
    """One libtest target's report with `names` failing, in cargo's real
    shape: progress lines, each failure's captured output, the
    `failures:` list the parser reads, then `test result:`. Escaped (`\\n`
    as two characters) for the stub's argv unless `escaped` is false."""
    out = (
        "running 2 tests\n"
        + "".join(f"test {n} ... FAILED\n" for n in names)
        + "test ok ... ok\n\nfailures:\n\n"
        + "".join(f"---- {n} stdout ----\nboom\n\n" for n in names)
        + "\nfailures:\n"
        + "".join(f"    {n}\n" for n in names)
        + f"\ntest result: FAILED. 1 passed; {len(names)} failed; 0 ignored; 0 measured; 0 filtered out\n"
    )
    return out.replace("\n", "\\n") if escaped else out
# `cargo test --doc` on a crate with four doctests (rustc 1.99.1): one
# passes, one panics, one fails to compile, and one is marked
# `compile_fail` and compiles. Verbatim but for the panicking thread's id.
DOCTEST_RUN = """\
   Doc-tests dt

running 4 tests
test src/lib.rs - one (line 12) ... FAILED
test src/lib.rs - one (line 17) - compile fail ... FAILED
test src/lib.rs - one (line 2) ... ok
test src/lib.rs - one (line 7) ... FAILED

failures:

---- src/lib.rs - one (line 12) stdout ----
error[E0425]: cannot find function `nope` in crate `dt`
  --> src/lib.rs:14:17
   |
14 | let x: u8 = dt::nope();
   |                 ^^^^ not found in `dt`

error: aborting due to 1 previous error

For more information about this error, try `rustc --explain E0425`.
Couldn't compile the test.
---- src/lib.rs - one (line 17) stdout ----
Test compiled successfully, but it's marked `compile_fail`.
---- src/lib.rs - one (line 7) stdout ----
Test executable failed (exit status: 101).

stderr:

thread 'main' panicked at src/lib.rs:5:1:
assertion `left == right` failed
  left: 1
 right: 2


failures:
    src/lib.rs - one (line 12)
    src/lib.rs - one (line 17)
    src/lib.rs - one (line 7)

test result: FAILED. 1 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s

error: doctest failed, to rerun pass `--doc`
"""

# `cargo test --no-fail-fast --lib` (rustc 1.99.1) on a unit target with a
# `#[should_panic]` test that does not panic and a test that prints
# cargo-shaped lines before failing. Verbatim but for thread ids;
# `{progress}` holds the progress lines, which `-q` drops.
NOISY_PROGRESS = """\
test tests::ok ... ok
test tests::noisy ... FAILED
test tests::plain ... FAILED
test tests::sp - should panic ... FAILED
"""
NOISY_RUN = """\
running 4 tests
{progress}
failures:

---- tests::noisy stdout ----
test fake::thing ... FAILED
---- fake::other stdout ----
error[E0001]: fake
failures:
    fake::listed
---- section 1

thread 'tests::noisy' panicked at src/lib.rs:25:9:
boom
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

---- tests::plain stdout ----

thread 'tests::plain' panicked at src/lib.rs:17:26:
assertion `left == right` failed
  left: 1
 right: 2

---- tests::sp stdout ----
note: test did not panic as expected at src/lib.rs:16:32

failures:
    tests::noisy
    tests::plain
    tests::sp

test result: FAILED. 1 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: test failed, to rerun pass `--lib`
"""


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
        # The variables `unittest_env` sets, and the colour switches it
        # overrides, cleared from this process: a sweep of this file by
        # the unittest runner runs it with them set, and an inner sweep
        # that inherited them would hide a mutant that drops one.
        cleared = ("PYTHONPYCACHEPREFIX", "PYTHONDONTWRITEBYTECODE", "PYTHON_COLORS", "NO_COLOR", "FORCE_COLOR")
        env = mock.patch.dict(os.environ, {})
        env.start()
        self.addCleanup(env.stop)
        for name in cleared:
            os.environ.pop(name, None)
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
        stdout = failing("a::b") + "error: test failed\\n"
        results, rc, _, _ = self._sweep(self.repo.manifest(stdout, 101, [_case(expect=["a::b"])]))
        self.assertEqual(rc, 0)
        self.assertEqual(results[0]["verdict"], "caught")
        self.assertEqual(results[0]["failed"], ["a::b"])
        self.assertEqual(results[0]["missing_expected"], [])

    def test_caught_by_the_wrong_test_is_reported_as_missing(self) -> None:
        stdout = failing("a::other")
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
        verdict, failed, _ = perturb.classify(101, failing("a::b", escaped=False) + "error: test failed\n")
        self.assertEqual((verdict, failed), ("caught", {"a::b"}))

    def test_doctest_names_with_spaces_are_parsed(self) -> None:
        # #1831: a doctest's name holds spaces, which `\S+` stopped at, so
        # every red `--doc` run read as 0 parsed failures against exit 101.
        # The `compile_fail` one is named as `--list` names it, without the
        # ` - compile fail` its progress line adds.
        verdict, failed, detail = perturb.classify(101, DOCTEST_RUN)
        self.assertEqual((verdict, detail), ("caught", ""))
        self.assertEqual(
            failed,
            {
                "src/lib.rs - one (line 12)",
                "src/lib.rs - one (line 17)",
                "src/lib.rs - one (line 7)",
            },
        )

    def test_failures_are_named_as_cargo_lists_them(self) -> None:
        # `cargo test --no-fail-fast` (rustc 1.99.1) on a crate whose unit
        # target has a `#[should_panic]` test that does not panic, and a
        # test that prints cargo-shaped lines before failing. The progress
        # line spells the first `tests::sp - should panic`, which no
        # `--exact` or `expect` spells; the second prints a fake progress
        # line, header, `error[`, `failures:` list and `---- ` line, none
        # of which is cargo's report. Same names under `-q` and
        # `--format terse`, which print no progress lines at all.
        for label, progress in (("default", NOISY_PROGRESS), ("-q", "")):
            out = NOISY_RUN.replace("{progress}", progress)
            verdict, failed, detail = perturb.classify(101, out)
            self.assertEqual((verdict, detail), ("caught", ""), label)
            self.assertEqual(failed, {"tests::noisy", "tests::plain", "tests::sp"}, label)

    def test_a_printed_header_does_not_hide_a_real_build_error(self) -> None:
        # A `---- ` line that is not a header, before an `error[`, then a
        # real header: a header pattern free to cross lines joins the two
        # and strips the error with them.
        out = "---- not a header\nerror[E0308]: x\n---- t stdout ----\nboom\n\n" + failing("t", escaped=False)
        self.assertEqual(perturb.classify(101, out)[0], "BUILD-ERROR")

    def test_section_stripping_is_linear(self) -> None:
        # A header pattern free to cross lines rescans to the end of the
        # input from every `---- ` line with no real header after it:
        # 8 000 of them in one test's output took 4.7 s.
        noise = "".join(f"---- section {i}\n" for i in range(8_000))
        out = (
            "running 1 test\ntest t ... FAILED\n\nfailures:\n\n---- t stdout ----\n"
            + noise
            + "\nfailures:\n    t\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\n"
        )
        start = time.perf_counter()
        self.assertEqual(perturb.classify(101, out)[:2], ("caught", {"t"}))
        self.assertLess(time.perf_counter() - start, 0.5)

    def test_a_doctest_that_fails_to_compile_is_a_failing_test_not_a_build_error(self) -> None:
        # Its `error[E0425]` sits in its own captured stdout. Read as a
        # build failure, every `compile_fail` perturbation is BUILD-ERROR.
        self.assertEqual(perturb.classify(101, DOCTEST_RUN)[0], "caught")
        # A real build failure outside any test's output still is one.
        broken = "error: this file contains an unclosed delimiter\nerror: could not compile `dt` (lib) due to 1 previous error\n"
        self.assertEqual(perturb.classify(101, broken)[0], "BUILD-ERROR")
        self.assertEqual(perturb.classify(101, "error[E0425]: cannot find\n" + DOCTEST_RUN)[0], "BUILD-ERROR")

    def test_a_name_never_runs_past_its_own_line(self) -> None:
        # Two names in one `failures:` list, one with spaces: each line
        # is one name, whole.
        self.assertEqual(perturb.classify(101, failing("a", "b c", escaped=False))[1], {"a", "b c"})

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
            f"import pathlib, sys\npathlib.Path({str(seen)!r}).write_bytes(pathlib.Path({str(self.repo.target)!r}).read_bytes())\nsys.stdout.write({failing('a', escaped=False)!r})\nsys.exit(101)\n"
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
        manifest = self.repo.manifest(failing("a::b"), 101, [_case(expect=["a::b"])])
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
        manifest = self.repo.manifest(failing("a::b"), 101, [_case(expect=["a::b"])])
        rc, backup, record = self._main(manifest)
        self.assertEqual(rc, 0)
        self.assertEqual(record, backup / "perturb-results.json")
        self.assertEqual(json.loads(record.read_text())[0]["verdict"], "caught")
        self.assertFalse((self.repo.root / "perturb-results.json").exists(), "no litter in the repo")
        self._drop(backup)

    def _cargo_stub(self) -> pathlib.Path:
        """An executable named `cargo`, outside the repository, that
        records it ran and reports one failing test; the argv check keys
        on the program's basename, so only this shape reaches it."""
        bin_dir = pathlib.Path(self._tmp.name) / "bin"
        bin_dir.mkdir()
        stub = bin_dir / "cargo"
        ran = pathlib.Path(self._tmp.name) / "cargo-ran"
        stub.write_text(
            f"#!{sys.executable}\nimport pathlib, sys\npathlib.Path({str(ran)!r}).write_text(' '.join(sys.argv[1:]))\n"
            f"sys.stdout.write({failing('a::b', escaped=False)!r})\nsys.exit(101)\n"
        )
        stub.chmod(0o755)
        return stub

    def test_refuses_a_multi_target_argv_before_any_git_read_or_write(self) -> None:
        stub = self._cargo_stub()
        # Dirty too: the argv refusal must come first, so the target
        # check's message is absent and no `git` ran to produce it.
        dirty = ORIGINAL + b"// uncommitted\n"
        self.repo.target.write_bytes(dirty)
        manifest = {**self.repo.manifest("", 0, [_case()]), "cargo": [str(stub), "test", "-p", "a", "-p", "b", "--lib"]}
        results, rc, _, err = self._sweep(manifest)
        self.assertEqual((results, rc), ([], 2))
        self.assertIn("without --no-fail-fast: 2 package selectors", err)
        self.assertNotIn("not tracked and clean", err)
        self.assertEqual(self.repo.target.read_bytes(), dirty)
        self.assertEqual(list(self.backup.iterdir()), [], "nothing was written")
        self.assertFalse((pathlib.Path(self._tmp.name) / "cargo-ran").exists(), "cargo never ran")

    def test_an_accepted_cargo_argv_runs(self) -> None:
        # The positive control for the refusal above: the same stub with
        # the flag, and a single-target argv without it, reach the run.
        stub = self._cargo_stub()
        ran = pathlib.Path(self._tmp.name) / "cargo-ran"
        for tail in (["-p", "a", "-p", "b", "--lib", "--no-fail-fast"], ["-p", "a", "--lib"]):
            ran.unlink(missing_ok=True)
            manifest = {**self.repo.manifest("", 0, [_case(expect=["a::b"])]), "cargo": [str(stub), "test", *tail]}
            results, rc, _, err = self._sweep(manifest)
            self.assertEqual((rc, results[0]["verdict"]), (0, "caught"), err)
            self.assertEqual(ran.read_text(), " ".join(["test", *tail]))
            self.assertEqual(self.repo.target.read_bytes(), ORIGINAL)

    def test_argv_rule_refuses_every_multi_target_shape(self) -> None:
        refused = {
            "two packages": ["-p", "a", "-p", "b", "--lib"],
            "no target selector": ["-p", "a"],
            "no package selector": ["--lib"],
            "nothing selected": [],
            "two targets": ["-p", "a", "--lib", "--test", "t"],
            # Each beside an otherwise single-target argv, so the flag
            # itself is the only reason it is refused. Spelled out rather
            # than read from `perturb.MULTI_TARGET_FLAGS`: a table built
            # from the set under test loses a case with every member the
            # set loses.
            **{
                flag: ["-p", "a", "--lib", flag]
                for flag in ("--workspace", "--all", "--bins", "--tests", "--examples", "--benches", "--all-targets")
            },
            "--package= spelling": ["--package=a", "--package=b", "--doc"],
            "-pNAME spelling": ["-pa", "-pb", "--doc"],
            # cargo reads `-qpa` as `-q -p a`, so these are two packages.
            "a -p inside a cluster": ["-qpa", "-p", "b", "--lib"],
            "two clusters": ["-qpa", "-vpb", "--lib"],
            # Each triple is a test executable of its own.
            "two triples": ["-p", "a", "--lib", "--target", "x86_64-unknown-linux-gnu", "--target", "i686-unknown-linux-gnu"],
            "--target= spelling": ["-p", "a", "--lib", "--target=x86_64-unknown-linux-gnu", "--target=i686-unknown-linux-gnu"],
        }
        for label, tail in refused.items():
            self.assertIsNotNone(perturb.cargo_argv_problem(["cargo", "test", *tail]), label)
            # The flag anywhere before `--` accepts each one.
            for at in (0, len(tail)):
                flagged = [*tail[:at], "--no-fail-fast", *tail[at:]]
                self.assertIsNone(perturb.cargo_argv_problem(["cargo", "test", *flagged]), (label, at))

    def test_argv_rule_accepts_one_package_with_one_target(self) -> None:
        for tail in (
            ["-p", "a", "--lib"],
            ["-p", "a", "--doc"],
            ["--package", "a", "--test", "t"],
            ["--package=a", "--test=t"],
            ["-pa", "--bin", "b"],
            ["-p=a", "--example", "e"],
            ["-p", "a", "--bench", "b"],
            # Feature, profile and filter arguments do not add targets.
            ["-p", "a", "--lib", "--all-features", "-F", "x,y", "--release", "some_filter"],
            # Short-option clusters, read as cargo reads them.
            ["-qp", "a", "--lib"],
            ["-qpa", "--lib"],
            ["-vqp=a", "--lib"],
            ["-p", "a", "--lib", "-j8", "-Fx"],
            # A value holding a `p` is a value, not a second `-p`: the
            # letters after `-F`, `-j` or `-m` are its argument.
            ["-p", "a", "--lib", "-Fpx"],
            ["-p", "a", "--lib", "-jp"],
            ["-p", "a", "--lib", "-mp"],
            # One triple is one build.
            ["-p", "a", "--lib", "--target", "x86_64-unknown-linux-gnu"],
        ):
            self.assertIsNone(perturb.cargo_argv_problem(["cargo", "test", *tail]), tail)

    def test_argv_rule_ignores_everything_after_a_bare_double_dash(self) -> None:
        # Those go to the test binary: neither a selector nor the flag.
        self.assertIsNone(perturb.cargo_argv_problem(["cargo", "test", "-p", "a", "--lib", "--", "--test", "x", "--workspace"]))
        self.assertIn(
            "2 package selectors", perturb.cargo_argv_problem(["cargo", "test", "-p", "a", "-p", "b", "--lib", "--", "--no-fail-fast"]) or ""
        )

    def test_argv_rule_refuses_a_glob_selector(self) -> None:
        # Cargo expands these, so one selector can still run many targets.
        for tail in (
            ["-p", "bca-*", "--lib"],
            ["-p", "a?", "--lib"],
            ["-p", "a", "--test", "*"],
            ["-p", "a", "--test=t[12]"],
            ["-p", "a", "--bin", "b*"],
        ):
            problem = perturb.cargo_argv_problem(["cargo", "test", *tail])
            self.assertIn("is a glob", problem or "", tail)
            self.assertIsNone(perturb.cargo_argv_problem(["cargo", "test", *tail, "--no-fail-fast"]), tail)

    def test_argv_rule_recognises_every_spelling_of_cargo_test(self) -> None:
        multi = ["-p", "a", "-p", "b", "--lib"]
        for head in (
            ["cargo", "test"],
            ["cargo.exe", "test"],
            ["/home/u/.cargo/bin/cargo", "test"],
            ["cargo", "+nightly", "test"],
            ["cargo", "t"],
            ["cargo", "--locked", "test"],
            ["cargo", "--color", "never", "-Z", "unstable-options", "test"],
            ["cargo", "+nightly", "-q", "--config=build.jobs=2", "test"],
            # A global option's value as a separate token, which the
            # skip must consume with it.
            ["cargo", "--config", "build.jobs=2", "test"],
            ["cargo", "-C", "sub", "test"],
            # A wrapper in front of cargo is seen through, including one
            # whose own argument also ends in `cargo`.
            ["timeout", "1800", "cargo", "test"],
            ["env", "RUST_BACKTRACE=1", "cargo", "test"],
            ["nice", "cargo", "test"],
            ["rustup", "run", "nightly", "cargo", "test"],
            ["env", "CARGO=/usr/bin/cargo", "cargo", "test"],
        ):
            self.assertIsNotNone(perturb.cargo_argv_problem([*head, *multi]), head)

    def test_argv_rule_leaves_other_programs_and_subcommands_alone(self) -> None:
        # The self-tests' own stub argv, and cargo subcommands that are
        # not `cargo test`, carry no target rule.
        self.assertIsNone(perturb.cargo_argv_problem(self.repo.manifest(RED, 101, [_case()])["cargo"]))
        for argv in (
            [],
            ["cargo"],
            ["cargo", "build", "-p", "a", "-p", "b"],
            ["cargo", "nextest", "run", "--workspace"],
            ["cargo-test", "-p", "a", "-p", "b"],
            # A shell string is not parsed, and a wrapped cargo that does
            # not run `test` is not `cargo test`.
            ["sh", "-c", "cargo test --workspace"],
            ["timeout", "60", "cargo", "build", "-p", "a", "-p", "b"],
        ):
            self.assertIsNone(perturb.cargo_argv_problem(argv), argv)

    # The unittest runner, against a real `python -m unittest -v` run on
    # a module of the scratch repository: the parser is checked on the
    # report this interpreter prints, not on a transcription of one.

    def _commit(self, message: str) -> None:
        env = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t", "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t"}
        for argv in (["git", "add", "-A"], ["git", "commit", "-q", "-m", message]):
            subprocess.run(argv, cwd=self.repo.root, check=True, env=env)

    def _unittest_repo(self, body: str) -> dict:
        """Commits `t_sub.py` (the subject) and `t_case.py` (its tests,
        `body` being the class body) and returns a manifest skeleton."""
        (self.repo.root / "t_sub.py").write_text("def value():\n    return 1\n")
        (self.repo.root / "t_case.py").write_text(
            "import unittest\nimport t_sub\n\n\nclass T(unittest.TestCase):\n" + body
        )
        self._commit("unittest")
        return {
            "cwd": str(self.repo.root),
            "runner": "unittest",
            "command": [sys.executable, "-B", "-m", "unittest", "-v", "t_case"],
        }

    @staticmethod
    def _flip(expect: list[str]) -> dict:
        return {"name": "flip", "file": "t_sub.py", "old": "return 1", "new": "return 2", "expect": expect}

    def test_unittest_caught_by_the_named_test(self) -> None:
        manifest = self._unittest_repo(
            "    def test_value(self):\n        self.assertEqual(t_sub.value(), 1)\n"
            "    def test_other(self):\n        pass\n"
        )
        results, rc, _, err = self._sweep({**manifest, "cases": [self._flip(["t_case.T.test_value"])]})
        self.assertEqual((rc, results[0]["verdict"]), (0, "caught"), err)
        self.assertEqual(results[0]["failed"], ["t_case.T.test_value"])
        self.assertEqual((self.repo.root / "t_sub.py").read_text(), "def value():\n    return 1\n")

    def test_unittest_uncaught_when_no_test_reads_the_line(self) -> None:
        manifest = self._unittest_repo("    def test_other(self):\n        pass\n")
        results, rc, _, _ = self._sweep({**manifest, "cases": [self._flip([])]})
        self.assertEqual((rc, results[0]["verdict"]), (1, "UNCAUGHT"))

    def test_unittest_subtest_failure_counts_against_its_test(self) -> None:
        # Two subtests fail; the test holding them is one failing test.
        manifest = self._unittest_repo(
            "    def test_value(self):\n"
            "        for i in range(3):\n"
            "            with self.subTest(i=i):\n"
            "                self.assertEqual(t_sub.value(), 1)\n"
        )
        results, rc, _, err = self._sweep({**manifest, "cases": [self._flip(["t_case.T.test_value"])]})
        self.assertEqual((rc, results[0]["verdict"], results[0]["failed"]), (0, "caught", ["t_case.T.test_value"]), err)

    def test_unittest_skips_and_expected_failures_are_not_failures(self) -> None:
        # `expected failures=1` must not be read as `failures=1`, or the
        # summary disagrees with the parsed set and every case is a
        # harness error.
        manifest = self._unittest_repo(
            "    @unittest.skip('s')\n    def test_skip(self):\n        pass\n"
            "    @unittest.expectedFailure\n    def test_xf(self):\n        self.fail()\n"
            "    def test_value(self):\n        self.assertEqual(t_sub.value(), 1)\n"
        )
        results, rc, _, err = self._sweep({**manifest, "cases": [self._flip(["t_case.T.test_value"])]})
        self.assertEqual((rc, results[0]["verdict"]), (0, "caught"), err)

    def test_unittest_unexpected_success_is_a_failure(self) -> None:
        # unittest exits 1 on one, and the perturbation is what made it.
        manifest = self._unittest_repo(
            "    @unittest.expectedFailure\n    def test_value(self):\n        self.assertEqual(t_sub.value(), 2)\n"
        )
        results, rc, _, err = self._sweep({**manifest, "cases": [self._flip(["t_case.T.test_value"])]})
        self.assertEqual((rc, results[0]["verdict"]), (0, "caught"), err)

    def test_unittest_import_failure_is_a_harness_error(self) -> None:
        # A syntax error in the subject kills the loader outright (no
        # report); one in a discovered module is reported as an error of
        # a synthetic loader test. Neither is an assertion catching it.
        manifest = self._unittest_repo("    def test_value(self):\n        self.assertEqual(t_sub.value(), 1)\n")
        case = {"name": "break", "file": "t_sub.py", "old": "return 1", "new": "return (", "expect": ["t_case.T.test_value"]}
        results, rc, out, _ = self._sweep({**manifest, "cases": [case]})
        self.assertEqual((rc, results[0]["verdict"]), (1, "HARNESS-ERROR"))
        self.assertIn("no outcome recorded: the loader raised", out)
        discovered = {**manifest, "command": [sys.executable, "-B", "-m", "unittest", "discover", "-v", "-p", "t_case.py"]}
        results, rc, out, _ = self._sweep({**discovered, "cases": [case]})
        self.assertEqual((rc, results[0]["verdict"]), (1, "HARNESS-ERROR"), out)
        self.assertIn("import or discovery failed (unittest.loader._FailedTest.", out)

    def test_unittest_refuses_a_red_suite_before_any_write(self) -> None:
        manifest = self._unittest_repo("    def test_value(self):\n        self.assertEqual(t_sub.value(), 2)\n")
        results, rc, _, err = self._sweep({**manifest, "cases": [self._flip([])]})
        self.assertEqual((results, rc), ([], 2))
        self.assertIn("unperturbed suite is not green", err)
        self.assertEqual(list(self.backup.iterdir()), [], "nothing was backed up, so nothing was written")

    def test_unittest_reads_no_stale_bytecode(self) -> None:
        # A same-length edit restored within the second leaves the
        # source's size and mtime as they were, so a `.pyc` written from
        # the mutant is valid to the importer and `-B` alone reads it.
        # Seed exactly that `.pyc` in the repository's `__pycache__`: a
        # run that honours it sees `2` and fails although the file says 1.
        manifest = self._unittest_repo("    def test_value(self):\n        self.assertEqual(t_sub.value(), 1)\n")
        self._seed_stale_pyc("def value():\n    return 2\n")
        stale = subprocess.run(manifest["command"], cwd=self.repo.root, capture_output=True, text=True)
        self.assertNotEqual(stale.returncode, 0, "the seeded .pyc must be read without the private cache")
        # Reading it, the preflight sees a red suite and refuses the sweep.
        results, rc, _, err = self._sweep({**manifest, "cases": [self._flip(["t_case.T.test_value"])]})
        self.assertNotIn("not green", err, "the run read the stale .pyc")
        self.assertEqual((rc, [r["verdict"] for r in results]), (0, ["caught"]), err)

    def _seed_stale_pyc(self, mutant: str) -> None:
        """Writes `t_sub.py`'s `.pyc` for `mutant` into the repository's
        `__pycache__`, stamped with the committed source's mtime, so the
        importer takes it as current for the unchanged file. The mutant
        carries the same stamp before it is compiled, so the seed holds
        whichever second the writes land in."""
        sub = self.repo.root / "t_sub.py"
        original, stat = sub.read_text(), sub.stat()
        sub.write_text(mutant)
        os.utime(sub, ns=(stat.st_atime_ns, stat.st_mtime_ns))
        subprocess.run([sys.executable, "-c", "import py_compile; py_compile.compile('t_sub.py')"], cwd=self.repo.root, check=True)
        sub.write_text(original)
        os.utime(sub, ns=(stat.st_atime_ns, stat.st_mtime_ns))

    def test_unittest_writes_no_bytecode_for_a_later_case_to_read(self) -> None:
        # Without `-B`, a run that may write bytecode leaves the first
        # mutant's `.pyc` where the second same-length mutant, written in
        # the same second, would load it. The private prefix is shared by
        # every run of a sweep, so writes must be off as well.
        manifest = self._unittest_repo("    def test_value(self):\n        self.assertEqual(t_sub.value(), 1)\n")
        manifest["command"] = [sys.executable, "-m", "unittest", "-v", "t_case"]
        cases = [
            {"name": "two", "file": "t_sub.py", "old": "return 1", "new": "return 2", "expect": ["t_case.T.test_value"]},
            {"name": "three", "file": "t_sub.py", "old": "return 1", "new": "return 3", "expect": ["t_case.T.test_value"]},
        ]
        stat = (self.repo.root / "t_sub.py").stat()
        real_write = pathlib.Path.write_bytes

        def write_keeping_mtime(path: pathlib.Path, data: bytes) -> int:
            # Every write in the same second, as a fast sweep's are.
            n = real_write(path, data)
            if path.name == "t_sub.py":
                os.utime(path, ns=(stat.st_atime_ns, stat.st_mtime_ns))
            return n

        with mock.patch.object(pathlib.Path, "write_bytes", write_keeping_mtime):
            results, rc, _, err = self._sweep({**manifest, "cases": cases})
        self.assertEqual([r["verdict"] for r in results], ["caught", "caught"], err)
        self.assertEqual(list(self.backup.glob("**/*.pyc")), [], "the sweep wrote bytecode")

    def test_unittest_reads_its_report_not_what_a_test_printed(self) -> None:
        # A test printing a progress line of its own: read with the
        # report, it is one more unexpected success than the summary
        # counts, and the green preflight reads as a harness error.
        manifest = self._unittest_repo(
            "    def test_value(self):\n"
            "        print('test_fake (x.Y.test_fake) ... unexpected success')\n"
            "        self.assertEqual(t_sub.value(), 1)\n"
        )
        results, rc, _, err = self._sweep({**manifest, "cases": [self._flip(["t_case.T.test_value"])]})
        self.assertEqual((rc, results[0]["verdict"], results[0]["failed"]), (0, "caught", ["t_case.T.test_value"]), err)

    def test_unittest_counts_several_unexpected_successes(self) -> None:
        # From 3.11 the report lists consecutive unexpected successes
        # under one `====` rule; before it, not at all. The progress lines
        # name each on every version.
        manifest = self._unittest_repo(
            "    @unittest.expectedFailure\n    def test_a(self):\n        self.assertEqual(t_sub.value(), 2)\n"
            "    @unittest.expectedFailure\n    def test_b(self):\n        self.assertEqual(t_sub.value(), 2)\n"
            "    def test_c(self):\n        self.assertEqual(t_sub.value(), 1)\n"
        )
        results, rc, _, err = self._sweep({**manifest, "cases": [self._flip(["t_case.T.test_a", "t_case.T.test_b"])]})
        self.assertEqual((rc, results[0]["verdict"]), (0, "caught"), err)
        self.assertEqual(results[0]["failed"], ["t_case.T.test_a", "t_case.T.test_b", "t_case.T.test_c"])

    def test_unittest_reads_no_header_from_a_traceback(self) -> None:
        # A failure message holding a header-shaped line is not a second
        # failure: headers sit directly under their `====` rule.
        manifest = self._unittest_repo(
            "    def test_value(self):\n"
            "        self.assertEqual(t_sub.value(), 1, '\\nFAIL: fake (x.Y.fake)')\n"
        )
        results, rc, _, err = self._sweep({**manifest, "cases": [self._flip(["t_case.T.test_value"])]})
        self.assertEqual((rc, results[0]["verdict"], results[0]["failed"]), (0, "caught", ["t_case.T.test_value"]), err)

    def test_unittest_reads_no_header_a_test_wrote_to_stderr(self) -> None:
        # Before the outer report, a test's own stderr: a ruled header it
        # printed, and the report of a failing unittest it ran inside
        # itself. Each is followed by a passing test, so the outer report
        # cannot simply start at the first rule after the last `... ok`
        # of the test that wrote it.
        manifest = self._unittest_repo(
            "    def test_a_printer(self):\n"
            "        import sys\n"
            "        sys.stderr.write('\\n' + '=' * 70 + '\\nFAIL: fake (x.Y.fake)\\n' + '-' * 70 + '\\n')\n"
            "        self.assertEqual(t_sub.value(), 1)\n"
            "    def test_b_nested(self):\n"
            "        class Inner(unittest.TestCase):\n"
            "            def test_inner(self):\n"
            "                self.fail()\n"
            "        suite = unittest.defaultTestLoader.loadTestsFromTestCase(Inner)\n"
            "        result = unittest.TextTestRunner(verbosity=2).run(suite)\n"
            "        self.assertEqual(len(result.failures), 1)\n"
            "    def test_c_clean(self):\n"
            "        pass\n"
        )
        results, rc, _, err = self._sweep({**manifest, "cases": [self._flip(["t_case.T.test_a_printer"])]})
        self.assertEqual((rc, results[0]["verdict"], results[0]["failed"]), (0, "caught", ["t_case.T.test_a_printer"]), err)

    def test_unittest_counts_no_unexpected_success_from_a_traceback(self) -> None:
        # A failure message, and `-b`'s captured stdout appended to the
        # failure block, each holding a whole progress line ending
        # `... unexpected success`: neither is a progress line, because
        # both sit in the outer report after the last one.
        line = "worker (a.B.worker) ... unexpected success"
        manifest = self._unittest_repo(
            "    def test_value(self):\n"
            f"        print({line!r})\n"
            f"        self.assertEqual(t_sub.value(), 1, '\\n' + {line!r})\n"
            "    def test_clean(self):\n"
            "        pass\n"
        )
        manifest["command"] = [sys.executable, "-B", "-m", "unittest", "-v", "-b", "t_case"]
        results, rc, _, err = self._sweep({**manifest, "cases": [self._flip(["t_case.T.test_value"])]})
        self.assertEqual((rc, results[0]["verdict"], results[0]["failed"]), (0, "caught", ["t_case.T.test_value"]), err)

    def test_unittest_counts_no_unexpected_success_mid_line(self) -> None:
        # Before the report, a test's own stderr holding a progress-shaped
        # line after other text on the same line: a progress line starts
        # its line, so this one is not counted.
        manifest = self._unittest_repo(
            "    def test_value(self):\n"
            "        import sys\n"
            "        sys.stderr.write('prefix worker (a.B.worker) ... unexpected success\\n')\n"
            "        self.assertEqual(t_sub.value(), 1)\n"
            "    def test_clean(self):\n"
            "        pass\n"
        )
        results, rc, _, err = self._sweep({**manifest, "cases": [self._flip(["t_case.T.test_value"])]})
        self.assertEqual((rc, results[0]["verdict"], results[0]["failed"]), (0, "caught", ["t_case.T.test_value"]), err)

    def test_unittest_names_the_test_that_failed_not_one_it_printed(self) -> None:
        # Under `-b` a failing test's own output is copied into its block
        # of the report, after its real header; a header it printed there
        # names a test that passed. A reading of the text that took the
        # last header named the passing one, and a sweep expecting it
        # exited 0.
        manifest = self._unittest_repo(
            "    def test_real(self):\n"
            "        print('\\n' + '=' * 70 + '\\nFAIL: test_bystander (t_case.T.test_bystander)\\n' + '-' * 70)\n"
            "        self.assertEqual(t_sub.value(), 1)\n"
            "    def test_bystander(self):\n"
            "        pass\n"
        )
        for flags in (["-v", "-b"], ["-v"], []):
            manifest["command"] = [sys.executable, "-B", "-m", "unittest", *flags, "t_case"]
            results, rc, _, err = self._sweep({**manifest, "cases": [self._flip(["t_case.T.test_bystander"])]})
            self.assertEqual(results[0]["failed"], ["t_case.T.test_real"], (flags, err))
            self.assertEqual((rc, results[0]["missing_expected"]), (1, ["t_case.T.test_bystander"]), flags)

    def test_unittest_record_is_the_one_this_run_wrote(self) -> None:
        # The record is the only thing read, so a run must find exactly
        # its own: none left by an earlier run in the same sweep, and the
        # recorder must be what ran. A case whose mutant kills the loader
        # after a run that recorded a catch must not inherit that catch.
        manifest = self._unittest_repo("    def test_value(self):\n        self.assertEqual(t_sub.value(), 1)\n")
        cases = [
            self._flip(["t_case.T.test_value"]),
            {"name": "break", "file": "t_sub.py", "old": "return 1", "new": "return (", "expect": []},
        ]
        results, _, _, err = self._sweep({**manifest, "cases": cases})
        self.assertEqual([r["verdict"] for r in results], ["caught", "HARNESS-ERROR"], err)
        self.assertEqual(results[1]["failed"], [], err)

    def test_unittest_refuses_a_partial_run_as_a_catch(self) -> None:
        # Under the mutant, a failing test and then a KeyboardInterrupt
        # before the last test: unittest still calls stopTestRun on the
        # way out, so a record written there held the first failure and
        # the sweep certified it with the third test never run. With `-c`
        # the interrupt stops the run gracefully instead; same partial set.
        body = (
            "    def test_a(self):\n        self.assertEqual(t_sub.value(), 1)\n"
            "    def test_b(self):\n"
            "        if t_sub.value() != 1:\n"
            "            raise KeyboardInterrupt\n"
            "    def test_c(self):\n        self.assertEqual(t_sub.value(), 1)\n"
        )
        # A third shape no signal is involved in, so only the record's
        # timing can refuse it: `tearDownClass` exits with status 7 after
        # a failure, and the class after it never runs. Its own shape
        # only: in the other two it would end those runs as well, and the
        # `stopped` guard that the SIGINT shape exists for would go
        # untested.
        exiting = body.replace("raise KeyboardInterrupt", "pass") + (
            "    @classmethod\n"
            "    def tearDownClass(cls):\n"
            "        if t_sub.value() != 1:\n"
            "            raise SystemExit(7)\n"
            "\n\nclass U(unittest.TestCase):\n"
            "    def test_d(self):\n        self.assertEqual(t_sub.value(), 1)\n"
        )
        # `-c` catches SIGINT; a raised KeyboardInterrupt is not one, so
        # that shape signals the process itself.
        shapes = [
            ("SystemExit in tearDownClass", [], exiting, ["t_case.T.test_a", "t_case.U.test_d"]),
            ("KeyboardInterrupt", [], body, ["t_case.T.test_a", "t_case.T.test_c"]),
            ("SIGINT under -c", ["-c"], body.replace("raise KeyboardInterrupt", "import os, signal; os.kill(os.getpid(), signal.SIGINT)"), ["t_case.T.test_a", "t_case.T.test_c"]),
        ]
        manifest = self._unittest_repo(shapes[0][2])
        for label, flags, shape, expect in shapes:
            if shape != shapes[0][2]:
                (self.repo.root / "t_case.py").write_text("import unittest\nimport t_sub\n\n\nclass T(unittest.TestCase):\n" + shape)
                self._commit(label)
            manifest["command"] = [sys.executable, "-B", "-m", "unittest", *flags, "t_case"]
            results, rc, _, err = self._sweep({**manifest, "cases": [self._flip(expect)]})
            # The verdict on its own: a partial run read as `caught` still
            # exits 1 here, through the never-run test it leaves MISSING.
            # The KeyboardInterrupt shape leaves no record, so only the
            # record-reading test exercises the signal guard.
            self.assertEqual(results[0]["verdict"], "HARNESS-ERROR", (label, err))
            self.assertEqual(rc, 1, label)
            if label.startswith("SystemExit"):
                self.assertIn("no outcome recorded", results[0]["detail"], label)

    def test_unittest_is_linear_in_a_progress_shaped_line(self) -> None:
        # A run-on line of progress-shaped prefixes with no valid verdict
        # backtracked exponentially in the text parser: 25 repetitions
        # took over 2 s.
        manifest = self._unittest_repo(
            "    def test_value(self):\n"
            "        import sys\n"
            "        sys.stderr.write('t (a.B) (i=1) ... ' * 2000 + 'bad verdict\\n')\n"
            "        self.assertEqual(t_sub.value(), 1)\n"
        )
        start = time.perf_counter()
        results, rc, _, err = self._sweep({**manifest, "cases": [self._flip(["t_case.T.test_value"])]})
        self.assertEqual((rc, results[0]["verdict"]), (0, "caught"), err)
        self.assertLess(time.perf_counter() - start, 10)

    def test_unittest_reads_a_failure_whose_message_looks_like_progress(self) -> None:
        # A traceback ends in the assertion's message, which can end in
        # ` ... ok`; a parser that took the report to start after the last
        # progress-shaped line skipped the real failure headers.
        manifest = self._unittest_repo(
            "    def test_value(self):\n"
            "        self.assertEqual(t_sub.value(), 1, 'worker ... ok')\n"
            "    def test_other(self):\n"
            "        self.assertEqual(t_sub.value(), 1, 'second ... FAIL')\n"
        )
        results, rc, _, err = self._sweep({**manifest, "cases": [self._flip(["t_case.T.test_value", "t_case.T.test_other"])]})
        self.assertEqual((rc, results[0]["verdict"]), (0, "caught"), err)
        self.assertEqual(results[0]["failed"], ["t_case.T.test_other", "t_case.T.test_value"])

    def test_unittest_fixture_failure_is_named_for_its_class(self) -> None:
        manifest = self._unittest_repo(
            "    @classmethod\n    def setUpClass(cls):\n        assert t_sub.value() == 1\n"
            "    def test_value(self):\n        pass\n"
        )
        results, rc, out, _ = self._sweep({**manifest, "cases": [self._flip(["t_case.T"])]})
        self.assertEqual((rc, results[0]["verdict"]), (1, "HARNESS-ERROR"))
        self.assertEqual(results[0]["failed"], ["t_case.T"])
        self.assertIn("fixture failed before any test ran", out)

    def test_unittest_ignores_colour_the_environment_asks_for(self) -> None:
        # unittest colours its report from 3.14 on, so the test needs a
        # 3.14 interpreter, whichever one runs this file.
        python = next(
            (p for p in (sys.executable, shutil.which("python3.14")) if p and self._python_version(p) >= (3, 14)),
            None,
        )
        if python is None:
            self.skipTest("no Python 3.14+ interpreter on PATH: unittest only colours from 3.14")
        manifest = self._unittest_repo("    def test_value(self):\n        self.assertEqual(t_sub.value(), 1)\n")
        manifest["command"] = [python, *manifest["command"][1:]]
        with mock.patch.dict(os.environ, {"FORCE_COLOR": "1", "PYTHON_COLORS": "1"}):
            results, rc, _, err = self._sweep({**manifest, "cases": [self._flip(["t_case.T.test_value"])]})
        self.assertEqual((rc, results[0]["verdict"]), (0, "caught"), err)

    @staticmethod
    def _python_version(python: str) -> tuple[int, int]:
        out = subprocess.run([python, "-c", "import sys; print(*sys.version_info[:2])"], capture_output=True, text=True, check=True)
        major, minor = out.stdout.split()
        return int(major), int(minor)

    def test_unittest_refuses_its_argv_before_any_write(self) -> None:
        manifest = self._unittest_repo("    def test_value(self):\n        self.assertEqual(t_sub.value(), 1)\n")
        manifest["command"] = [sys.executable, "-BE", "-m", "unittest", "-v", "t_case"]
        results, rc, _, err = self._sweep({**manifest, "cases": [self._flip([])]})
        self.assertEqual((results, rc), ([], 2))
        self.assertIn("refusing: the unittest argv: `-E` ignores the environment", err)
        self.assertEqual(list(self.backup.iterdir()), [])

    def test_unittest_argv_rule(self) -> None:
        base = [sys.executable, "-B", "-m", "unittest"]
        for tail, needle in (
            (["-v", "-f", "t_case"], "fail-fast"),
            (["-v", "--failfast", "t_case"], "fail-fast"),
            (["-vf", "t_case"], "fail-fast"),
            # argparse accepts any unambiguous prefix of a long option.
            (["-v", "--fail", "t_case"], "fail-fast"),
            (["discover", "-v", "--failf"], "fail-fast"),
        ):
            self.assertIn(needle, perturb.unittest_argv_problem([*base, *tail]) or "", tail)
        self.assertIn("-m unittest", perturb.unittest_argv_problem([sys.executable, "t_case.py"]) or "")
        for interpreter, needle in (
            (["-E"], "`-E` ignores the environment"),
            (["-BE"], "`-E` ignores the environment"),
            (["-I"], "`-I` ignores the environment"),
            (["-X", "pycache_prefix=/tmp/x"], "`-X pycache_prefix`"),
            (["-Xpycache_prefix=/tmp/x"], "`-X pycache_prefix`"),
        ):
            argv = [sys.executable, *interpreter, "-m", "unittest", "-v", "t_case"]
            self.assertIn(needle, perturb.unittest_argv_problem(argv) or "", interpreter)
        for tail in (
            ["-v", "t_case"],
            ["-vb", "t_case"],
            ["--verbose", "-k", "foo", "t_case"],
            # A `-k` value is a pattern: `fast` holds an `f` and `v` is no
            # `-v` of its own, so it is the trailing `-v` that counts.
            ["-kfast", "-v", "t_case"],
            ["discover", "-v", "-p", "t_*.py"],
            # Discovery's start, pattern and top are three optional
            # positionals.
            ["discover", "-v", ".", "t_*.py", "."],
            # Verbosity does not matter: the recorder reads outcomes, not
            # the text, so quiet runs, `-vq` included, are fine.
            ["t_case"],
            ["-vq", "t_case"],
            # `-fv`'s `f` is fail-fast however the cluster ends; `-kfv`'s
            # is a pattern.
            ["-kfv", "t_case"],
        ):
            self.assertIsNone(perturb.unittest_argv_problem([*base, *tail]), tail)
        # Options between them: argparse reads that from 3.12, so the
        # validator, built on this interpreter's argparse as unittest is,
        # answers as this interpreter's unittest would.
        interleaved = perturb.unittest_argv_problem([*base, "discover", ".", "-v", "t_*.py"])
        if sys.version_info >= (3, 12):
            self.assertIsNone(interleaved)
        else:
            self.assertIn("unrecognized arguments: t_*.py", interleaved or "")
        # An argv argparse rejects outright is refused, not raised: before
        # 3.12 `exit_on_error=False` still exits on an unknown option, so
        # only an older interpreter running this file exercises that arm.
        with contextlib.redirect_stderr(io.StringIO()):
            unknown = perturb.unittest_argv_problem([*base, "-v", "--no-such-option", "t_case"])
        self.assertIn("would reject", unknown or "")
        # A fourth positional is one discovery has no slot for.
        self.assertIn("would reject", perturb.unittest_argv_problem([*base, "discover", "-v", "a", "b", "c", "d"]) or "")
        for interpreter in (["-X", "dev"], ["-W", "error"], ["-Werror"]):
            self.assertIsNone(perturb.unittest_argv_problem([sys.executable, *interpreter, "-m", "unittest", "-v", "t"]), interpreter)

    def test_manifest_names_its_runner_unambiguously(self) -> None:
        ok = {"cwd": ".", "cases": []}
        self.assertEqual(perturb.manifest_command({**ok, "cargo": ["c"]}), ("cargo", ["c"]))
        self.assertEqual(perturb.manifest_command({**ok, "runner": "unittest", "command": ["p"]}), ("unittest", ["p"]))
        for manifest, needle in (
            ({**ok}, "exactly one"),
            ({**ok, "cargo": ["c"], "command": ["c"]}, "exactly one"),
            ({**ok, "runner": "unittest", "cargo": ["c"]}, "use `command`"),
            ({**ok, "runner": "pytest", "command": ["p"]}, "unknown runner"),
        ):
            self.assertIn(needle, perturb.manifest_command(manifest), manifest)

    def test_classify_unittest_reads_the_record(self) -> None:
        def record(**fields: object) -> dict:
            return {"tests_run": 2, "stopped": False, "failed": [], "unexpected": [], "loader": [], "fixtures": [], **fields}

        self.assertEqual(perturb.classify_unittest(1, record(failed=["m.T.a"])), ("caught", {"m.T.a"}, ""))
        self.assertEqual(perturb.classify_unittest(1, record(unexpected=["m.T.b"])), ("caught", {"m.T.b"}, ""))
        self.assertEqual(perturb.classify_unittest(0, record()), ("UNCAUGHT", set(), ""))
        for label, rc, rec, detail in (
            ("no record", 1, None, "no outcome recorded"),
            ("loader", 1, record(loader=["unittest.loader._FailedTest.m"]), "import or discovery failed"),
            ("no tests", 0, record(tests_run=0), "0 tests ran"),
            ("fixture before any test", 1, record(tests_run=0, fixtures=["m.T"]), "fixture failed before any test ran"),
            ("exit 0 with a failure", 0, record(failed=["m.T.a"]), "exit 0 where unittest exits 1"),
            ("exit 1 with none", 1, record(), "exit 1 where unittest exits 0"),
            # An atexit hook or a thread ending the process after the record.
            ("exit 7 with a failure", 7, record(failed=["m.T.a"]), "exit 7 where unittest exits 1"),
            ("stopped early", 1, record(failed=["m.T.a"], stopped=True), "stopped early"),
            ("killed", -2, record(failed=["m.T.a"]), "killed by signal 2"),
            # What `read_unittest_record` hands over for a file it could
            # not read as a record.
            ("unreadable", 1, "the record is not JSON", "not JSON"),
        ):
            verdict, _, got = perturb.classify_unittest(rc, rec)
            self.assertEqual(verdict, "HARNESS-ERROR", label)
            self.assertIn(detail, got, label)

    def test_read_unittest_record_refuses_what_the_recorder_did_not_write(self) -> None:
        # A run killed mid-write leaves a truncated file; a module that
        # shadows the recorder could write anything. Neither is a
        # traceback out of the sweep, and neither is read as a result.
        path = pathlib.Path(self._tmp.name) / "record.json"
        self.assertIsNone(perturb.read_unittest_record(path))
        for label, text, needle in (
            ("truncated", '{"tests_run": 1, "fai', "not JSON"),
            ("not utf-8", b"\xff\xfe", "not JSON"),
            ("a list", "[1, 2]", "unexpected shape (list)"),
            ("a key missing", '{"tests_run": 1}', "unexpected shape"),
            ("a wrong type", '{"tests_run": "1", "stopped": false, "failed": [], "unexpected": [], "loader": [], "fixtures": []}', "unexpected shape"),
        ):
            if isinstance(text, bytes):
                path.write_bytes(text)
            else:
                path.write_text(text)
            got = perturb.read_unittest_record(path)
            self.assertIsInstance(got, str, label)
            self.assertIn(needle, got, label)
            self.assertEqual(perturb.classify_unittest(1, got)[0], "HARNESS-ERROR", label)
        good = '{"tests_run": 1, "stopped": false, "failed": ["m.T.a"], "unexpected": [], "loader": [], "fixtures": []}'
        path.write_text(good)
        self.assertEqual(perturb.read_unittest_record(path)["failed"], ["m.T.a"])

    def test_help_and_a_missing_manifest_are_usage_not_a_traceback(self) -> None:
        for argv in (["--help"], []):
            err, out = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err), self.assertRaises(SystemExit) as cm:
                perturb.main(argv)
            self.assertEqual(cm.exception.code, 0 if argv else 2, argv)
            self.assertIn("usage: ", out.getvalue() + err.getvalue())


if __name__ == "__main__":
    unittest.main()
