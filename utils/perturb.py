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
`HARNESS-ERROR` — loud, but not a result. A `cargo` argv naming more
than one target must carry `--no-fail-fast`, or the targets after the
first failing one never run and their guards read as dead
(`.claude/rules/testing.md`, "So is the invocation").

The refusals and classifications are the acceptance criteria of #1744,
each one a failure the rules file has already paid for:

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
