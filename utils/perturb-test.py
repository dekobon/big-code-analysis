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
            f"sys.stdout.write('test a::b ... FAILED\\n{RED}')\nsys.exit(101)\n"
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

    def test_help_and_a_missing_manifest_are_usage_not_a_traceback(self) -> None:
        for argv in (["--help"], []):
            err, out = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err), self.assertRaises(SystemExit) as cm:
                perturb.main(argv)
            self.assertEqual(cm.exception.code, 0 if argv else 2, argv)
            self.assertIn("usage: ", out.getvalue() + err.getvalue())


if __name__ == "__main__":
    unittest.main()
