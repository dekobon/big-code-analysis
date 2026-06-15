#!/usr/bin/env python3
"""check-versions

Lockstep version-invariant check. Every owned crate in this
repository — workspace members, the workspace-excluded helper
crates (`enums/`, `xtask/`), and the five `bca-tree-sitter-*`
vendored grammar leaves — must share one version number. Every
internal-dep pin must reference that same version.

See `RELEASING.md` "Lockstep version policy" for the policy this
enforces. Wired into `make pre-commit` and the CI lint job.

Exits 0 on lockstep, non-zero with a per-source listing on drift.
"""

from __future__ import annotations

import pathlib
import re
import sys

REPO_ROOT = pathlib.Path(__file__).resolve().parent

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

# Doc files that hard-code the workspace version in install snippets
# or stability prose. Every plain `X.Y.Z` or `= X.Y.Z` match in these
# files must equal the canonical version.
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
# matches, so they need their own file list + pattern (#879). The
# line-665 comment ("pin a published big-code-analysis-cli release")
# confirms these are meant to track the current release, not lag it.
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
CI_PIN_RE = re.compile(
    r"--version\s+(\d+\.\d+(?:\.\d+)?)"
    r"|big-code-analysis-cli@(\d+\.\d+(?:\.\d+)?)"
    r'|BCA_VERSION:\s*"(\d+\.\d+(?:\.\d+)?)"'
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
            if normalize(cited, canonical) != canonical:
                line = read(doc)[: m.start()].count("\n") + 1
                failures.append(
                    f"{doc_path}:{line}: snippet cites version "
                    f"{cited!r}, expected {canonical!r} (or a prefix)"
                )

    for ci_path in CI_RECIPE_FILES:
        text = read(root / ci_path)
        for m in CI_PIN_RE.finditer(text):
            cited = next(g for g in m.groups() if g is not None)
            if normalize(cited, canonical) != canonical:
                line = text[: m.start()].count("\n") + 1
                failures.append(
                    f"{ci_path}:{line}: install snippet pins release "
                    f"{cited!r}, expected {canonical!r} (or a prefix)"
                )

    failures.extend(check_external_grammar_lockstep(root))

    if failures:
        print("lockstep-version check FAILED", file=sys.stderr)
        print(f"canonical workspace version: {canonical}", file=sys.stderr)
        for f in failures:
            print(f"  {f}", file=sys.stderr)
        return 1
    print(f"versions OK: every owned crate at {canonical}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
