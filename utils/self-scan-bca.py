#!/usr/bin/env python3
"""Run `bca` for the self-scan recipes, choosing which one (#1773).

    utils/self-scan-bca.py check --tier=soft=0.95 --write-baseline

Resolution, first match wins:

1. `BCA_SELF_SCAN_FROM_SOURCE=1` — build from the tree (step 3), whatever
   else is set. The override for a parity check that must compare two
   from-source builds.
2. `BCA_SELF_SCAN_BINARY=<path>` — run that executable, trusted as given.
   CI sets it to the release binary it has just installed from HEAD.
3. A `bca` on PATH whose `--version` reports a `build inputs:` digest
   equal to the digest of this checkout's inputs, recomputed here with
   `self_scan_digest.py`. Equal means the binary was compiled from these
   exact bytes — committed or not, whatever its mtime or branch — so it
   is used in place of a build. A missing line (a `bca` from before the
   field existed), `unavailable` (a crates.io build), a mismatch or a
   failing `--version` all fall through.
4. `cargo run --quiet [--release] -p big-code-analysis-cli --`, under the
   profile `BCA_SELF_SCAN_PROFILE` names (`dev` by default; anything
   other than `dev` or `release` is an error).

One line on stderr names what was chosen and why; a captured gate log
then says what measured the tree. On POSIX the chosen command replaces
this process (`os.execvp`), so signals, exit status and stdin — for a
`--paths-from -` — are the child's own; on Windows it runs as a child
and its status is relayed, a signal death mapped to `128 + N`.

What this does not do: hash the executable, or verify its self-report.
A binary that prints a digest it was not built from is trusted exactly
as the `cargo install`ed one in CI is. And a disagreement between the
two digest implementations only ever costs the fast path — the resolver
falls back to building — never a wrong binary.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

UTILS_DIR = Path(__file__).resolve().parent
REPO_ROOT = UTILS_DIR.parent
sys.path.insert(0, str(UTILS_DIR))

import self_scan_digest  # noqa: E402

PROFILES = ("dev", "release")
CARGO_RUN = ("cargo", "run", "--quiet")
INPUTS_PREFIX = "build inputs: "


def env(name: str) -> str | None:
    """The variable's value, with an empty string reading as unset."""
    value = os.environ.get(name, "")
    return value if value != "" else None


def truthy(name: str) -> bool:
    return (env(name) or "").lower() in {"1", "true", "yes"}


def note(message: str) -> None:
    print(f"bca: self-scan {message}", file=sys.stderr, flush=True)


def build_inputs_of(binary: str) -> str | None:
    """The `build inputs:` digest `binary --version` reports, or `None`."""
    try:
        out = subprocess.run(
            [binary, "--version"], capture_output=True, text=True, check=False, timeout=30
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if out.returncode != 0:
        return None
    for line in out.stdout.splitlines():
        if line.startswith(INPUTS_PREFIX):
            value = line[len(INPUTS_PREFIX) :].strip()
            return value if value and value != "unavailable" else None
    return None


def cargo_command(profile: str) -> list[str]:
    flag = ["--release"] if profile == "release" else []
    return [*CARGO_RUN, *flag, "-p", "big-code-analysis-cli", "--"]


def resolve(workspace: Path) -> tuple[list[str], str]:
    """The command to run — without the user's arguments — and the reason."""
    profile = env("BCA_SELF_SCAN_PROFILE") or "dev"
    if profile not in PROFILES:
        raise SystemExit(
            f"bca: self-scan: BCA_SELF_SCAN_PROFILE must be one of {PROFILES}, not {profile!r}"
        )

    if truthy("BCA_SELF_SCAN_FROM_SOURCE"):
        return cargo_command(
            profile
        ), f"via cargo run ({profile} profile): BCA_SELF_SCAN_FROM_SOURCE is set"

    explicit = env("BCA_SELF_SCAN_BINARY")
    if explicit is not None:
        # A relative path with a directory part is anchored to the
        # invoking directory now, before `main` changes to the repo root,
        # so it means what the caller's shell meant; a bare name is left
        # for PATH lookup.
        path = Path(explicit)
        resolved = (
            str(path.resolve()) if path.parent != Path() and not path.is_absolute() else explicit
        )
        return [resolved], f"via {resolved}: BCA_SELF_SCAN_BINARY is set"

    on_path = shutil.which("bca")
    if on_path is not None:
        reported = build_inputs_of(on_path)
        if reported is None:
            reason = f"{on_path} reports no build-inputs digest"
        else:
            try:
                live = self_scan_digest.digest(workspace)
            except self_scan_digest.DigestError as e:
                live, reason = None, f"the tree cannot be digested ({e})"
            else:
                reason = f"{on_path} was built from other inputs ({reported[:12]} vs {live[:12]})"
            if live is not None and live == reported:
                return [on_path], f"via {on_path}: its build inputs match this tree ({live[:12]})"
        return cargo_command(profile), f"via cargo run ({profile} profile): {reason}"

    return cargo_command(profile), f"via cargo run ({profile} profile): no bca on PATH"


def main(argv: list[str]) -> int:
    # `BCA_SELF_SCAN_WORKSPACE` is the self-test's seam: it points the
    # digest and the `chdir` at a scratch tree. Nothing else sets it.
    workspace = Path(env("BCA_SELF_SCAN_WORKSPACE") or REPO_ROOT)
    command, reason = resolve(workspace)
    note(reason)
    full = [*command, *argv]
    if env("BCA_SELF_SCAN_DRY_RUN") is not None:
        print("\n".join(full))
        return 0
    os.chdir(workspace)
    if os.name == "posix":
        os.execvp(full[0], full)
    completed = subprocess.run(full, check=False)
    code = completed.returncode
    return 128 - code if code < 0 else code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
