#!/usr/bin/env python3
"""worktree-setup

Idempotently check out the integration corpora under
``tests/repositories/`` for a fresh clone or a fresh ``git worktree``.

Driven by ``make worktree-setup``, which runs this script and then the
Python-bindings venv bootstrap. This half owns only the submodules.

Why a script rather than a Makefile recipe
------------------------------------------

Recovering an *interrupted* ``git submodule update`` needs ``--force``,
and ``--force`` is destructive: it is ``git checkout --force`` inside the
submodule, so it overwrites locally modified tracked files. The
integration-snapshot submodule is one contributors legitimately edit
(accepting ``.snap`` files, per AGENTS.md), so the escalation has to be
conditional on what the submodule actually looks like. That
classification is what this script exists for.

The state that motivates it (#1171): an interrupted
``git submodule update --init`` leaves the submodule's ``.git`` in place,
its HEAD already at the recorded SHA, and its working tree missing some
or all of its files. Because the SHA matches, a plain re-run is a
**silent no-op** — it exits 0 and restores nothing. Only ``--force``
repairs it.

Classification per submodule
----------------------------

``READY``       working tree complete and at the recorded SHA -> skip.
``MISSING``     never initialized (no ``.git``) -> plain ``--init``.
``STALE``       complete, but not at the recorded SHA -> plain ``--init``.
``INCOMPLETE``  tracked files deleted and nothing modified -> ``--force``.
``BLOCKED``     tracked files deleted *and* others modified -> refuse and
                print the command, because forcing would discard the
                modifications.

Deliberately non-recursive
--------------------------

``DeepSpeech`` carries its own submodules (``tensorflow`` at 246 MB,
``kenlm``, ``doc/examples``). The corpus test excludes
``**/DeepSpeech/tensorflow/**`` and ``**/DeepSpeech/kenlm/**``, so no
test reads them — and fetching them is most of the wall time that gets
interrupted in the first place. The out-of-band benchmark harness
(``make bench-*``) does walk them; see
``docs/development/benchmarking.md`` for its recursive checkout.
"""

from __future__ import annotations

import enum
import pathlib
import subprocess
import sys

# `parents[1]`, not `parent`: this script lives in `utils/` but every
# path it touches is anchored at the repository root. Resolving from
# `__file__` rather than the cwd is what keeps a `--force` from ever
# being aimed at whatever directory the caller happened to be in.
REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]

GITMODULES = REPO_ROOT / ".gitmodules"

# Why the by-hand recovery needs `--force`. Named once so the message
# this script prints and the one the test suite prints stay in step.
FORCE_RATIONALE = (
    "the --force is load-bearing: after an interrupted checkout the "
    "submodule HEAD already matches the recorded SHA, so a plain re-run "
    "is a silent no-op"
)


class State(enum.Enum):
    """How a submodule's working tree compares to what git recorded."""

    READY = "ok"
    MISSING = "not initialized"
    STALE = "at the wrong revision"
    INCOMPLETE = "incomplete checkout"
    BLOCKED = "incomplete checkout with local modifications"


def git(args: list[str], cwd: pathlib.Path, on_failure: str | None = None) -> str:
    """Run git in `cwd`, returning stdout.

    Raises `SystemExit` carrying git's own stderr on a non-zero exit, and
    on a missing git binary: a traceback would bury the one line that
    says what went wrong. `on_failure` replaces the generic preamble
    where the caller can name the actual diagnosis — this runs with
    ``check=False``, so a caller cannot catch `CalledProcessError` to
    supply one itself.
    """
    try:
        result = subprocess.run(
            ["git", *args],
            cwd=cwd,
            check=False,
            capture_output=True,
            text=True,
        )
    except FileNotFoundError as exc:
        raise SystemExit("git is not on PATH; refusing to run") from exc
    if result.returncode != 0:
        preamble = on_failure or f"git {' '.join(args)} failed in {cwd}"
        raise SystemExit(f"{preamble}:\n{result.stderr.strip()}")
    return result.stdout


def submodule_paths(root: pathlib.Path) -> list[str]:
    """Repository-relative paths of every submodule in `.gitmodules`.

    Read through `git config` rather than a hand-rolled INI parse so the
    quoting and section-name rules are git's own.
    """
    out = git(
        ["config", "-f", str(root / ".gitmodules"), "--get-regexp", r"^submodule\..*\.path$"],
        cwd=root,
    )
    paths = [line.split(" ", 1)[1] for line in out.splitlines() if " " in line]
    # These come from a tracked file, but a `--force` is aimed at each of
    # them, so refuse anything that could escape the repository.
    for path in paths:
        if pathlib.PurePosixPath(path).is_absolute() or ".." in pathlib.PurePosixPath(path).parts:
            raise SystemExit(f"refusing to act on submodule path outside the repository: {path}")
    return paths


def recorded_sha(root: pathlib.Path, path: str) -> str | None:
    """The gitlink SHA the superproject's HEAD records for `path`."""
    out = git(["ls-tree", "HEAD", "--", path], cwd=root).split()
    return out[2] if len(out) >= 3 and out[1] == "commit" else None


def classify(root: pathlib.Path, path: str) -> State:
    """Decide what, if anything, `path` needs to become usable."""
    work = root / path
    # A submodule git creates gets its `.git` file before any file
    # content, so `.git` present means "a checkout was started here" —
    # which is exactly the case a plain re-run cannot repair.
    if not (work / ".git").exists():
        return State.MISSING

    diff = git(["diff", "--name-status", "HEAD"], cwd=work).splitlines()
    statuses = {line.split("\t", 1)[0][:1] for line in diff if line}
    if "D" in statuses:
        return State.BLOCKED if statuses - {"D"} else State.INCOMPLETE

    current = git(["rev-parse", "HEAD"], cwd=work).strip()
    return State.READY if current == recorded_sha(root, path) else State.STALE


def update(root: pathlib.Path, path: str, force: bool) -> None:
    """Check out `path` at the recorded SHA, optionally forcing.

    git's own progress output is inherited rather than captured — the
    first checkout of a corpus is slow enough that a silent wait reads
    as a hang, and that wait is what gets interrupted (#1171).
    """
    args = ["submodule", "update", "--init"]
    if force:
        args.append("--force")
    result = subprocess.run(["git", *args, "--", path], cwd=root, check=False)
    if result.returncode != 0:
        raise SystemExit(f"checking out {path} failed; see git's output above")


def verify_repo_root() -> None:
    """Refuse to run anywhere but this repository's own checkout."""
    if not GITMODULES.is_file():
        raise SystemExit(f"no .gitmodules under {REPO_ROOT}; refusing to run")
    toplevel = git(
        ["rev-parse", "--show-toplevel"],
        cwd=REPO_ROOT,
        on_failure=f"{REPO_ROOT} is not a git checkout; refusing to run",
    ).strip()
    if pathlib.Path(toplevel).resolve() != REPO_ROOT:
        raise SystemExit(
            f"this script lives under {REPO_ROOT} but git reports the checkout root as "
            f"{toplevel}; refusing to run"
        )


def main() -> int:
    verify_repo_root()
    print(f"worktree-setup: integration corpora under {REPO_ROOT}")

    blocked: list[str] = []
    fetched = False
    for path in submodule_paths(REPO_ROOT):
        state = classify(REPO_ROOT, path)
        print(f"  {path}: {state.value}")
        if state is State.READY:
            continue
        if state is State.BLOCKED:
            blocked.append(path)
            continue
        update(REPO_ROOT, path, force=state is State.INCOMPLETE)
        fetched = True

    for path in blocked:
        print(
            f"\nERROR: {path} is missing tracked files but also has local "
            f"modifications. Repairing it means `git checkout --force` inside "
            f"the submodule, which would discard them, so this script will "
            f"not. Commit or stash them, then run:\n"
            f"  git submodule update --init --force -- {path}\n"
            f"({FORCE_RATIONALE}.)",
            file=sys.stderr,
        )
    if blocked:
        return 1

    if fetched:
        print(
            "\nNote: nested submodules (DeepSpeech's tensorflow and kenlm) are\n"
            "not fetched — the corpus tests exclude them. `make bench-*` does\n"
            "walk them; see docs/development/benchmarking.md for its checkout."
        )
    else:
        print("  all corpora already checked out; nothing to do")
    return 0


if __name__ == "__main__":
    sys.exit(main())
