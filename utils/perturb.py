#!/usr/bin/env python3
"""Perturbation sweep driver (`.claude/rules/testing.md`, "Verify
defensive refactor tests by reverting the production code").

Applies one exact substitution to one production file, runs a cargo
test command, records which tests fail, and restores the file from the
in-memory copy in a `finally`. A perturbation that fails nothing is a
finding; one whose anchor does not match is an error, never a silent
no-op (lesson 94).

manifest: {
  "cwd": <repo root>,
  "cargo": ["cargo", "test", "-p", ..., "--", <filters>...],
  "record": <results path under cwd; default perturb-results.json in
             the backup directory>,
  "cases": [{"name": ..., "file": <cwd-relative>, "old": ..., "new": ...,
             "expect": [<test names>]}, ...]
}

`expect` is optional but is what makes a `caught` verdict mean "the
test that should discriminate this did". Without it `caught` means
"some test failed", which a bench-harness probe tripping on a zero
reading satisfies; a run with any `expect`-less case prints a note, and
`failed` must be read for those.

The failure parser reads `cargo test` output only: the `test <name>
... FAILED` lines and the `test result:` summary of each target. A
`cargo nextest` argv prints neither, so every red run under it is
`HARNESS-ERROR` — loud, but not a result.

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
import json
import pathlib
import re
import subprocess
import sys
import tempfile

BUILD_ERROR_RE = re.compile(r"^error\[|could not compile", re.M)
FAILED_RES = (
    re.compile(r"^---- (\S+) stdout ----", re.M),
    re.compile(r"^test (\S+) \.\.\. FAILED", re.M),
)
TEST_RESULT_RE = re.compile(r"^test result: \w+\. (\d+) passed; (\d+) failed;", re.M)

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
    failed: set[str] = set()
    for rx in FAILED_RES:
        failed.update(rx.findall(output))
    if BUILD_ERROR_RE.search(output):
        return "BUILD-ERROR", failed, "the build failed"
    if (returncode != 0) != bool(failed):
        return "HARNESS-ERROR", failed, f"exit {returncode} disagrees with {len(failed)} parsed failure(s)"
    ran = tests_ran(output)
    if ran is None:
        return "HARNESS-ERROR", failed, "no `test result:` line: not `cargo test` output, or no target ran"
    if ran == 0:
        return "HARNESS-ERROR", failed, "0 tests ran: the filter matched nothing"
    return ("caught" if failed else "UNCAUGHT"), failed, ""


def run_tests(cwd: pathlib.Path, cargo: list[str]) -> tuple[int, str]:
    r = subprocess.run(cargo, cwd=cwd, capture_output=True, text=True, errors="replace")
    return r.returncode, r.stdout + r.stderr


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
    cargo = list(manifest["cargo"])
    argv_problem = cargo_argv_problem(cargo)
    if argv_problem is not None:
        print(f"refusing: the cargo argv can run several test targets without --no-fail-fast: {argv_problem}", file=sys.stderr)
        print(
            "a failing target stops the run and every later target's guards read as dead; "
            "add --no-fail-fast to the manifest's argv — perturb.py never adds it on your behalf",
            file=sys.stderr,
        )
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
                rc, output = run_tests(cwd, cargo)
            finally:
                path.write_bytes(original)
            verdict, failed, detail = classify(rc, output)
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
