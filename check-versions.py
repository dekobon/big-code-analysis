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

WORKSPACE_VERSION_RE = re.compile(
    r"^\[workspace\.package\][^\[]*?^version\s*=\s*\"([^\"]+)\"",
    re.MULTILINE | re.DOTALL,
)
PACKAGE_VERSION_RE = re.compile(
    r"^\[package\][^\[]*?^version\s*=\s*\"([^\"]+)\"",
    re.MULTILINE | re.DOTALL,
)
INTERNAL_PIN_RE = re.compile(
    r"(?:bca-tree-sitter-\w+|big-code-analysis)\s*="
    r"[^\n}]*\bversion\s*=\s*\"=([^\"]+)\""
)
# Match: `big-code-analysis = "X.Y.Z"`, `bca-tree-sitter-* = "X.Y"`,
# or `big-code-analysis = "= X.Y.Z"` style snippets in doc prose.
DOC_PIN_RE = re.compile(
    r"\b(?:big-code-analysis|bca-tree-sitter-\w+)\s*=\s*"
    r"(?:\{[^}]*\bversion\s*=\s*)?\"=?\s*([^\"]+?)\""
)


def read(path: pathlib.Path) -> str:
    return path.read_text(encoding="utf-8")


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
        for m in INTERNAL_PIN_RE.finditer(read(manifest)):
            pinned = m.group(1)
            if pinned != canonical:
                line = read(manifest)[: m.start()].count("\n") + 1
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
