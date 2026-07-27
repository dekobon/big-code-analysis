#!/usr/bin/env python3
"""check-versions

Lockstep version-invariant check. Every owned crate in this
repository — workspace members, the workspace-excluded helper
crates (`enums/`, `xtask/`), and the five `bca-tree-sitter-*`
vendored grammar leaves — must share one version number. Every
internal-dep pin must reference that same version.

Documentation pins follow a different clock: readers deploy the
latest *published* release, not the workspace version, which runs
ahead of it between releases. Doc snippets are therefore checked
against the topmost released `## [X.Y.Z] - YYYY-MM-DD` section of
`CHANGELOG.md` (which release-prep moves in the same commit as the
doc pins), and the `recipes/ci.md` install pins may additionally
lag one release because they can only move once the release's
`SHA256SUMS` exists.

See `RELEASING.md` "Lockstep version policy" and "Version strings
in documentation" for the policies this enforces. Wired into
`make pre-commit` and the CI lint job.

Exits 0 on lockstep, non-zero with a per-source listing on drift.
"""

from __future__ import annotations

import pathlib
import re
import sys

# `parents[1]`, not `parent`: these gates live in `utils/` but every
# path they read or write is anchored at the repository root.
REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]

# Owned crates that carry an own `[package].version` line (i.e. do
# not inherit via `version.workspace = true`). Each must match the
# canonical workspace version.
EXCLUDED_LEAF_DIRS = (
    "enums",
    "tree-sitter-ccomment",
    "tree-sitter-mozcpp",
    "tree-sitter-mozjs",
    "tree-sitter-preproc",
    "tree-sitter-tcl",
)

# Lines of the form
#     <key> = { ..., version = "=X.Y.Z", ... }
# in the listed manifests. Every `=X.Y.Z` pin in one of these blocks
# is an internal-crate pin that must equal the canonical version.
INTERNAL_PIN_MANIFESTS = (
    "Cargo.toml",
    "enums/Cargo.toml",
    "big-code-analysis-cli/Cargo.toml",
    "big-code-analysis-web/Cargo.toml",
)

# Doc files that hard-code a version in install snippets or
# stability prose. Every plain `X.Y.Z` or `= X.Y.Z` match in these
# files must equal the **latest published release** (the topmost
# released section of CHANGELOG.md), not the workspace version:
# readers copy these lines, so they must resolve against the
# registries today. See RELEASING.md "Version strings in
# documentation".
DOC_VERSION_FILES = (
    "README.md",
    "STABILITY.md",
    "big-code-analysis-book/src/library/quick-start.md",
    "big-code-analysis-book/src/library/cargo-features.md",
    "big-code-analysis-book/src/library/stability.md",
    "tree-sitter-ccomment/bindings/rust/README.md",
    "tree-sitter-mozcpp/bindings/rust/README.md",
    "tree-sitter-mozjs/bindings/rust/README.md",
    "tree-sitter-preproc/bindings/rust/README.md",
    "tree-sitter-tcl/bindings/rust/README.md",
)

# CI-recipe docs that pin a *published* big-code-analysis-cli release in
# install snippets. These use install-action / binstall / env-var forms
# rather than the `<crate> = "X.Y.Z"` Cargo-snippet shape DOC_PIN_RE
# matches, so they need their own file list + pattern (#879). Because
# the paired BCA_SHA256 values come from the release's SHA256SUMS
# asset, these pins move in a post-publish follow-up commit and may
# therefore cite either the latest published release or the one
# immediately before it (RELEASING.md "Version strings in
# documentation"); anything older is stale.
#
# Deliberately *not* gated here: the `key: bca-…-X.Y.Z` GitHub Actions
# cache key (ci.md:205). It embeds the version too, but a stale cache
# key only causes a cache miss, never a wrong install — so it is left
# out rather than forcing a churn-only bump on every release.
CI_RECIPE_FILES = ("big-code-analysis-book/src/recipes/ci.md",)
# The three install-pin forms used in the CI recipe:
#   * `cargo (b)install … --version X.Y.Z`
#   * `tool: big-code-analysis-cli@X.Y.Z` (taiki-e/install-action)
#   * `BCA_VERSION: "X.Y.Z"` (workflow env var)
# The optional `-<suffix>` tail captures a pre-release version
# (`2.0.0-rc1`) whole, so a pre-release cut's CI pins are checked
# against the canonical pre-release version rather than silently
# matching only the `X.Y.Z` core and tripping on the suffix. The
# bare-string Cargo-snippet form (DOC_PIN_RE) already handled this via
# its `[^"]+?` capture; this keeps the install-action / binstall /
# env-var forms in step.
CI_PIN_RE = re.compile(
    r"--version\s+(\d+\.\d+(?:\.\d+)?(?:-[0-9A-Za-z.]+)?)"
    r"|big-code-analysis-cli@(\d+\.\d+(?:\.\d+)?(?:-[0-9A-Za-z.]+)?)"
    r'|BCA_VERSION:\s*"(\d+\.\d+(?:\.\d+)?(?:-[0-9A-Za-z.]+)?)"'
)

# A released CHANGELOG section header: `## [X.Y.Z] - YYYY-MM-DD`.
# `## [Unreleased]` has no version-date shape and never matches. The
# file is newest-first, so the first match is the latest published
# release.
CHANGELOG_RELEASE_RE = re.compile(
    r"^## \[(\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?)\] - \d{4}-\d{2}-\d{2}",
    re.MULTILINE,
)

WORKSPACE_VERSION_RE = re.compile(
    r"^\[workspace\.package\][^\[]*?^version\s*=\s*\"([^\"]+)\"",
    re.MULTILINE | re.DOTALL,
)
PACKAGE_VERSION_RE = re.compile(
    r"^\[package\][^\[]*?^version\s*=\s*\"([^\"]+)\"",
    re.MULTILINE | re.DOTALL,
)
# An internal-crate pin lives inside an inline dependency table:
#   single-line: `<key> = { ..., version = "=X.Y.Z", ... }`
#   multi-line:  `<key> = {\n  path = "..",\n  version = "=X.Y.Z",\n}`
# `INTERNAL_TABLE_RE` captures the whole `{ … }` table body, anchored
# on the opener `{`; `[^}]*?` scans across newlines *inside* the table
# but stops at its closing `}`, so the body can never bleed past the
# brace into an unrelated later dependency. The body is then probed for
# the `=X.Y.Z` pin and, separately, for whether the table is an
# internal crate (see `_is_internal_table`). Splitting "is this a pin"
# from "is this an internal crate" is what lets us recognise the
# vendored form, where the table KEY is the upstream alias
# (`tree-sitter-ccomment`) and the `bca-tree-sitter-*` name only
# appears in `package = "…"` inside the body (#878). Field order inside
# the table is irrelevant — `version` may precede or follow `package`.
# (Internal crates are always path-deps pinned via the `{ … }` table
# form; the bare-string `<dep> = "X.Y.Z"` snippet form is a doc pin,
# handled separately by DOC_PIN_RE.)
INTERNAL_TABLE_RE = re.compile(r"(?P<key>[\w-]+)\s*=\s*\{(?P<body>[^}]*?)\}")
INTERNAL_VERSION_PIN_RE = re.compile(r"\bversion\s*=\s*\"=([^\"]+)\"")
# An internal crate is identified by the dependency table KEY being
# `big-code-analysis` / `bca-tree-sitter-*`, OR by the table body
# aliasing a `bca-tree-sitter-*` package (the vendored grammar form).
_INTERNAL_KEY_RE = re.compile(r"bca-tree-sitter-[\w-]+|big-code-analysis")
_INTERNAL_PACKAGE_RE = re.compile(r"\bpackage\s*=\s*\"bca-tree-sitter-[\w-]+\"")
# Match: `big-code-analysis = "X.Y.Z"`, `bca-tree-sitter-* = "X.Y"`,
# or `big-code-analysis = "= X.Y.Z"` style snippets in doc prose.
DOC_PIN_RE = re.compile(
    r"\b(?:big-code-analysis|bca-tree-sitter-\w+)\s*=\s*"
    r"(?:\{[^}]*\bversion\s*=\s*)?\"=?\s*([^\"]+?)\""
)

# The external (crates.io) tree-sitter grammar pins are duplicated
# between the root Cargo.toml and enums/Cargo.toml because `enums/` is
# workspace-excluded and cannot inherit [workspace.dependencies] (see
# the lockstep comment in both manifests). Nothing but a hand-edited
# comment kept them aligned — issue #524 caught `tree-sitter-kotlin-ng`
# carrying a loose `"1.1.0"` caret in *both* files. This check diffs
# the two simple-form grammar-pin blocks and fails on any drift
# (a crate present in only one file, or pinned to a different version).
#
# Only the bare-string simple form is matched here:
#     <name> = "=X.Y.Z"
# The vendored `{ package = ..., path = ... }` forks are deliberately
# excluded — those are internal-crate pins already enforced against the
# canonical workspace version by the INTERNAL_TABLE_RE scan above, so
# re-checking them here would be redundant.
EXTERNAL_GRAMMAR_MANIFESTS = (
    "Cargo.toml",
    "enums/Cargo.toml",
)
EXTERNAL_GRAMMAR_PIN_RE = re.compile(
    r"^((?:dekobon-)?tree-sitter[\w-]*)\s*=\s*\"([^\"]+)\"",
    re.MULTILINE,
)


def read(path: pathlib.Path) -> str:
    return path.read_text(encoding="utf-8")


def _is_internal_table(key: str, body: str) -> bool:
    """True if an inline dependency table refers to an owned crate.

    Recognised by the table KEY (`big-code-analysis` / `bca-tree-sitter-*`)
    or, for the vendored grammar forks, by a `package = "bca-tree-sitter-*"`
    alias inside the body — the case the old key-only regex missed (#878).
    """
    return bool(_INTERNAL_KEY_RE.fullmatch(key)) or bool(
        _INTERNAL_PACKAGE_RE.search(body)
    )


def normalize(version: str, canonical: str) -> str:
    """Allow doc snippets to write `1.0` for canonical `1.0.0`."""
    if version == canonical:
        return version
    parts = version.split(".")
    canon_parts = canonical.split(".")
    if len(parts) < len(canon_parts) and canon_parts[: len(parts)] == parts:
        return canonical
    return version


def workspace_version(root: pathlib.Path) -> str:
    text = read(root / "Cargo.toml")
    m = WORKSPACE_VERSION_RE.search(text)
    if not m:
        sys.exit("error: no [workspace.package].version in Cargo.toml")
    return m.group(1)


def released_versions(root: pathlib.Path) -> list[str]:
    """Released versions from CHANGELOG.md section headers, newest first."""
    return CHANGELOG_RELEASE_RE.findall(read(root / "CHANGELOG.md"))


def matches_any(cited: str, allowed: list[str]) -> bool:
    """True if `cited` equals (or is a prefix of) any allowed version."""
    return any(normalize(cited, version) == version for version in allowed)


def package_version(manifest: pathlib.Path) -> str | None:
    m = PACKAGE_VERSION_RE.search(read(manifest))
    return m.group(1) if m else None


def external_grammar_pins(manifest: pathlib.Path) -> dict[str, str]:
    """Map each simple-form external grammar dep to its version string.

    Returns e.g. {"tree-sitter-bash": "=0.25.1", ...}. The bare core
    `tree-sitter` crate is included; vendored `{ package = ... }` forks
    are not (they don't match the simple-string pattern).
    """
    return {name: ver for name, ver in EXTERNAL_GRAMMAR_PIN_RE.findall(read(manifest))}


def check_external_grammar_lockstep(root: pathlib.Path) -> list[str]:
    """Diff the external grammar pin blocks across the listed manifests.

    Uses the first manifest as the reference and reports every crate
    that is missing from, or pinned differently in, any other manifest.
    """
    reference_path, *others = EXTERNAL_GRAMMAR_MANIFESTS
    reference = external_grammar_pins(root / reference_path)
    failures: list[str] = []
    for other_path in others:
        other = external_grammar_pins(root / other_path)
        for name in sorted(set(reference) | set(other)):
            ref_ver = reference.get(name)
            oth_ver = other.get(name)
            if ref_ver != oth_ver:
                failures.append(
                    f"{name}: {reference_path} pins {ref_ver!r} but "
                    f"{other_path} pins {oth_ver!r}"
                )
    return failures


def main() -> int:
    root = REPO_ROOT
    canonical = workspace_version(root)
    released = released_versions(root)
    if not released:
        sys.exit(
            "error: CHANGELOG.md has no released '## [X.Y.Z] - YYYY-MM-DD' section"
        )
    latest_release = released[0]
    # ci.md pins move in a post-publish follow-up (they need the
    # release's SHA256SUMS), so they may lag one release behind.
    ci_allowed = released[:2]
    failures: list[str] = []

    for leaf in EXCLUDED_LEAF_DIRS:
        manifest = root / leaf / "Cargo.toml"
        ver = package_version(manifest)
        if ver is None:
            failures.append(f"{manifest.relative_to(root)}: no [package].version")
        elif ver != canonical:
            failures.append(
                f"{manifest.relative_to(root)}: [package].version = "
                f"{ver!r}, expected {canonical!r}"
            )

    for manifest_path in INTERNAL_PIN_MANIFESTS:
        manifest = root / manifest_path
        text = read(manifest)
        for table in INTERNAL_TABLE_RE.finditer(text):
            body = table.group("body")
            if not _is_internal_table(table.group("key"), body):
                continue
            pin = INTERNAL_VERSION_PIN_RE.search(body)
            if pin is None:
                continue
            pinned = pin.group(1)
            if pinned != canonical:
                line = text[: table.start()].count("\n") + 1
                failures.append(
                    f"{manifest_path}:{line}: internal-dep pin "
                    f"= {pinned!r}, expected {canonical!r}"
                )

    for doc_path in DOC_VERSION_FILES:
        doc = root / doc_path
        for m in DOC_PIN_RE.finditer(read(doc)):
            cited = m.group(1).strip()
            if normalize(cited, latest_release) != latest_release:
                line = read(doc)[: m.start()].count("\n") + 1
                failures.append(
                    f"{doc_path}:{line}: snippet cites version "
                    f"{cited!r}, expected the latest published release "
                    f"{latest_release!r} (or a prefix)"
                )

    for ci_path in CI_RECIPE_FILES:
        text = read(root / ci_path)
        for m in CI_PIN_RE.finditer(text):
            cited = next(g for g in m.groups() if g is not None)
            if not matches_any(cited, ci_allowed):
                line = text[: m.start()].count("\n") + 1
                failures.append(
                    f"{ci_path}:{line}: install snippet pins release "
                    f"{cited!r}, expected one of {ci_allowed!r} (ci.md "
                    f"pins move in the post-publish follow-up and may "
                    f"lag one release)"
                )

    failures.extend(check_external_grammar_lockstep(root))

    if failures:
        print("lockstep-version check FAILED", file=sys.stderr)
        print(f"canonical workspace version: {canonical}", file=sys.stderr)
        for f in failures:
            print(f"  {f}", file=sys.stderr)
        return 1
    print(
        f"versions OK: every owned crate at {canonical}, "
        f"doc pins at published release {latest_release}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
