#!/usr/bin/env python3
"""check-excluded-manifests

Invariants for the root manifest and for the crates listed in its
``[workspace] exclude`` array — the five vendored ``tree-sitter-*``
grammars and the ``enums`` codegen helper.

Three invariants are checked.

**Every excluded crate must root its own workspace.** ``exclude``
denies workspace membership but does **not** terminate cargo's upward
search for a workspace root. In a git worktree under
``.claude/worktrees/`` that search escapes the worktree and lands on the
main checkout's manifest, where the crate's path is neither a member nor
excluded — so ``cargo metadata`` errors, taking ``cargo fmt --all`` and
every ``make pre-commit`` stage chained behind it with it (#1145).

**Every tree-sitter dependency must use an ``=X.Y.Z`` pin.** A caret
range lets ``cargo update`` move a grammar silently, and lets a
downstream consumer of the published ``bca-tree-sitter-*`` crates
resolve one freely — the accidental bump the pinning rule exists to
prevent (#1151). This covers the root manifest's
``[workspace.dependencies]`` block as well as each excluded crate's own
tables; member crates take their grammars through ``workspace = true``
and so carry no requirement of their own. Non-grammar dependencies
(``cc``, ``clap``, ``askama``) are out of scope; the rule is about
grammars.

**Every excluded crate must declare its own ``[lints]``, or be listed
as exempt.** ``[workspace.lints]`` reaches members through
``lints.workspace = true``; an excluded crate is not a member, so it
silently gets rustc's and clippy's defaults instead. ``enums`` was
gated by ``make enums-check`` at ``-D warnings`` for two years against
a lint set 38 findings smaller than every shipping crate's, which reads
as "fully linted" and is not (#1228). The five vendored
``tree-sitter-*`` grammars are exempt by decision, recorded in
:data:`LINTS_EXEMPT_CRATES`; the point of checking the rest is that the
*next* crate added to ``exclude`` forces the same decision instead of
inheriting silence.

Manifests are parsed with ``tomllib`` rather than matched with regexes.
The regex version of this gate silently skipped TOML literal strings
(``tree-sitter-cpp = '0.23.4'`` read as no dependency at all), reported
a commented-out ``exclude`` entry as a crate, and counted the text
``[workspace]`` inside a multi-line string as a workspace table.

See AGENTS.md "Tree-sitter grammars" for the pinning rule and the
`#1145` issue for the worktree traversal this guards.
"""

from __future__ import annotations

import pathlib
import re
import sys
from typing import Any, Iterator

# tomllib landed in 3.11. On older Python, fall back to the external
# `tomli` package (same API), matching check-grammar-marker-sync.py.
try:
    import tomllib
except ImportError:
    try:
        import tomli as tomllib  # type: ignore[import-not-found,no-redef]
    except ImportError:
        sys.stderr.write(
            "error: check-excluded-manifests.py requires Python 3.11+\n"
            "       (tomllib lives in the standard library starting at 3.11).\n"
            "       On older Python, install `tomli` and retry:\n"
            "           pip install tomli\n"
        )
        sys.exit(2)

# `parents[1]`, not `parent`: this gate lives in `utils/` but every path
# it reads is anchored at the repository root.
REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
ROOT_MANIFEST = REPO_ROOT / "Cargo.toml"
ROOT_MANIFEST_LABEL = "Cargo.toml"

# Excluded entries that name a directory rather than a crate. These have
# no manifest of their own and are exempt from every check below.
NON_CRATE_EXCLUDES = frozenset({".claude/worktrees"})

# A dependency whose name contains this substring is a grammar (or the
# `tree-sitter` runtime) and falls under the pinning rule. Matching a
# substring rather than a prefix is load-bearing: `dekobon-tree-sitter-
# groovy` is a real entry in both the root and `enums` manifests, and a
# `^tree-sitter` anchor left it — and any future `bca-tree-sitter-*` —
# unchecked.
GRAMMAR_DEP_SUBSTRING = "tree-sitter"

# Excluded crates deliberately exempt from carrying a `[lints]` table.
#
# The five vendored grammar forks hold ~60 lines of Rust each, and none
# of it is ours: `bindings/rust/lib.rs` and `build.rs` come out of
# `tree-sitter generate` from the upstream binding templates, and are
# replaced wholesale when a grammar is regenerated. Applying the
# workspace's `pedantic` + `missing_docs` posture there would buy
# annotations on generated code that the next regeneration discards,
# against a body of code with no branching to get wrong. Nothing lints
# them today either — they are path dependencies rather than members, and
# clippy does not lint dependencies — so this exemption records a
# decision rather than describing a gate that exists (#1228).
LINTS_EXEMPT_CRATES = frozenset(
    {
        "tree-sitter-ccomment",
        "tree-sitter-mozcpp",
        "tree-sitter-mozjs",
        "tree-sitter-preproc",
        "tree-sitter-tcl",
    }
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
#
# The `tree-sitter` runtime itself is deliberately NOT exempt, though
# the "not a grammar" half of the rationale fits it too. The exemption
# is about unification pressure, and the runtime has none: the lockfile
# shows 25 crates depending on `tree-sitter-language` but only one
# external crate (`tree-sitter-perl`) depending on `tree-sitter`, and
# every manifest here already pins it at `=0.26.13` with the workspace
# resolving. The runtime's ABI version is also what a grammar's
# generated `parser.c` is built against, so an accidental bump is
# exactly the drift this gate exists to catch.
PIN_EXEMPT_DEPS = frozenset({"tree-sitter-language"})

# Cargo dependency-table names. A key is only read as a dependency
# inside one of these, so a same-named key under `[package]`,
# `[features]`, or `[package.metadata.cargo-udeps.ignore]` cannot
# masquerade as one. They may sit at the manifest root, under
# `[workspace]`, or under `[target.'cfg(...)']`.
_DEP_TABLE_NAMES = frozenset({"dependencies", "build-dependencies", "dev-dependencies"})

# How deep the recursive dependency-table walk goes. Cargo's deepest
# standard form is `[target.'cfg(...)'.build-dependencies]` (3 levels);
# 6 gives headroom without admitting pathological recursion. Mirrors
# check-grammar-marker-sync.py's `_DEP_SCAN_MAX_DEPTH`.
_DEP_SCAN_MAX_DEPTH = 6

# A bare semver requirement, with no comparator. Shared between the
# "is this an exact pin" test and the suggestion text so the two cannot
# disagree about what a bare version looks like.
_BARE_VERSION = r"\d+(?:\.\d+)*(?:[-+][0-9A-Za-z.+-]+)?"
BARE_VERSION_RE = re.compile(rf"^{_BARE_VERSION}$")
# `= 0.25.0` with a space is what cargo accepts and what
# check-grammar-marker-sync.py's `.removeprefix("=").strip()` reduces to
# the bare form. A compound requirement (`=0.25.0, <0.26`) is
# deliberately NOT a pin: it is not a version the grammar-marker
# baseline can name, and admitting it here would make the two gates
# disagree about the same manifest line.
EXACT_PIN_RE = re.compile(rf"^=\s*{_BARE_VERSION}$")


def parse_manifest(manifest_text: str, label: str) -> dict[str, Any]:
    """Parse a Cargo manifest, exiting with a located error on bad TOML."""
    try:
        return tomllib.loads(manifest_text)
    except tomllib.TOMLDecodeError as exc:
        raise SystemExit(f"error: {label} is not valid TOML: {exc}") from exc


def read_excluded_crates(manifest_text: str) -> list[str]:
    """Return the crate paths in the root manifest's ``exclude`` array.

    Directory entries listed in :data:`NON_CRATE_EXCLUDES` are dropped —
    they gate cargo's search but carry no manifest to check.
    """
    data = parse_manifest(manifest_text, ROOT_MANIFEST_LABEL)
    workspace = data.get("workspace")
    exclude = workspace.get("exclude") if isinstance(workspace, dict) else None
    if not isinstance(exclude, list):
        raise SystemExit(
            "error: could not locate the [workspace] exclude array in Cargo.toml"
        )
    for entry in exclude:
        if not isinstance(entry, str):
            raise SystemExit(
                f"error: [workspace] exclude holds a non-string entry: {entry!r}"
            )
    return [entry for entry in exclude if entry not in NON_CRATE_EXCLUDES]


def has_workspace_key(manifest_text: str, label: str = "Cargo.toml") -> bool:
    """True when the manifest carries any top-level ``workspace`` key.

    Cargo terminates its upward workspace search on the *key*, not on
    the bare ``[workspace]`` header: a manifest whose only workspace
    content is ``[workspace.package]``, ``[workspace.dependencies]``, or
    ``[workspace.lints]`` still roots a workspace. Measured on cargo
    1.95.0 — a sub-package with no workspace table under an unrelated
    root fails `cargo metadata` with "current package believes it's in a
    workspace when it's not", and each of those three headers alone
    makes it exit 0.
    """
    return "workspace" in parse_manifest(manifest_text, label)


def missing_workspace_table(crates: list[str], root: pathlib.Path) -> list[str]:
    """Return excluded crates whose manifest roots no workspace."""
    offenders = []
    for crate in crates:
        manifest = root / crate / "Cargo.toml"
        if not manifest.is_file():
            raise SystemExit(f"error: excluded crate has no manifest: {manifest}")
        text = manifest.read_text(encoding="utf-8")
        if not has_workspace_key(text, f"{crate}/Cargo.toml"):
            offenders.append(crate)
    return offenders


def workspace_lints(manifest: dict[str, Any]) -> dict[str, Any] | None:
    """Return a manifest's own ``[workspace.lints]``, or ``None``.

    An excluded crate roots its *own* workspace — every one here carries a
    ``[workspace]`` table — so ``[lints] workspace = true`` resolves
    against this same file rather than the repository root. Measured on
    cargo 1.95: a self-rooted crate spelling both tables builds and emits
    the inherited lints, so that shape is a real lint posture and not the
    copied-in mistake it resembles.

    An *empty* ``[workspace.lints]`` is returned rather than rejected
    here, so the caller's vacuity rule names it as declaring no levels —
    which is what it does. Reporting it as inheriting nothing would be
    the less accurate of the two: the inheritance resolves fine.
    """
    workspace = manifest.get("workspace")
    if not isinstance(workspace, dict):
        return None
    lints = workspace.get("lints")
    return lints if isinstance(lints, dict) else None


def lints_table_problem(manifest_text: str, label: str) -> str | None:
    """Describe why ``label``'s ``[lints]`` table is unusable, or ``None``.

    Three ways to fail. An absent or empty table leaves the crate on
    compiler defaults. A ``[lints.rust]`` / ``[lints.clippy]`` sub-table
    with no keys under it is exactly as vacuous — it sets no level, so it
    must not buy a pass either. And ``workspace = true`` with no local
    ``[workspace.lints]`` to resolve against inherits nothing: it is the
    spelling every *member* crate uses, so it looks right, but an
    excluded crate does not reach the repository root's table.

    ``workspace = true`` *with* a populated ``[workspace.lints]`` in the
    same file is legitimate and passes — the effective table is the
    inherited one, and it faces the same vacuity rule.
    """
    manifest = parse_manifest(manifest_text, label)
    lints = manifest.get("lints")
    if not isinstance(lints, dict) or not lints:
        return "no [lints] table"
    if "workspace" in lints:
        if lints["workspace"] is not True:
            # cargo refuses `workspace = false` outright ("`workspace`
            # cannot be false"), so there is nothing to resolve. Quote
            # what the manifest says: reporting `= true` here would send
            # the reader hunting for text that is not there.
            spelled = str(lints["workspace"]).lower()
            return f"[lints] workspace = {spelled}, which cargo rejects"
        inherited = workspace_lints(manifest)
        if inherited is None:
            return "[lints] workspace = true, which inherits nothing here"
        lints = inherited
    if not any(isinstance(table, dict) and table for table in lints.values()):
        return "[lints] declares no lint levels"
    return None


def unlinted_crates(crates: list[str], root: pathlib.Path) -> list[tuple[str, str]]:
    """Return ``(crate, problem)`` for excluded crates declaring no lints."""
    offenders = []
    for crate in crates:
        if crate in LINTS_EXEMPT_CRATES:
            continue
        label = f"{crate}/Cargo.toml"
        text = (root / crate / "Cargo.toml").read_text(encoding="utf-8")
        problem = lints_table_problem(text, label)
        if problem is not None:
            offenders.append((crate, problem))
    return offenders


def _dependency_entries(
    data: Any, depth: int = 0, in_dep_table: bool = False
) -> Iterator[tuple[str, Any]]:
    """Yield ``(name, value)`` for every entry of a dependency table.

    Descends `[workspace]` / `[target]` / `[target.'cfg(...)']` wrappers
    with `in_dep_table` still False, so only entries that really sit in
    a dependency table are yielded. Entries themselves are leaves — an
    inline table's `version` / `path` / `package` fields are never
    re-entered as if they were dependencies.
    """
    if depth > _DEP_SCAN_MAX_DEPTH or not isinstance(data, dict):
        return
    for key, value in data.items():
        if in_dep_table:
            yield key, value
            continue
        if isinstance(value, dict):
            yield from _dependency_entries(
                value, depth + 1, key in _DEP_TABLE_NAMES
            )


def is_exact_pin(requirement: str) -> bool:
    """True when ``requirement`` is an ``=X.Y.Z`` pin and nothing else."""
    return EXACT_PIN_RE.match(requirement.strip()) is not None


def pin_suggestion(requirement: str) -> str:
    """Return the remediation phrase for an unpinned requirement.

    Only a bare version can be repaired by prepending `=`. Saying
    `should be "=^0.23.4"` for a caret range — or `"=*"` for a
    wildcard — hands the reader a requirement cargo rejects.
    """
    if BARE_VERSION_RE.match(requirement.strip()):
        return f'should be "={requirement}"'
    return 'replace the range with an exact "=X.Y.Z" pin'


def unpinned_grammar_deps(
    manifest_text: str, label: str = "Cargo.toml"
) -> list[tuple[str, str]]:
    """Return ``(dependency, requirement)`` pairs not using an ``=`` pin.

    An entry with no requirement of its own — `{ workspace = true }`,
    `{ path = "..." }` — is skipped: the requirement it inherits lives
    in the root manifest, which this gate checks directly.
    """
    unpinned = []
    for name, value in _dependency_entries(parse_manifest(manifest_text, label)):
        if GRAMMAR_DEP_SUBSTRING not in name or name in PIN_EXEMPT_DEPS:
            continue
        if isinstance(value, str):
            requirement = value
        elif isinstance(value, dict) and isinstance(value.get("version"), str):
            requirement = value["version"]
        else:
            continue
        if not is_exact_pin(requirement):
            unpinned.append((name, requirement))
    return unpinned


def _report_missing_workspace(offenders: list[str]) -> None:
    print(
        "error: excluded crates missing a [workspace] table:\n"
        + "\n".join(f"  {crate}/Cargo.toml" for crate in offenders)
        + "\n\nWithout it, cargo's upward workspace search escapes a git\n"
        "worktree under .claude/worktrees/ and resolves against the main\n"
        "checkout, breaking `cargo fmt --all` and `make pre-commit` there\n"
        "(#1145). Append an empty `[workspace]` table to each manifest.",
        file=sys.stderr,
    )


def _report_unlinted(offenders: list[tuple[str, str]]) -> None:
    print(
        "error: excluded crates not declaring their own lint set:\n"
        + "\n".join(f"  {crate}/Cargo.toml: {problem}" for crate, problem in offenders)
        + "\n\n[workspace.lints] in the root Cargo.toml reaches members only.\n"
        "An excluded crate roots its own workspace, so without a [lints]\n"
        "table of its own it builds on compiler defaults while any gate\n"
        "over it still reads as complete (#1228). Copy the root's\n"
        "[lints.rust] / [lints.clippy] tables into the manifest, or add\n"
        "the crate to LINTS_EXEMPT_CRATES with the reason.",
        file=sys.stderr,
    )


def _report_unpinned(drifted: list[tuple[str, str, str]]) -> None:
    print(
        "error: tree-sitter dependencies without an `=X.Y.Z` pin:\n"
        + "\n".join(
            f'  {label}: {dep} = "{requirement}" ({pin_suggestion(requirement)})'
            for label, dep, requirement in drifted
        )
        + "\n\nA caret range lets `cargo update` move a grammar silently and\n"
        "lets a downstream consumer of the published crate resolve one\n"
        "freely (#1151). See AGENTS.md \"Tree-sitter grammars\".",
        file=sys.stderr,
    )


def main() -> int:
    root_text = ROOT_MANIFEST.read_text(encoding="utf-8")
    crates = read_excluded_crates(root_text)

    offenders = missing_workspace_table(crates, REPO_ROOT)
    if offenders:
        _report_missing_workspace(offenders)
        return 1

    # Ordered after the workspace-table check so a crate with no manifest
    # at all is reported by that one's explicit hard error rather than as
    # a read failure here.
    unlinted = unlinted_crates(crates, REPO_ROOT)
    if unlinted:
        _report_unlinted(unlinted)
        return 1

    # The root manifest is checked alongside the excluded crates: its
    # `[workspace.dependencies]` block holds ~20 grammar pins, and
    # AGENTS.md claims this gate enforces them.
    manifests = [(ROOT_MANIFEST_LABEL, root_text)] + [
        (
            f"{crate}/Cargo.toml",
            (REPO_ROOT / crate / "Cargo.toml").read_text(encoding="utf-8"),
        )
        for crate in crates
    ]
    drifted = [
        (label, dep, requirement)
        for label, text in manifests
        for dep, requirement in unpinned_grammar_deps(text, label)
    ]
    if drifted:
        _report_unpinned(drifted)
        return 1

    linted = len(crates) - len(LINTS_EXEMPT_CRATES.intersection(crates))
    print(
        f"Excluded manifests OK ({len(crates)} crates checked, "
        f"{len(manifests)} manifests pin-checked, "
        f"{linted} lint-checked)."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
