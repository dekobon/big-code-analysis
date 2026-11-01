#!/usr/bin/env python3
"""Perturbation sweep driver (`.claude/rules/testing.md`, "Verify
defensive refactor tests by reverting the production code").

Applies one exact substitution to one production file, runs a cargo
test command, records which tests fail, and restores the file from the
in-memory copy in a `finally`. A perturbation that fails nothing is a
finding; one whose anchor does not match is an error, never a silent
no-op (lesson 94).

usage: perturb.py <manifest.json>

manifest: {
  "cwd": <repo root>,
  "cargo": ["cargo", "test", "-q", "-p", ..., "--", <filters>...],
  "record": <results path, default "perturb-results.json" under cwd>,
  "cases": [{"name": ..., "file": <cwd-relative>, "old": ..., "new": ...,
             "expect": [<test names>]}, ...]
}

`expect` is optional but is what makes a `caught` verdict mean "the
test that should discriminate this did". Without it `caught` means
"some test failed", which a bench-harness probe tripping on a zero
reading satisfies; a run with any `expect`-less case prints a note, and
`failed` must be read for those.

The refusals and classifications are the acceptance criteria of #1744,
each one a failure the rules file has already paid for:

- Every mutation target must be clean and tracked in git, checked with
  `git status --porcelain` before the first write. A crash between a
  mutation and its restore would otherwise leave the only copy of
  uncommitted work in scratch. Nothing is stashed on the user's behalf.
- The on-disk backup goes to a `mkdtemp` directory, never a fixed path,
  and the directory is printed so an interrupted run can be recovered.
- `old != new` and `old` occurs exactly once, asserted before every
  write; a violation is a `BAD-ANCHOR` verdict with no write.
- A build failure is classified by `error[` / `could not compile`,
  never by a bare `error` — `cargo test` prints `error: test failed` on
  every ordinary red run (#1466).
- The parsed failure set is cross-checked against the exit status; the
  two disagreeing in either direction is `HARNESS-ERROR`, not a result
  (#1238).

The replay inputs for the #1697 sweeps (`perturb-c*.json`) are attached
to that issue as `bca-1697-records.zip`; each needs its `cwd` and
`record` paths pointed at the checkout replaying it.

Self-tested by `utils/perturb-test.py` (`make perturb-test`). Nothing
lints `utils/*.py` today (#1477); the self-test is the only gate.
"""

from __future__ import annotations

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


def dirty_targets(cwd: pathlib.Path, files: list[str]) -> list[str]:
    """The targets `git status --porcelain` reports as modified or
    untracked, as the status lines. A git failure is reported as one line
    so the caller refuses rather than proceeds."""
    if not files:
        return []
    r = subprocess.run(
        ["git", "status", "--porcelain", "--untracked-files=all", "--", *files],
        cwd=cwd,
        capture_output=True,
        text=True,
        errors="replace",
    )
    if r.returncode != 0:
        return [f"git status failed (exit {r.returncode}): {r.stderr.strip()}"]
    return [line for line in r.stdout.splitlines() if line.strip()]


def classify(returncode: int, output: str) -> tuple[str, set[str]]:
    """The verdict for one test run and the tests it reported failing.

    `caught` and `UNCAUGHT` are results; `BUILD-ERROR` and
    `HARNESS-ERROR` describe the harness, and the latter is any
    disagreement between the parsed failure set and the exit status."""
    failed: set[str] = set()
    for rx in FAILED_RES:
        failed.update(rx.findall(output))
    if BUILD_ERROR_RE.search(output):
        return "BUILD-ERROR", failed
    if (returncode != 0) != bool(failed):
        return "HARNESS-ERROR", failed
    return ("caught" if failed else "UNCAUGHT"), failed


def run_tests(cwd: pathlib.Path, cargo: list[str]) -> tuple[int, str]:
    r = subprocess.run(cargo, cwd=cwd, capture_output=True, text=True, errors="replace")
    return r.returncode, r.stdout + r.stderr


def anchor_problem(original: str, old: str, new: str) -> str | None:
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
    dirty = dirty_targets(cwd, files)
    if dirty:
        for line in dirty:
            print(f"refusing: mutation target is dirty or untracked: {line}", file=sys.stderr)
        print(
            "commit or stash the targets first; perturb.py never stashes on your behalf",
            file=sys.stderr,
        )
        return [], 2
    backups: dict[pathlib.Path, str] = {}
    results: list[dict] = []
    rc_all = 0
    no_expect = 0
    try:
        for case in cases:
            path = cwd / case["file"]
            if path not in backups:
                backups[path] = path.read_text()
                # keyed by the repo-relative path, not the basename: two
                # files named `js_family.rs` live under abc/ and roles/
                (backup_dir / case["file"].replace("/", "__")).write_text(backups[path])
            original = backups[path]
            problem = anchor_problem(original, case["old"], case["new"])
            if problem is not None:
                results.append({"name": case["name"], "verdict": "BAD-ANCHOR", "detail": problem})
                print(f"{case['name']}: BAD-ANCHOR {problem}")
                rc_all = 1
                continue
            try:
                path.write_text(original.replace(case["old"], case["new"]))
                rc, output = run_tests(cwd, cargo)
            finally:
                path.write_text(original)
            verdict, failed = classify(rc, output)
            expect = set(case.get("expect", []))
            if not expect:
                no_expect += 1
            missing = sorted(expect - failed)
            results.append(
                {
                    "name": case["name"],
                    "verdict": verdict,
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
            )
    finally:
        for path, text in backups.items():
            if path.read_text() != text:
                path.write_text(text)
                print(f"restored {path} from the in-memory copy", file=sys.stderr)
    if no_expect:
        print(
            f"note: {no_expect} case(s) name no `expect` test; for those, `caught` means "
            "some test failed, not that the intended one did — read `failed` before trusting it"
        )
    return results, rc_all


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(__doc__.split("\n\n", 2)[1], file=sys.stderr)
        return 2
    manifest = json.loads(pathlib.Path(argv[1]).read_text())
    backup_dir = pathlib.Path(tempfile.mkdtemp(prefix="bca-perturb-backup."))
    print(f"on-disk backups: {backup_dir}", file=sys.stderr)
    results, rc = sweep(manifest, backup_dir)
    if results:
        record = pathlib.Path(manifest["cwd"]) / manifest.get("record", "perturb-results.json")
        record.write_text(json.dumps(results, indent=1) + "\n")
    return rc


if __name__ == "__main__":
    sys.exit(main(sys.argv))
