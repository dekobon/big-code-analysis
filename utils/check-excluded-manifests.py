#!/usr/bin/env python3
"""check-excluded-manifests

Invariants for the crates listed in the root manifest's
``[workspace] exclude`` array — the five vendored ``tree-sitter-*``
grammars and the ``enums`` codegen helper.

Two invariants are checked.

**Every excluded crate must declare its own ``[workspace]`` table.**
``exclude`` denies workspace membership but does **not** terminate
cargo's upward search for a workspace root. In a git worktree under
``.claude/worktrees/`` that search escapes the worktree and lands on the
main checkout's manifest, where the crate's path is neither a member nor
excluded — so ``cargo metadata`` errors, taking ``cargo fmt --all`` and
every ``make pre-commit`` stage chained behind it with it (#1145).

**Every tree-sitter dependency must use an ``=X.Y.Z`` pin.** A caret
range lets ``cargo update`` move a grammar silently, and lets a
downstream consumer of the published ``bca-tree-sitter-*`` crates
resolve one freely — the accidental bump the pinning rule exists to
prevent (#1151). Non-grammar dependencies (``cc``, ``clap``, ``askama``)
are out of scope; the rule is about grammars.

See AGENTS.md "Tree-sitter grammars" for the pinning rule and the
`#1145` issue for the worktree traversal this guards.
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

# A `tree-sitter*` dependency and its version requirement, in either the
# bare-string form (`tree-sitter-cpp = "=0.23.4"`) or the inline-table
# form (`... = { package = "…", version = "=2.1.0" }`).
#
# Whitespace around `=` is optional and inconsistent across these
# manifests — four of the five spell it `tree-sitter-language="0.1.0"`.
# #1151's own table missed exactly those four for that reason, so the
# `\s*` is load-bearing rather than defensive.
GRAMMAR_DEP_RE = re.compile(
    r'^\s*(tree-sitter[\w-]*)\s*=\s*(?:"(?P<bare>[^"]+)"'
    r'|\{[^}]*\bversion\s*=\s*"(?P<table>[^"]+)")',
    re.MULTILINE,
)

# `tree-sitter` dependencies that must NOT carry an `=` pin.
#
# `tree-sitter-language` is the ecosystem's shared `LanguageFn` trait
# shim, not a grammar, so AGENTS.md's pinning rule does not reach it.
# Pinning it breaks resolution in both directions: `tree-sitter-irules
# 0.1.1` requires `^0.1.7` and cargo unifies 0.1.x deps, so `=0.1.0`
# makes this workspace unresolvable; and these crates are published, so
# an `=` pin on a shim every grammar depends on would break downstream
# consumers pairing it with a grammar wanting a newer 0.1.x. Measured
# on #1151, whose table listed it in error.
PIN_EXEMPT_DEPS = frozenset({"tree-sitter-language"})


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


def unpinned_grammar_deps(manifest_text: str) -> list[tuple[str, str]]:
    """Return ``(dependency, requirement)`` pairs not using an ``=`` pin."""
    unpinned = []
    for match in GRAMMAR_DEP_RE.finditer(manifest_text):
        dependency = match.group(1)
        requirement = match.group("bare") or match.group("table")
        if dependency in PIN_EXEMPT_DEPS or requirement.startswith("="):
            continue
        unpinned.append((dependency, requirement))
    return unpinned


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

    drifted = [
        (crate, dep, requirement)
        for crate in crates
        for dep, requirement in unpinned_grammar_deps(
            (REPO_ROOT / crate / "Cargo.toml").read_text(encoding="utf-8")
        )
    ]
    if drifted:
        print(
            "error: tree-sitter dependencies without an `=X.Y.Z` pin:\n"
            + "\n".join(
                f"  {crate}/Cargo.toml: {dep} = \"{requirement}\""
                f' (should be "={requirement}")'
                for crate, dep, requirement in drifted
            )
            + "\n\nA caret range lets `cargo update` move a grammar silently and\n"
            "lets a downstream consumer of the published crate resolve one\n"
            "freely (#1151). See AGENTS.md \"Tree-sitter grammars\".",
            file=sys.stderr,
        )
        return 1

    print(f"Excluded manifests OK ({len(crates)} crates checked).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
