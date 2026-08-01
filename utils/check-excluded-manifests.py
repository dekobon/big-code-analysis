#!/usr/bin/env python3
"""check-excluded-manifests

Invariants for the crates listed in the root manifest's
``[workspace] exclude`` array — the five vendored ``tree-sitter-*``
grammars and the ``enums`` codegen helper.

Every excluded crate must declare its own ``[workspace]`` table.
``exclude`` denies workspace membership but does **not** terminate
cargo's upward search for a workspace root. In a git worktree under
``.claude/worktrees/`` that search escapes the worktree and lands on the
main checkout's manifest, where the crate's path is neither a member nor
excluded — so ``cargo metadata`` errors, taking ``cargo fmt --all`` and
every ``make pre-commit`` stage chained behind it with it (#1145).

See the `#1145` issue for the worktree traversal this guards.
"""

from __future__ import annotations

import pathlib
import re
import sys

# `parents[1]`, not `parent`: this gate lives in `utils/` but every path
# it reads is anchored at the repository root.
REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
ROOT_MANIFEST = REPO_ROOT / "Cargo.toml"

# Excluded entries that name a directory rather than a crate. These have
# no manifest of their own and are exempt from every check below.
NON_CRATE_EXCLUDES = frozenset({".claude/worktrees"})

EXCLUDE_ARRAY_RE = re.compile(r"^exclude\s*=\s*\[(.*?)^\]", re.DOTALL | re.MULTILINE)
QUOTED_ENTRY_RE = re.compile(r'"([^"]+)"')
# A bare `[workspace]` table header, not `[workspace.package]` or
# `[workspace.dependencies]` — only the bare form roots a workspace.
WORKSPACE_TABLE_RE = re.compile(r"^\s*\[workspace\]\s*$", re.MULTILINE)


def read_excluded_crates(manifest_text: str) -> list[str]:
    """Return the crate paths in the root manifest's ``exclude`` array.

    Directory entries listed in :data:`NON_CRATE_EXCLUDES` are dropped —
    they gate cargo's search but carry no manifest to check.
    """
    match = EXCLUDE_ARRAY_RE.search(manifest_text)
    if match is None:
        raise SystemExit(
            "error: could not locate the [workspace] exclude array in Cargo.toml"
        )
    entries = QUOTED_ENTRY_RE.findall(match.group(1))
    return [entry for entry in entries if entry not in NON_CRATE_EXCLUDES]


def missing_workspace_table(crates: list[str], root: pathlib.Path) -> list[str]:
    """Return excluded crates whose manifest lacks a ``[workspace]`` table."""
    offenders = []
    for crate in crates:
        manifest = root / crate / "Cargo.toml"
        if not manifest.is_file():
            raise SystemExit(f"error: excluded crate has no manifest: {manifest}")
        if WORKSPACE_TABLE_RE.search(manifest.read_text(encoding="utf-8")) is None:
            offenders.append(crate)
    return offenders


def main() -> int:
    crates = read_excluded_crates(ROOT_MANIFEST.read_text(encoding="utf-8"))
    offenders = missing_workspace_table(crates, REPO_ROOT)
    if offenders:
        print(
            "error: excluded crates missing a [workspace] table:\n"
            + "\n".join(f"  {crate}/Cargo.toml" for crate in offenders)
            + "\n\nWithout it, cargo's upward workspace search escapes a git\n"
            "worktree under .claude/worktrees/ and resolves against the main\n"
            "checkout, breaking `cargo fmt --all` and `make pre-commit` there\n"
            "(#1145). Append an empty `[workspace]` table to each manifest.",
            file=sys.stderr,
        )
        return 1

    print(f"Excluded manifests OK ({len(crates)} crates checked).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
