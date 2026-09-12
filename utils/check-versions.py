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

The lockfiles of the workspace-excluded crates are checked too.
Each excluded crate roots its own workspace and so carries its own
`Cargo.lock`, and `cargo update --workspace` — the refresh
`RELEASING.md` calls **mandatory** during a bump — reaches none of
them. Every gate that consumes one passes `--locked`, so a bump
that misses them strands the recorded path-package versions and the
next otherwise-correct commit fails with "cannot update the lock
file … because --locked was passed" — inside `make enums-check`,
`make fuzz-check`, or `make release-check`, none of which is
plausibly related to whatever that commit touched. Checking the
lockfiles here makes the staleness name itself at the bump, where
the fix is one `cargo update --manifest-path` away (#1234).

See `RELEASING.md` "Lockstep version policy", "Version strings
in documentation", and "Refresh the workspace-excluded lockfiles
too" for the policies this enforces. Wired into `make pre-commit`
and the CI lint job.

Exits 0 on lockstep, non-zero with a per-source listing on drift.
"""

from __future__ import annotations

import pathlib
import re
import sys
from typing import Any

# tomllib landed in 3.11. On older Python, fall back to the external
# `tomli` package (same API), matching check-excluded-manifests.py.
try:
    import tomllib
except ImportError:
    try:
        import tomli as tomllib  # type: ignore[import-not-found,no-redef]
    except ImportError:
        sys.stderr.write(
            "error: check-versions.py requires Python 3.11+\n"
            "       (tomllib lives in the standard library starting at 3.11).\n"
            "       On older Python, install `tomli` and retry:\n"
            "           pip install tomli\n"
        )
        sys.exit(2)

# `parents[1]`, not `parent`: these gates live in `utils/` but every
# path they read or write is anchored at the repository root.
REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]

# Owned crates that carry an own `[package].version` line (i.e. do
# not inherit via `version.workspace = true`). Each must match the
# canonical workspace version.
#
# Deliberately *not* the source of the lockfile-staleness check's
# candidate list: `fuzz` is absent here because it is version `0.0.0`
# by design and so cannot satisfy this tuple's "== canonical" rule,
# yet `fuzz/Cargo.lock` is one of the two lockfiles that motivated
# #1234. The lockfile check derives its own list from the root
# manifest's `[workspace] exclude` array instead — see
# `excluded_lockfiles`.
EXCLUDED_LEAF_DIRS = (
    "enums",
    "tree-sitter-ccomment",
    "tree-sitter-mozcpp",
    "tree-sitter-mozjs",
    "tree-sitter-preproc",
    "tree-sitter-tcl",
)

# `[workspace] exclude` entries that name a directory rather than a
# crate: no manifest, nothing to resolve. Mirrors the same constant in
# check-excluded-manifests.py.
NON_CRATE_EXCLUDES = frozenset({".claude/worktrees"})

# Stand-in for a lockfile entry whose `version` is absent or is not a
# string. It is deliberately not a version any manifest can declare, so
# such an entry is reported rather than skipped.
NO_VERSION = "<no version>"

# Lines of the form
#     <key> = { ..., version = "=X.Y.Z", ... }
# in the listed manifests. Every `=X.Y.Z` pin in one of these blocks
# is an internal-crate pin that must equal the canonical version.
INTERNAL_PIN_MANIFESTS = (
    "Cargo.toml",
    "enums/Cargo.toml",
    "big-code-analysis-ast/Cargo.toml",
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
# `big-code-analysis` / `big-code-analysis-*` / `bca-tree-sitter-*`, OR
# by the table body aliasing a `bca-tree-sitter-*` package (the vendored
# grammar form).
#
# The `-*` suffix group is what makes `big-code-analysis-ast` (#1376)
# match: the pre-#1376 spelling was a bare `big-code-analysis`
# alternative under `fullmatch`, so the root's two `=X.Y.Z` pins on the
# parse layer were silently skipped while the gate still reported
# "versions OK: every owned crate at …". Any future owned
# `big-code-analysis-<something>` is covered by construction.
_INTERNAL_KEY_RE = re.compile(r"bca-tree-sitter-[\w-]+|big-code-analysis(?:-[\w-]+)?")
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


def parse_toml(path: pathlib.Path, label: str) -> dict[str, Any]:
    """Parse a TOML file, exiting with a located error on bad syntax.

    Manifests and lockfiles are parsed rather than regex-matched: a
    literal string (`version = '2.2.1'`), a commented-out entry, and a
    `[[package]]` header inside a multi-line string all read wrongly
    under a pattern and correctly under a parser. Same reasoning as
    check-excluded-manifests.py, which documents the drift each of
    those produced.
    """
    try:
        return tomllib.loads(read(path))
    except tomllib.TOMLDecodeError as exc:
        raise SystemExit(f"error: {label} is not valid TOML: {exc}") from exc


def _workspace_array(root: pathlib.Path, key: str) -> list[str]:
    """Return the root manifest's `[workspace] <key>` string array."""
    data = parse_toml(root / "Cargo.toml", "Cargo.toml")
    workspace = data.get("workspace")
    entries = workspace.get(key) if isinstance(workspace, dict) else None
    if not isinstance(entries, list) or not all(
        isinstance(entry, str) for entry in entries
    ):
        raise SystemExit(
            f"error: could not read a [workspace] {key} string array from Cargo.toml"
        )
    return entries


def owned_crate_dirs(root: pathlib.Path) -> list[str]:
    """Every directory in this repository holding an owned manifest.

    The repository root, the workspace members, and the excluded
    crates — in other words every crate a path dependency can resolve
    to. Derived from the root manifest so the list cannot drift as
    crates are added or removed.
    """
    dirs = ["."]
    dirs += _workspace_array(root, "members")
    dirs += [
        entry
        for entry in _workspace_array(root, "exclude")
        if entry not in NON_CRATE_EXCLUDES
    ]
    for crate in dirs:
        if not (root / crate / "Cargo.toml").is_file():
            raise SystemExit(
                f"error: {crate}/Cargo.toml is listed in the root manifest "
                f"but does not exist (a glob in `members` / `exclude` is not "
                f"supported here)"
            )
    return dirs


def owned_package_versions(
    root: pathlib.Path, canonical: str
) -> dict[str, tuple[str, str]]:
    """Map every owned package name to `(version, declaring manifest)`.

    The package name is not the directory name — `tree-sitter-tcl/`
    publishes as `bca-tree-sitter-tcl` — so the name is read from
    `[package].name` rather than inferred. `version.workspace = true`
    resolves to the canonical workspace version; a manifest with no
    `[package]` table at all (a virtual workspace root) contributes
    nothing and is not an error. The manifest path rides along so a
    failure can point at the file that decides the expected value.
    """
    versions: dict[str, tuple[str, str]] = {}
    for crate in owned_crate_dirs(root):
        label = f"{crate}/Cargo.toml" if crate != "." else "Cargo.toml"
        package = parse_toml(root / crate / "Cargo.toml", label).get("package")
        if not isinstance(package, dict):
            continue
        name = package.get("name")
        declared = package.get("version")
        if isinstance(declared, dict) and declared.get("workspace") is True:
            declared = canonical
        if isinstance(name, str) and isinstance(declared, str):
            versions[name] = (declared, label)
    return versions


def locked_path_packages(lockfile: pathlib.Path, label: str) -> list[tuple[str, str]]:
    """Return `(name, version)` for each path package in a lockfile.

    A `[[package]]` entry with no `source` key was resolved from a
    path, not from a registry — i.e. it is one of this repository's own
    crates, recorded at whatever version its manifest carried when the
    lockfile was last written. Registry entries carry a `source` and
    are legitimately at unrelated versions, so they are skipped.

    A named entry whose `version` is missing or is not a string gets
    the placeholder below rather than being dropped: it can equal no
    manifest version, so the caller reports it instead of quietly
    checking one entry fewer than the file holds.
    """
    packages = parse_toml(lockfile, label).get("package")
    if not isinstance(packages, list):
        return []
    found = []
    for entry in packages:
        if not isinstance(entry, dict) or "source" in entry:
            continue
        name = entry.get("name")
        version = entry.get("version")
        if isinstance(name, str):
            found.append((name, version if isinstance(version, str) else NO_VERSION))
    return found


def excluded_lockfiles(root: pathlib.Path) -> list[tuple[str, pathlib.Path]]:
    """Return `(crate dir, lockfile)` for each excluded crate carrying one.

    An excluded crate need not have a `Cargo.lock` — one only exists
    once something has resolved that crate's manifest — so a missing
    lockfile is skipped rather than reported. A crate with no manifest
    at all is a different matter and is rejected by
    `owned_crate_dirs`.
    """
    return [
        (crate, root / crate / "Cargo.lock")
        for crate in _workspace_array(root, "exclude")
        if crate not in NON_CRATE_EXCLUDES and (root / crate / "Cargo.lock").is_file()
    ]


def check_excluded_lockfiles(root: pathlib.Path, canonical: str) -> list[str]:
    """Report path-package versions stranded by a version bump.

    In an excluded crate's `Cargo.lock`, every path package must sit
    at the version its own manifest declares. `cargo update
    --workspace` does not reach these files, so a bump leaves them
    behind and the failure surfaces later, inside an unrelated commit's
    `--locked` invocation (#1234).
    """
    versions = owned_package_versions(root, canonical)
    failures: list[str] = []
    lockfiles = excluded_lockfiles(root)
    for crate, lockfile in lockfiles:
        label = f"{crate}/Cargo.lock"
        locked = locked_path_packages(lockfile, label)
        if not locked:
            # Every Cargo.lock records at least its own root package
            # with no `source`. None at all means the file is empty or
            # malformed, and checking it proved nothing.
            failures.append(f"{label}: no path packages recorded (empty or malformed)")
            continue
        drifted = []
        for name, found in locked:
            declared = versions.get(name)
            if declared is None:
                # A path package this repository does not own — the
                # version map cannot adjudicate it, and staying silent
                # would mean a lockfile entry nothing checks.
                drifted.append(f"{name} at {found!r} resolves to no manifest here")
                continue
            expected, manifest_label = declared
            if found != expected:
                drifted.append(
                    f"{name} locked at {found!r}, "
                    f"{manifest_label} declares {expected!r}"
                )
        if drifted:
            failures.append(
                f"{label}: stale against the manifests: "
                + "; ".join(drifted)
                + f" — refresh with `cargo update --manifest-path "
                f"{crate}/Cargo.toml --workspace`"
            )
    if not lockfiles:
        # A vacuity guard, not a real-world state: the seven excluded
        # lockfiles are checked in. Reaching zero means the derivation
        # above broke, and a gate that reports "versions OK" having
        # examined nothing is the failure this check exists to prevent.
        failures.append(
            "no workspace-excluded lockfile was found to check — the "
            "[workspace] exclude derivation in check-versions.py has broken"
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
    failures.extend(check_excluded_lockfiles(root, canonical))

    if failures:
        print("lockstep-version check FAILED", file=sys.stderr)
        print(f"canonical workspace version: {canonical}", file=sys.stderr)
        for f in failures:
            print(f"  {f}", file=sys.stderr)
        return 1
    # The lockfile count is printed, not just tallied, so a derivation
    # that silently narrows shows up in the passing output too.
    print(
        f"versions OK: every owned crate at {canonical}, "
        f"doc pins at published release {latest_release}, "
        f"{len(excluded_lockfiles(root))} excluded lockfiles in step"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
