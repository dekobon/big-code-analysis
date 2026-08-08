#!/usr/bin/env python3
"""check-publish-metadata

Registry-independent gate on the publish metadata of every crate this
repository uploads to crates.io.

Why it exists
-------------
``cargo publish --dry-run`` is the natural pre-tag gate: it rejects a
missing ``description`` / ``license``, a ``readme`` pointing outside the
package, and an ``include`` whitelist that stopped covering what the
crate needs. For the three top-level crates it cannot run before the
tag, and the workaround that was in place did not run *ever*.

``big-code-analysis`` pins each vendored grammar leaf at
``bca-tree-sitter-<lang> = "=<version>"``, and the Lockstep version
policy (``utils/check-versions.py``) makes that version equal to the one
being released. Packaging resolves the registry requirement, so it fails
on a version that is by definition not yet published::

    $ cargo package -p big-code-analysis --allow-dirty --no-verify
    error: failed to select a version for the requirement
      `bca-tree-sitter-ccomment = "=2.1.1"`
    candidate versions found which didn't match: 2.1.0, 2.0.0, 1.1.0, ...

``big-code-analysis-cli`` and ``big-code-analysis-web`` pin
``big-code-analysis = "=<version>"`` and hit the same wall one level up.
The previous code skipped the parent dry-run whenever that leaf was
absent from the sparse index and called the skip a first-release
bootstrap; because the probed version is always the version being
released, the skip fired on every release and the dry-run branch never
ran before an upload (#1224).

What this checks instead
------------------------
Everything above that does not need the dependency graph resolved:

* **Metadata fields.** ``description``, ``readme``, ``repository``, and
  ``license`` (or ``license-file``) present and non-blank.
* **The ``include`` whitelist**, for a package rooted at the workspace
  root. ``big-code-analysis``'s manifest *is* the workspace manifest, so
  without ``include`` cargo packages the whole repository — notably
  ``tests/repositories/`` — and the upload dies against crates.io's size
  limit with a Varnish ``503 backend write error``.
* **The packaged size**, totalled from ``cargo package --list``.

``cargo package --list`` is the one packaging operation that does *not*
resolve the registry — measured: it exits 0 on this workspace while
``--no-verify`` fails on the unpublished pin — which is what makes a
real, behavioural packaging check available pre-tag.

Running it also gets ``readme`` / ``license-file`` validation for free,
which is why nothing here re-checks them. Measured against cargo 1.95:
a path that does not exist fails ``--list`` outright (``error: readme
`X` does not appear to exist``), and a path *outside* ``include`` is
added to the archive anyway rather than dropped. An earlier revision of
this gate asserted both were present in the listing; neither assertion
could ever have fired.

What it does not check
----------------------
It does not build anything, so it cannot catch a feature-resolution or
compile error in the packaged crate, and it does not validate dependency
version pins (``utils/check-versions.py`` owns those). The five
``bca-tree-sitter-*`` leaves are deliberately out of scope: they carry no
internal pins, so ``release-check`` dry-runs them for real, which is
strictly stronger than this.

Fields are read from ``cargo metadata``, never from the manifest text,
because ``[workspace.package]`` inheritance is real here — the root
manifest spells ``license.workspace = true`` and
``repository.workspace = true``, which a raw ``tomllib`` read would
report as missing. ``include`` is the exception: ``cargo metadata`` does
not emit it, so it is read from the manifest with inheritance resolved
by hand.

See `#1224`, ``RELEASING.md``, and AGENTS.md "Validation gates".
"""

from __future__ import annotations

import json
import pathlib
import subprocess
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
            "error: check-publish-metadata.py requires Python 3.11+\n"
            "       (tomllib lives in the standard library starting at 3.11).\n"
            "       On older Python, install `tomli` and retry:\n"
            "           pip install tomli\n"
        )
        sys.exit(2)

# `parents[1]`, not `parent`: this gate lives in `utils/` but every path
# it reads is anchored at the repository root.
REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]

# Fields crates.io requires or renders on the crate's landing page. Each
# maps to a `cargo metadata` package key.
REQUIRED_FIELDS = ("description", "readme", "repository")

# `license` and `license-file` are alternatives, not both-required:
# crates.io accepts either. Spelled as cargo metadata emits them.
LICENSE_FIELDS = ("license", "license_file")

# Entries `cargo package` synthesises into the archive. They have no
# counterpart on disk, so they are the only listing rows allowed to be
# absent; anything else missing means the listing is being measured
# against the wrong directory.
GENERATED_ENTRIES = frozenset({".cargo_vcs_info.json", "Cargo.lock", "Cargo.toml.orig"})

# crates.io rejects a `.crate` above 10 MiB. The uploaded tarball is
# gzipped, and this repository's parent crate measures 6.3 MiB of source
# for a 1.4 MiB archive (~4.4x), so the ceiling is worth roughly 44 MiB
# of source here. 32 MiB is deliberately below that: the gate fires
# before crates.io would, while leaving ~5x headroom over today's
# largest crate so ordinary `src/` growth cannot trip it. The regression
# it exists to catch is not incremental — dropping `include` pulls in
# `tests/repositories/` (30k+ corpus files) and clears this bound by
# more than an order of magnitude.
MAX_PACKAGED_BYTES = 32 * 1024 * 1024


def run_cargo(args: list[str], cwd: pathlib.Path) -> str:
    """Run a cargo subcommand, hard-erroring on a non-zero exit."""
    try:
        completed = subprocess.run(
            ["cargo", *args],
            cwd=cwd,
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError as exc:
        raise SystemExit(f"error: cannot run `cargo {' '.join(args)}`: {exc}") from exc
    if completed.returncode != 0:
        raise SystemExit(
            f"error: `cargo {' '.join(args)}` failed with exit "
            f"{completed.returncode}:\n{completed.stderr.rstrip()}"
        )
    return completed.stdout


def cargo_metadata(root: pathlib.Path) -> dict[str, Any]:
    """Return `cargo metadata` for the workspace, dependencies excluded.

    `--no-deps` keeps this to workspace members and, more importantly,
    keeps it from resolving the dependency graph — the resolution that
    fails on the not-yet-published leaf pins.
    """
    raw = run_cargo(["metadata", "--format-version", "1", "--no-deps"], root)
    try:
        data = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise SystemExit(f"error: `cargo metadata` did not emit JSON: {exc}") from exc
    if not isinstance(data, dict):
        raise SystemExit("error: `cargo metadata` did not emit a JSON object.")
    return data


def package_listing(name: str, package_root: pathlib.Path) -> list[str]:
    """Return the paths `cargo package` would put in the archive.

    `--allow-dirty` is unconditional: the question is what the *working
    tree* would package, and without it cargo refuses outright on any
    uncommitted change, which would make this unusable mid-edit.
    """
    raw = run_cargo(["package", "-p", name, "--allow-dirty", "--list"], package_root)
    return [line.strip() for line in raw.splitlines() if line.strip()]


def publishable_packages(metadata: dict[str, Any]) -> list[dict[str, Any]]:
    """Return the workspace members that `cargo publish` would upload.

    `publish = false` renders as an empty registry list, which is how
    `big-code-analysis-bench` and `big-code-analysis-py` opt out.
    """
    packages = metadata.get("packages")
    if not isinstance(packages, list):
        raise SystemExit("error: `cargo metadata` emitted no `packages` array.")
    return [
        package
        for package in packages
        if isinstance(package, dict) and package.get("publish") != []
    ]


def _blank(value: Any) -> bool:
    """True when a metadata field is absent or carries no text.

    Present-but-empty is the interesting case: `description = ""` parses,
    inherits, and serialises like a real value, and crates.io rejects it
    only at upload time.
    """
    return not isinstance(value, str) or not value.strip()


def check_metadata_fields(package: dict[str, Any]) -> list[str]:
    """Report the crates.io-facing fields this package is missing."""
    problems = [
        f"[package].{field} is missing or empty"
        for field in REQUIRED_FIELDS
        if _blank(package.get(field))
    ]
    if all(_blank(package.get(field)) for field in LICENSE_FIELDS):
        problems.append("[package].license and .license-file are both missing or empty")
    return problems


def load_toml(path: pathlib.Path) -> dict[str, Any]:
    """Parse a manifest, hard-erroring on an unreadable or invalid file."""
    try:
        with path.open("rb") as handle:
            return tomllib.load(handle)
    except OSError as exc:
        raise SystemExit(f"error: cannot read {path}: {exc}") from exc
    except tomllib.TOMLDecodeError as exc:
        raise SystemExit(f"error: {path} is not valid TOML: {exc}") from exc


def resolve_include(manifest: dict[str, Any], workspace_manifest: dict[str, Any]) -> Any:
    """Return a manifest's `[package].include`, resolving inheritance.

    `include` is one of cargo's inheritable keys, so `include.workspace =
    true` is a legitimate spelling that a plain lookup would read as an
    unrecognised value rather than as the workspace's list. Any other
    table shape is a hard error: guessing at it is how a gate comes to
    pass on input it did not understand.
    """
    package = manifest.get("package")
    if not isinstance(package, dict):
        raise SystemExit("error: manifest has no [package] table.")
    value = package.get("include")
    if isinstance(value, dict):
        if value.get("workspace") is not True:
            raise SystemExit(
                f"error: unrecognised [package].include table {value!r}; "
                "expected `include.workspace = true`."
            )
        workspace = workspace_manifest.get("workspace")
        workspace_package = workspace.get("package") if isinstance(workspace, dict) else None
        value = workspace_package.get("include") if isinstance(workspace_package, dict) else None
    return value


def check_include(manifest: dict[str, Any], workspace_manifest: dict[str, Any]) -> list[str]:
    """Report an absent or vacuous `include` whitelist."""
    value = resolve_include(manifest, workspace_manifest)
    if value is None:
        return [
            (
                "[package].include is absent — a package rooted at the workspace "
                "root packages the entire repository without it"
            )
        ]
    if not isinstance(value, list) or not any(
        isinstance(entry, str) and entry.strip() for entry in value
    ):
        return [f"[package].include is empty or malformed: {value!r}"]
    return []


def measure_listing(listing: list[str], package_root: pathlib.Path) -> int:
    """Return the on-disk bytes of a `cargo package --list` output.

    Rows cargo synthesises have no file to size and are skipped. Any
    *other* absent row means the listing is being measured against the
    wrong directory, which would otherwise total zero bytes and pass —
    the silent-pass failure this gate exists to remove.
    """
    total = 0
    for entry in listing:
        path = package_root / entry
        if path.is_file():
            total += path.stat().st_size
        elif entry not in GENERATED_ENTRIES:
            raise SystemExit(
                f"error: `cargo package --list` named {entry!r}, which does not "
                f"exist under {package_root}."
            )
    return total


def check_packaged_size(listing: list[str], package_root: pathlib.Path) -> list[str]:
    """Report a packaged file set that clears the size ceiling."""
    total = measure_listing(listing, package_root)
    if total <= MAX_PACKAGED_BYTES:
        return []
    return [
        f"packaged files total {total / 1024 / 1024:.1f} MiB across "
        f"{len(listing)} entries, over the {MAX_PACKAGED_BYTES // 1024 // 1024} "
        "MiB ceiling — check [package].include"
    ]


def audit(metadata: dict[str, Any]) -> list[str]:
    """Return one finding per problem, over every publishable member.

    Findings are indented and prefixed with the crate they belong to, so
    `main()` can join them straight into its error block. An empty list
    is a pass.
    """
    workspace_root = metadata.get("workspace_root")
    if not isinstance(workspace_root, str):
        raise SystemExit("error: `cargo metadata` emitted no `workspace_root`.")
    root = pathlib.Path(workspace_root)
    workspace_manifest = load_toml(root / "Cargo.toml")

    targets = publishable_packages(metadata)
    if not targets:
        raise SystemExit(
            "error: no publishable workspace members found; this gate would "
            "have checked nothing."
        )

    problems: list[str] = []
    for package in sorted(targets, key=lambda entry: str(entry.get("name"))):
        name = package.get("name")
        manifest_path = package.get("manifest_path")
        if not isinstance(name, str) or not isinstance(manifest_path, str):
            raise SystemExit(f"error: malformed `cargo metadata` package entry: {package!r}")
        package_root = pathlib.Path(manifest_path).parent

        findings = check_metadata_fields(package)
        if package_root == root:
            # A package rooted here *is* the workspace manifest, so the
            # already-parsed copy serves as both sides — `include` may
            # still be spelled as an inheritance from the
            # `[workspace.package]` table in that same file.
            findings += check_include(workspace_manifest, workspace_manifest)
        findings += check_packaged_size(package_listing(name, package_root), package_root)
        problems += [f"  {name}: {finding}" for finding in findings]
    return problems


def main() -> int:
    metadata = cargo_metadata(REPO_ROOT)
    problems = audit(metadata)
    if problems:
        print(
            "error: publish metadata is incomplete or would package the wrong files:\n"
            + "\n".join(problems)
            + "\n\nFix the manifest before tagging: these crates cannot be\n"
            "`cargo publish --dry-run`-ed pre-tag, so this is the only gate\n"
            "standing between a metadata regression and a half-finished\n"
            "upload to crates.io. See RELEASING.md and #1224.",
            file=sys.stderr,
        )
        return 1
    checked = len(publishable_packages(metadata))
    print(f"publish metadata OK ({checked} publishable crates checked).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
