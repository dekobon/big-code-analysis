#!/usr/bin/env python3
"""Perturbation sweep driver (`.claude/rules/testing.md`, "Verify
defensive refactor tests by reverting the production code").

Applies one exact substitution to one production file, runs a test
command, records which tests fail, and restores the file from the
in-memory copy in a `finally`. A perturbation that fails nothing is a
finding; one whose anchor does not match is an error, never a silent
no-op (lesson 94).

manifest: {
  "cwd": <repo root>,
  "runner": "cargo" (the default) | "unittest",
  "cargo": ["cargo", "test", "-p", ..., "--", <filters>...],
     or, under any runner,
  "command": [<argv>],
  "record": <results path under cwd; default perturb-results.json in
             the backup directory>,
  "cases": [{"name": ..., "file": <cwd-relative>, "old": ..., "new": ...,
             "expect": [<test names>]}, ...]
}

`expect` is optional but is what makes a `caught` verdict mean "the
test that should discriminate this did". Without it `caught` means
"some test failed", which a bench-harness probe tripping on a zero
reading satisfies; a run with any `expect`-less case prints a note, and
`failed` must be read for those. Names are spelled as each runner lists
them: `cargo test -- --list` (a `#[should_panic]` test without the
` - should panic` its progress line adds), and unittest's
`module.Class.test`.

The cargo runner reads `cargo test` output only: each target's
`failures:` list and `test result:` summary, which `-q` and `--format
terse` print too. A `cargo nextest` argv prints neither, so every red
run under it is `HARNESS-ERROR` — loud, but not a result. Under
`--nocapture` a doctest's compile error is printed outside its captured
section and reads as `BUILD-ERROR`.

The unittest runner, for a Python subject such as the `utils/*-test.py`
gates, takes `"command": ["python3", "-B", "-m", "unittest", ...]` (or
`... -m unittest discover ...`). It:

- parses the argv as unittest does, and refuses fail-fast in any
  spelling (`-f`, `-vf`, `--failfast`, `--fail`), which hides every
  guard after the first failure, and an interpreter option that ignores
  the environment (`-E`, `-I`) or moves the bytecode cache (`-X
  pycache_prefix`);
- runs the suite once unperturbed and refuses one that is not green;
- runs `-m perturb_unittest_recorder` in place of `-m unittest`: the same
  `unittest.main`, which writes each outcome by `TestCase.id()` to a
  file. That record is what is classified; the printed report is never
  read, because a test can print anything — a ruled `FAIL:` header, a
  nested unittest's report — and `-b` copies it into the report, where
  no reading of the text can tell it from unittest's own. A failing
  subtest counts against the test that holds it; an unexpected success
  as a failure; a failing `setUpClass` / `setUpModule` against its class
  or module id. An import or discovery failure, a run stopped early or
  killed, one whose exit status is not unittest's own for its record,
  or one that wrote no record, is `HARNESS-ERROR`, never `caught`: no
  assertion caught it;
- runs each test with no bytecode cache to read and writes none
  (`PYTHONPYCACHEPREFIX` at an empty scratch directory, writes off), so
  a `.pyc` left by a same-length mutant cannot be loaded for the next
  one (#1562). Every run recompiles what it imports, the interpreter's
  own start-up modules included: about 0.13 s per run against 0.05 s
  with a warm cache, so a sweep costs roughly 0.15 s per case. That is
  the price of the guarantee; `-B` alone is the cheap form #1562
  rejects.

Runs on Python 3.10 and later; the self-test skips what an older
interpreter cannot show (unittest's 3.14 colour, 3.12's argparse).

The refusals and classifications are the acceptance criteria of #1744
and #1787, each one a failure the rules file has already paid for:

- A `cargo test` argv that can run more than one test target must carry
  `--no-fail-fast`, or the targets after the first failing one never
  run and their guards read as dead (`.claude/rules/testing.md`, "So is
  the invocation"). Anything but exactly one `-p` with exactly one of
  `--lib`, `--doc`, `--test`, `--bin`, `--example` or `--bench` is
  refused without it, before anything is read or run; so is a glob
  (`*`, `?`, `[`) in either value, since cargo expands it, and a second
  `--target`, since each triple builds test executables of its own. A
  `-p` inside a short-option cluster (`-qpa`) counts like any other. The
  flag is never added on the manifest's behalf: the manifest says what
  it runs. A wrapper in front of cargo (`timeout 1800 cargo test`, `env`,
  `nice`, `rustup run nightly cargo test`) is seen through; an argv with
  no `cargo` program in it (the self-test's stub, a `sh -c` string) or
  whose subcommand is not `test` (a cargo `[alias]` included) is not
  checked.
- Every mutation target must be tracked and clean in git: `git ls-files
  --error-unmatch` refuses an untracked, ignored or nonexistent path,
  and `git status --porcelain` a modified one, both before the first
  write. A crash between a mutation and its restore would otherwise
  leave the only copy of uncommitted work in scratch. Nothing is
  stashed on the user's behalf.
- Files are read, written and restored as bytes, so a CRLF target
  comes back byte-identical and the on-disk backup is exact.
- The on-disk backup goes to a `mkdtemp` directory, never a fixed path,
  and the directory is printed so an interrupted run can be recovered.
- `old != new` and `old` occurs exactly once, asserted before every
  write; a violation is a `BAD-ANCHOR` verdict with no write.
- A build failure is classified by `error[` / `could not compile`,
  never by a bare `error` — `cargo test` prints `error: test failed` on
  every ordinary red run (#1466).
- The parsed failure set is cross-checked against the exit status; the
  two disagreeing in either direction is `HARNESS-ERROR`, not a result
  (#1238). So is a run whose `test result:` lines say no test ran: a
  filter matching nothing exits 0 with `running 0 tests`, which would
  otherwise read as `UNCAUGHT` for every case — the uniform-zero tell.

The replay inputs for the #1697 sweeps (`perturb-c*.json`) are attached
to that issue as `bca-1697-records.zip`; each needs its `cwd` and
`record` paths pointed at the checkout replaying it.

Self-tested by `utils/perturb-test.py` (`make perturb-test`). Nothing
lints `utils/*.py` today (#1477); the self-test is the only gate.
"""

from __future__ import annotations

import argparse
import contextlib
import io
import json
import os
import pathlib
import re
import subprocess
import sys
import tempfile
from collections.abc import Callable
from dataclasses import dataclass

BUILD_ERROR_RE = re.compile(r"^error\[|could not compile", re.M)
# Each target's `failures:` list, which cargo ends with a blank line and
# that target's `test result:`: the names cargo would accept back
# (`--exact`, `--list`), one per indented line. The progress line is not
# that: `#[should_panic]` and `compile_fail` tests add ` - should panic` /
# ` - compile fail` there and nowhere else, and `-q` / `--format terse`
# print no progress lines at all. A name can hold spaces: a doctest's is
# `src/lib.rs - item (line 7)` (#1831).
_FAILURES_LIST = r"^failures:\n((?: {4}\S[^\n]*\n)+)\ntest result: "
FAILURES_LIST_RE = re.compile(_FAILURES_LIST, re.M)
# A test's captured output: from its `---- name stdout ----` header to the
# next such header, or to the `failures:` list after the last one. Both
# ends are matched whole, header on one line and list up to its `test
# result:`, because a test may print lines starting `---- `, a
# `failures:` line, or `test x ... FAILED` itself. What a test printed is
# not cargo's report: a doctest's own `error[E…]` is a failing test, not
# a build that never reached the tests. A test that prints a whole target
# report, list and `test result:` both, is cargo's shape exactly; no line
# parser can tell the two apart.
_HEADER = r"^---- [^\n]+ stdout ----$"
TEST_STDOUT_RE = re.compile(rf"{_HEADER}.*?(?={_HEADER}|{_FAILURES_LIST}|\Z)", re.M | re.S)
TEST_RESULT_RE = re.compile(r"^test result: \w+\. (\d+) passed; (\d+) failed;", re.M)

# The module a unittest sweep runs in place of `unittest` (see
# `perturb_unittest_recorder.py`): unittest's own `main`, writing each
# outcome by `TestCase.id()` to the file named in RECORD_ENV.
UNITTEST_RECORDER = "perturb_unittest_recorder"
UTILS_DIR = pathlib.Path(__file__).resolve().parent
UNITTEST_RECORD_ENV = "PERTURB_UNITTEST_RECORD"

CARGO_PROGRAMS = frozenset({"cargo", "cargo.exe"})
# `t` is cargo's built-in alias for `test`.
TEST_SUBCOMMANDS = frozenset({"test", "t"})
# Options cargo accepts before the subcommand whose value is the next
# token; skipping them is what finds `test` in `cargo --color never test`.
CARGO_GLOBAL_VALUE_OPTS = frozenset({"--color", "--config", "-C", "-Z"})
PACKAGE_OPTS = frozenset({"-p", "--package"})
SINGLE_TARGET_FLAGS = frozenset({"--lib", "--doc"})
SINGLE_TARGET_OPTS = frozenset({"--test", "--bin", "--example", "--bench"})
MULTI_TARGET_FLAGS = frozenset({"--workspace", "--all", "--bins", "--tests", "--examples", "--benches", "--all-targets"})
# `cargo test`'s short options that take a value (`-C` and `-Z` are
# cargo's global ones). In a cluster such as `-qpa`, clap reads every
# letter as a flag up to the first of these, whose value is the rest of
# the token, or the next token when nothing is left.
SHORT_VALUE_OPTS = frozenset("pFjmCZ")
GLOB_CHARS = frozenset("*?[")


def split_short_cluster(tok: str) -> list[str]:
    """A short-option token as clap reads it: `-qpa` is `-q -p a`, `-vv`
    is `-v -v`, and `-p=a` is `-p a`."""
    split: list[str] = []
    for at, letter in enumerate(tok[1:], start=1):
        split.append(f"-{letter}")
        if letter in SHORT_VALUE_OPTS:
            value = tok[at + 1 :].removeprefix("=")
            if value:
                split.append(value)
            break
    return split


def cargo_test_args(cargo: list[str]) -> list[str] | None:
    """The `cargo test` options of `cargo`, before any bare `--` and with
    `--opt=value` and short-option clusters split into separate tokens;
    None when the argv runs no `cargo test`.

    Every token named `cargo` is tried in turn and the first that starts a
    `cargo test` invocation is read, which sees through a wrapper program
    in front of it. Trying each matters: an assignment such as
    `env CARGO=/usr/bin/cargo cargo test` also names `cargo`."""
    for start, program in enumerate(cargo):
        if pathlib.PurePath(program).name not in CARGO_PROGRAMS:
            continue
        rest = cargo[start + 1 :]
        if rest and rest[0].startswith("+"):
            rest = rest[1:]
        while rest and rest[0].startswith("-"):
            rest = rest[2:] if rest[0] in CARGO_GLOBAL_VALUE_OPTS else rest[1:]
        if rest and rest[0] in TEST_SUBCOMMANDS:
            return split_test_options(rest[1:])
    return None


def split_test_options(tokens: list[str]) -> list[str]:
    """`tokens` up to a bare `--`, with `--opt=value` and short-option
    clusters split the way cargo reads them."""
    args: list[str] = []
    for tok in tokens:
        if tok == "--":
            break
        if tok.startswith("--"):
            args.extend(tok.split("=", 1))
        elif tok.startswith("-") and len(tok) > 2:
            args.extend(split_short_cluster(tok))
        else:
            args.append(tok)
    return args


def cargo_argv_problem(cargo: list[str]) -> str | None:
    """Why `cargo` can run more than one test target with no
    `--no-fail-fast`, or None when it cannot (or is not `cargo test`).

    One target is exactly one package selector with exactly one of
    `--lib`, `--doc`, `--test N`, `--bin N`, `--example N`, `--bench N`,
    neither value a glob, built for at most one `--target` triple.
    Feature and profile options do not change the target count, so they
    are not read; nor is a `build.target` list in cargo's config."""
    args = cargo_test_args(cargo)
    if args is None or "--no-fail-fast" in args:
        return None
    reasons: list[str] = []
    packages = targets = triples = 0
    it = iter(args)
    for tok in it:
        if tok in MULTI_TARGET_FLAGS:
            reasons.append(f"`{tok}` selects several targets")
        elif tok in SINGLE_TARGET_FLAGS:
            targets += 1
        elif tok == "--target":
            next(it, "")
            triples += 1
        elif tok in PACKAGE_OPTS or tok in SINGLE_TARGET_OPTS:
            value = next(it, "")
            if GLOB_CHARS.intersection(value):
                reasons.append(f"`{tok} {value}` is a glob, which cargo expands to every match")
            if tok in PACKAGE_OPTS:
                packages += 1
            else:
                targets += 1
    if packages != 1:
        reasons.append(f"{packages} package selectors, need exactly one `-p`")
    if targets != 1:
        reasons.append(f"{targets} target selectors, need exactly one of --lib/--doc/--test/--bin/--example/--bench")
    if triples > 1:
        reasons.append(f"{triples} `--target` triples, each building its own test executables")
    return "; ".join(reasons) or None


def target_problems(cwd: pathlib.Path, files: list[str]) -> list[str]:
    """Why the targets cannot be mutated: one line per untracked,
    ignored or nonexistent path (`git ls-files --error-unmatch`) or per
    modified one (`git status --porcelain`). A git failure is reported as
    one line so the caller refuses rather than proceeds."""
    if not files:
        return []
    tracked = subprocess.run(
        ["git", "ls-files", "--error-unmatch", "--", *files],
        cwd=cwd,
        capture_output=True,
        text=True,
        errors="replace",
    )
    if tracked.returncode != 0:
        lines = [line for line in tracked.stderr.splitlines() if line.strip()]
        return lines or [f"git ls-files failed (exit {tracked.returncode})"]
    status = subprocess.run(
        ["git", "status", "--porcelain", "--untracked-files=all", "--", *files],
        cwd=cwd,
        capture_output=True,
        text=True,
        errors="replace",
    )
    if status.returncode != 0:
        return [f"git status failed (exit {status.returncode}): {status.stderr.strip()}"]
    return [line for line in status.stdout.splitlines() if line.strip()]


def tests_ran(output: str) -> int | None:
    """Tests run across every `test result:` line, or None when the
    output carries no such line (not `cargo test`, or it never reached a
    test target)."""
    totals = TEST_RESULT_RE.findall(output)
    if not totals:
        return None
    return sum(int(passed) + int(failed) for passed, failed in totals)


def classify(returncode: int, output: str) -> tuple[str, set[str], str]:
    """The verdict for one test run, the tests it reported failing, and
    a one-line reason for a harness verdict.

    `caught` and `UNCAUGHT` are results; `BUILD-ERROR` and
    `HARNESS-ERROR` describe the harness: a disagreement between the
    parsed failure set and the exit status, or a run in which no test
    ran."""
    report = TEST_STDOUT_RE.sub("", output)
    failed = {name.strip() for block in FAILURES_LIST_RE.findall(report) for name in block.splitlines()}
    if BUILD_ERROR_RE.search(report):
        return "BUILD-ERROR", failed, "the build failed"
    if (returncode != 0) != bool(failed):
        return "HARNESS-ERROR", failed, f"exit {returncode} disagrees with {len(failed)} parsed failure(s)"
    ran = tests_ran(output)
    if ran is None:
        return "HARNESS-ERROR", failed, "no `test result:` line: not `cargo test` output, or no target ran"
    if ran == 0:
        return "HARNESS-ERROR", failed, "0 tests ran: the filter matched nothing"
    return ("caught" if failed else "UNCAUGHT"), failed, ""


# Interpreter options that stop a run reading its environment (`-E`, `-I`)
# or that point its bytecode cache elsewhere (`-X pycache_prefix`): each
# undoes `unittest_env`'s isolation, so a mutant can run stale bytecode.
PYTHON_ENV_FLAGS = frozenset("EI")


def unittest_cli(discover: bool) -> argparse.ArgumentParser:
    """unittest's own command line, as `python -m unittest [discover]`
    parses it (`unittest/main.py`, `_getParentArgParser` and the main or
    discovery parser on top): abbreviated long options (`--fail` for
    `--failfast`), short-option clusters (`-vq`, `-kfoo`) and discovery's
    three optional positionals between options are read as unittest
    reads them. `--durations` is 3.12+; accepting it on an older host
    only lets unittest refuse it, which the preflight reports."""
    ap = argparse.ArgumentParser(add_help=False, allow_abbrev=True, exit_on_error=False)
    ap.add_argument("-v", "--verbose", dest="verbosity", action="store_const", const=2)
    ap.add_argument("-q", "--quiet", dest="verbosity", action="store_const", const=0)
    ap.add_argument("--locals", action="store_true")
    ap.add_argument("--durations", type=int)
    ap.add_argument("-f", "--failfast", action="store_true")
    ap.add_argument("-c", "--catch", action="store_true")
    ap.add_argument("-b", "--buffer", action="store_true")
    ap.add_argument("-k", action="append")
    if discover:
        ap.add_argument("-s", "--start-directory")
        ap.add_argument("-p", "--pattern")
        ap.add_argument("-t", "--top-level-directory")
        for positional in ("start", "pattern_", "top"):
            ap.add_argument(positional, nargs="?")
    else:
        ap.add_argument("tests", nargs="*")
    return ap


def python_argv_problem(interpreter: list[str]) -> str | None:
    """Why the interpreter options before `-m` defeat the bytecode
    isolation, or None."""
    for at, tok in enumerate(interpreter):
        if tok.startswith("--") or not tok.startswith("-"):
            continue
        # `-X` and `-W` take a value, attached or as the next token.
        flags = tok[1:]
        for i, letter in enumerate(flags):
            if letter in PYTHON_ENV_FLAGS:
                return f"`-{letter}` ignores the environment, and with it the private bytecode cache"
            if letter in "XW":
                value = flags[i + 1 :] or (interpreter[at + 1] if at + 1 < len(interpreter) else "")
                if letter == "X" and value.startswith("pycache_prefix"):
                    return "`-X pycache_prefix` overrides the private bytecode cache"
                break
    return None


def unittest_argv_problem(command: list[str]) -> str | None:
    """Why `command` cannot be a unittest sweep, or None.

    It must run `python -m unittest`, which the runner swaps for its
    recorder; must not stop at the first failure; and the interpreter must
    read the environment `unittest_env` builds, which is how the recorder
    is found and where its record goes."""
    try:
        at = command.index("-m")
    except ValueError:
        return "no `-m unittest`: the runner runs unittest under its recorder"
    if command[at + 1 : at + 2] != ["unittest"]:
        return "no `-m unittest`: the runner runs unittest under its recorder"
    if (problem := python_argv_problem(command[1:at])) is not None:
        return problem
    args = command[at + 2 :]
    discover = args[:1] == ["discover"]
    # Before 3.12 `exit_on_error=False` does not cover unrecognized
    # arguments, which still print usage and exit.
    err = io.StringIO()
    try:
        with contextlib.redirect_stderr(err):
            parsed = unittest_cli(discover).parse_args(args[1:] if discover else args)
    except argparse.ArgumentError as error:
        return f"unittest would reject the argv: {error}"
    except SystemExit:
        return f"unittest would reject the argv: {err.getvalue().strip().splitlines()[-1]}"
    if parsed.failfast:
        return "fail-fast stops at the first failure, so later guards read as dead"
    return None


# The record `perturb_unittest_recorder.py` writes, field by type.
UNITTEST_RECORD_SHAPE = {"tests_run": int, "stopped": bool, "failed": list, "unexpected": list, "loader": list, "fixtures": list}


def read_unittest_record(path: pathlib.Path) -> dict | str | None:
    """The record at `path`: None when there is none, or why what is
    there is not one. A run killed mid-write leaves a truncated file, and
    a module shadowing the recorder (a subject's own
    `perturb_unittest_recorder.py`, which `-m` finds on the cwd first)
    could write anything to the path the environment names."""
    try:
        record = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return None
    except (json.JSONDecodeError, UnicodeDecodeError) as err:
        return f"the record is not JSON ({err}): the run was killed mid-write, or something else wrote it"
    if not isinstance(record, dict) or any(
        not isinstance(record.get(key), kind) for key, kind in UNITTEST_RECORD_SHAPE.items()
    ):
        return f"the record has an unexpected shape ({sorted(record) if isinstance(record, dict) else type(record).__name__}): something else wrote it"
    return record


def classify_unittest(returncode: int, record: dict | str | None) -> tuple[str, set[str], str]:
    """`classify` for a unittest run, given the outcome its recorder wrote
    (`perturb_unittest_recorder.py`), None when it wrote none, or
    `read_unittest_record`'s reason it could not be read. A failing
    subtest counts against the test that holds it, so `failed` is a set
    of test ids, `module.Class.test`; an unexpected success counts as a
    failure, since unittest exits 1 on one."""
    if record is None:
        return "HARNESS-ERROR", set(), "no outcome recorded: the loader raised, or the run ended before its tests finished; no assertion caught it"
    if isinstance(record, str):
        return "HARNESS-ERROR", set(), record
    failed = set(record["failed"]) | set(record["unexpected"]) | set(record["fixtures"])
    if returncode < 0:
        return "HARNESS-ERROR", failed, f"the run was killed by signal {-returncode}: its failures are a partial set"
    if record["stopped"]:
        return "HARNESS-ERROR", failed, "the run was stopped early (an interrupt, or fail-fast): its failures are a partial set"
    if record["loader"]:
        return "HARNESS-ERROR", failed, f"import or discovery failed ({', '.join(record['loader'])}): no assertion caught it"
    if record["tests_run"] == 0:
        if record["fixtures"]:
            return "HARNESS-ERROR", failed, "a class or module fixture failed before any test ran"
        return "HARNESS-ERROR", failed, "0 tests ran: the selection matched nothing"
    # unittest exits 1 with a failure and 0 without; any other status is
    # the process ending some other way after the record was written (an
    # atexit hook or a thread calling os._exit), which is not a result.
    if returncode != (1 if failed else 0):
        return "HARNESS-ERROR", failed, f"exit {returncode} where unittest exits {1 if failed else 0} with {len(failed)} recorded failure(s)"
    return ("caught" if failed else "UNCAUGHT"), failed, ""


@dataclass(frozen=True)
class Runner:
    """What a sweep needs from one test runner."""

    #: Why an argv cannot be swept, or None.
    argv_problem: Callable[[list[str]], str | None]
    #: The refusal message's preamble, before the argv problem.
    refusal: str
    #: An extra refusal line saying what to do, or "".
    refusal_hint: str
    #: Whether to run the suite once unperturbed and refuse a red one.
    preflight: bool
    #: Runs the manifest's argv in `cwd`, with a private scratch
    #: directory, and returns (verdict, failed tests, harness detail, rc).
    run: Callable[[pathlib.Path, list[str], pathlib.Path], tuple[str, set[str], str, int]]


def run_cargo(cwd: pathlib.Path, argv: list[str], _scratch: pathlib.Path) -> tuple[str, set[str], str, int]:
    r = subprocess.run(argv, cwd=cwd, capture_output=True, text=True, errors="replace")
    return (*classify(r.returncode, r.stdout + r.stderr), r.returncode)


def unittest_env(scratch: pathlib.Path, record: pathlib.Path) -> dict[str, str]:
    """A unittest run's environment: the recorder importable and told
    where to write, no existing bytecode read and none written, no
    colour. `-B` alone stops writes but still loads a `.pyc` whose
    recorded size and mtime match, which a same-length edit restored
    within the second leaves behind (#1562); an empty
    `PYTHONPYCACHEPREFIX` hides every `.pyc`, the standard library's
    included, so each run recompiles what it imports: about 0.13 s per
    run against 0.05 s warm, the interpreter's own start-up being most
    of it. The output is not read, so colour only keeps logs plain."""
    path = os.environ.get("PYTHONPATH")
    return {
        **os.environ,
        "PYTHONPATH": str(UTILS_DIR) + (os.pathsep + path if path else ""),
        UNITTEST_RECORD_ENV: str(record),
        "PYTHONPYCACHEPREFIX": str(scratch / "pycache"),
        "PYTHONDONTWRITEBYTECODE": "1",
        "PYTHON_COLORS": "0",
        "NO_COLOR": "1",
    }


def run_unittest(cwd: pathlib.Path, argv: list[str], scratch: pathlib.Path) -> tuple[str, set[str], str, int]:
    """Runs the manifest's `-m unittest` argv as `-m` the recorder, which
    is unittest's own `main` writing its outcome to a file, and classifies
    that record. The printed report is never read: a test can print
    anything, `-b` copies it into the report, and no reading of the text
    tells it from unittest's own."""
    # The first `-m` is the one `unittest_argv_problem` accepted; a `-m`
    # earlier than it would have been refused as an interpreter option.
    at = argv.index("-m")
    recorded = [*argv[: at + 1], UNITTEST_RECORDER, *argv[at + 2 :]]
    # Its own directory, not the scratch one: that holds the sweep's
    # backups, and an interrupted run's record should not sit among them.
    with tempfile.TemporaryDirectory(prefix="bca-perturb-unittest.") as run_dir:
        record_path = pathlib.Path(run_dir) / "record.json"
        env = unittest_env(scratch, record_path)
        r = subprocess.run(recorded, cwd=cwd, capture_output=True, text=True, errors="replace", env=env)
        record = read_unittest_record(record_path)
    return (*classify_unittest(r.returncode, record), r.returncode)


RUNNERS = {
    "cargo": Runner(
        argv_problem=cargo_argv_problem,
        refusal="the cargo argv can run several test targets without --no-fail-fast",
        refusal_hint=(
            "a failing target stops the run and every later target's guards read as dead; "
            "add --no-fail-fast to the manifest's argv — perturb.py never adds it on your behalf"
        ),
        # Cargo sweeps have always skipped this run: a red suite already
        # shows as every case failing the same tests.
        preflight=False,
        run=run_cargo,
    ),
    "unittest": Runner(
        argv_problem=unittest_argv_problem,
        refusal="the unittest argv",
        refusal_hint="",
        # A suite that is red before any perturbation would read every
        # case as `caught`.
        preflight=True,
        run=run_unittest,
    ),
}


def manifest_command(manifest: dict) -> tuple[str, list[str]] | str:
    """The runner and argv a manifest names, or why it names none
    unambiguously. `cargo` is the original key and implies the cargo
    runner; `command` takes any runner."""
    runner = manifest.get("runner", "cargo")
    if runner not in RUNNERS:
        return f"unknown runner {runner!r}; expected one of {sorted(RUNNERS)}"
    if ("cargo" in manifest) == ("command" in manifest):
        return "name the argv under exactly one of `cargo` and `command`"
    if "cargo" in manifest and runner != "cargo":
        return f"the `cargo` key is the cargo runner's; use `command` for {runner!r}"
    return runner, list(manifest.get("command", manifest.get("cargo")))


def anchor_problem(original: bytes, old: bytes, new: bytes) -> str | None:
    if old == new:
        return "`old` equals `new`: the perturbation would run unperturbed code"
    n = original.count(old)
    if n != 1:
        return f"`old` occurs {n} times, need exactly 1"
    return None


def sweep(manifest: dict, backup_dir: pathlib.Path) -> tuple[list[dict], int]:
    """Runs every case; returns the results and the exit status (1 when
    any case is not a clean `caught`, or a harness problem occurred)."""
    cwd = pathlib.Path(manifest["cwd"]).resolve()
    chosen = manifest_command(manifest)
    if isinstance(chosen, str):
        print(f"refusing: {chosen}", file=sys.stderr)
        return [], 2
    name, argv = chosen
    runner = RUNNERS[name]
    argv_problem = runner.argv_problem(argv)
    if argv_problem is not None:
        print(f"refusing: {runner.refusal}: {argv_problem}", file=sys.stderr)
        if runner.refusal_hint:
            print(runner.refusal_hint, file=sys.stderr)
        return [], 2
    cases = manifest["cases"]
    files = sorted({case["file"] for case in cases})
    problems = target_problems(cwd, files)
    if problems:
        for line in problems:
            print(f"refusing: mutation target is not tracked and clean: {line}", file=sys.stderr)
        print(
            "every target must be tracked, unmodified, not ignored and present; "
            "commit or stash first — perturb.py never stashes on your behalf",
            file=sys.stderr,
        )
        return [], 2
    if runner.preflight:
        verdict, failed, detail, _rc = runner.run(cwd, argv, backup_dir)
        if verdict != "UNCAUGHT":
            print(f"refusing: the unperturbed suite is not green ({verdict}: {detail or sorted(failed)})", file=sys.stderr)
            return [], 2
    backups: dict[pathlib.Path, bytes] = {}
    results: list[dict] = []
    rc_all = 0
    no_expect = 0
    try:
        for case in cases:
            path = cwd / case["file"]
            if path not in backups:
                backups[path] = path.read_bytes()
                # keyed by the repo-relative path, not the basename: two
                # files named `js_family.rs` live under abc/ and roles/
                (backup_dir / case["file"].replace("/", "__")).write_bytes(backups[path])
            original = backups[path]
            old, new = case["old"].encode(), case["new"].encode()
            problem = anchor_problem(original, old, new)
            if problem is not None:
                results.append({"name": case["name"], "verdict": "BAD-ANCHOR", "detail": problem})
                print(f"{case['name']}: BAD-ANCHOR {problem}")
                rc_all = 1
                continue
            try:
                path.write_bytes(original.replace(old, new))
                verdict, failed, detail, rc = runner.run(cwd, argv, backup_dir)
            finally:
                path.write_bytes(original)
            expect = set(case.get("expect", []))
            if not expect:
                no_expect += 1
            missing = sorted(expect - failed)
            results.append(
                {
                    "name": case["name"],
                    "verdict": verdict,
                    "detail": detail,
                    "failed": sorted(failed),
                    "missing_expected": missing,
                    "rc": rc,
                }
            )
            if verdict != "caught" or missing:
                rc_all = 1
            print(
                f"{case['name']}: {verdict} rc={rc} failed={sorted(failed)}"
                + (f" MISSING={missing}" if missing else "")
                + (f" ({detail})" if detail else "")
            )
    finally:
        for path, data in backups.items():
            if path.read_bytes() != data:
                path.write_bytes(data)
                print(f"restored {path} from the in-memory copy", file=sys.stderr)
    if no_expect:
        print(
            f"note: {no_expect} case(s) name no `expect` test; for those, `caught` means "
            "some test failed, not that the intended one did — read `failed` before trusting it"
        )
    return results, rc_all


def parse_args(argv: list[str] | None) -> argparse.Namespace:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("manifest", type=pathlib.Path, help="the JSON manifest described above")
    return ap.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    manifest = json.loads(args.manifest.read_text())
    backup_dir = pathlib.Path(tempfile.mkdtemp(prefix="bca-perturb-backup."))
    print(f"on-disk backups: {backup_dir}", file=sys.stderr)
    results, rc = sweep(manifest, backup_dir)
    if results:
        if "record" in manifest:
            record = pathlib.Path(manifest["cwd"]) / manifest["record"]
        else:
            record = backup_dir / "perturb-results.json"
        record.write_text(json.dumps(results, indent=1) + "\n")
        print(f"results: {record}", file=sys.stderr)
    return rc


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
