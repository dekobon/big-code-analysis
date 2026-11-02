#!/usr/bin/env python3
"""The build-inputs digest, as `big-code-analysis-cli/src/build_info/digest.rs` defines it.

`utils/self-scan-bca.py` recomputes the digest over the live tree and
compares it with the `build inputs:` line of an installed `bca --version`
to decide whether that binary implements the checkout (#1773). The two
implementations must therefore agree byte-for-byte; the Rust module's
docs state the algorithm once, and `self-scan-bca-test.py` and the Rust
tests hash one shared fixture to one pinned hex.

A rule drift here cannot certify a wrong binary — it makes the resolver
fall back to building from source — but it silently gives the fast path
up, so the pinned fixture is the thing that keeps this file honest.
"""

from __future__ import annotations

import hashlib
import os
from pathlib import Path

# Mirrors `INPUT_ROOTS`, `INPUT_EXTENSIONS` and `SKIPPED_TOP_LEVEL` in
# `digest.rs`; the shared fixture test fails if either side moves alone.
INPUT_ROOTS = (
    "Cargo.toml",
    "Cargo.lock",
    "src",
    "big-code-analysis-ast",
    "big-code-analysis-cli",
    "tree-sitter-ccomment",
    "tree-sitter-mozcpp",
    "tree-sitter-mozjs",
    "tree-sitter-preproc",
    "tree-sitter-tcl",
)
INPUT_EXTENSIONS = frozenset({"rs", "c", "cc", "h", "toml"})
SKIPPED_TOP_LEVEL = frozenset({"tests", "benches", "examples", "target", "build", "node_modules"})


class DigestError(Exception):
    """The tree cannot be digested: a missing root, a symlink, or a non-UTF-8 name."""


def input_files(workspace: Path) -> list[str]:
    """Sorted, `/`-separated, workspace-relative paths of every input file."""
    files: list[str] = []
    for root in INPUT_ROOTS:
        path = workspace / root
        if path.is_symlink():
            raise DigestError(f"symlink under an input root: {root}")
        if path.is_file():
            files.append(root)
        elif path.is_dir():
            # `src/` is the root crate's source directory, not a crate root.
            _collect_dir(workspace, path, root != "src", files)
        else:
            raise DigestError(f"input root is absent: {root}")
    # Bytes rather than code points to match the Rust side literally;
    # UTF-8 preserves code-point order, so the two never differ for a
    # name both sides accept — this spelling only makes the agreement
    # visible.
    files.sort(key=lambda s: s.encode("utf-8"))
    return files


def _collect_dir(workspace: Path, directory: Path, crate_root: bool, out: list[str]) -> None:
    with os.scandir(directory) as it:
        entries = sorted(it, key=lambda e: e.name)
    for entry in entries:
        path = Path(entry.path)
        rel = _relative_posix(workspace, path)
        if entry.is_symlink():
            raise DigestError(f"symlink under an input root: {rel}")
        if entry.is_dir(follow_symlinks=False):
            if crate_root and entry.name in SKIPPED_TOP_LEVEL:
                continue
            _collect_dir(workspace, path, False, out)
        elif entry.is_file(follow_symlinks=False) and _has_input_extension(entry.name):
            out.append(rel)


def _has_input_extension(name: str) -> bool:
    # A pure dotfile (`.toml`) has no extension to Rust's `Path::extension`;
    # the empty stem check keeps this side from hashing what that side skips.
    stem, dot, ext = name.rpartition(".")
    return bool(dot) and bool(stem) and ext in INPUT_EXTENSIONS


def _relative_posix(workspace: Path, path: Path) -> str:
    rel = path.relative_to(workspace)
    try:
        return "/".join(part.encode("utf-8", "strict").decode("utf-8") for part in rel.parts)
    except UnicodeError as e:
        raise DigestError(f"non-UTF-8 path under an input root: {rel!r}") from e


def digest(workspace: Path) -> str:
    """The build-inputs digest of `workspace` as 64 lowercase hex digits."""
    h = hashlib.sha256()
    for rel in input_files(workspace):
        content = (workspace / rel).read_bytes()
        h.update(rel.encode("utf-8"))
        h.update(b"\0")
        h.update(len(content).to_bytes(8, "little"))
        h.update(content)
    return h.hexdigest()


if __name__ == "__main__":
    # `python3 utils/self_scan_digest.py <workspace>` prints the digest;
    # the Rust test `python_digest_agrees_with_the_rust_digest_over_this_checkout`
    # compares it with its own walk of the live tree.
    import sys

    try:
        print(digest(Path(sys.argv[1]) if len(sys.argv) > 1 else Path.cwd()))
    except (DigestError, OSError) as e:
        print(f"error: {e}", file=sys.stderr)
        sys.exit(1)
