#!/usr/bin/env python3
"""check-grammar-marker-sync

Lint that blocks the failure mode described in #400: bumping the
notification-only `tree-sitter-{javascript,cpp}` marker in
`tree-sitter-{mozjs,mozcpp}/Cargo.toml` without re-running
`./generate-grammars/generate-{mozjs,mozcpp}.sh` to refresh the
bundled `src/parser.c` / `src/scanner.c`.

The marker is a Dependabot notification hook only — Cargo never
compiles it. The bundled grammar can therefore lag the marker
indefinitely, and a reader at the marker line is misled into
thinking "marker = X" implies "bundled grammar matches X".

This gate compares the marker line in each vendored crate's
`Cargo.toml` against the corresponding `[mozjs]` / `[mozcpp]`
entry in `.grammar-marker-baseline.toml`. Any divergence fails.

Regenerate the baseline after a verified regen with:

    ./check-grammar-marker-sync.py --update

`--update` rewrites both the `marker` and `version` values of
existing sections in place (or inserts missing fields), preserving
per-section audit comments and ordering. New sections are
appended with a minimal template. Orphan sections (in the baseline
but not in `VENDORED_CRATES`) are not removed but a warning is
emitted so the maintainer can decide.

See AGENTS.md "Validation gates" and #400 for context.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys
from typing import Any, NamedTuple

# tomllib landed in 3.11. On older Python, fall back to the
# external `tomli` package (same API). Without either, fail loud:
# the prior `import tomllib` would have crashed pre-main with an
# opaque `ModuleNotFoundError`.
try:
    import tomllib
except ImportError:
    try:
        import tomli as tomllib  # type: ignore[import-not-found,no-redef]
    except ImportError:
        sys.stderr.write(
            "error: check-grammar-marker-sync.py requires Python 3.11+\n"
            "       (tomllib lives in the standard library starting at 3.11).\n"
            "       On older Python, install `tomli` and retry:\n"
            "           pip install tomli\n"
        )
        sys.exit(2)


REPO_ROOT = pathlib.Path(__file__).resolve().parent
BASELINE_PATH = REPO_ROOT / ".grammar-marker-baseline.toml"


class CrateMarker(NamedTuple):
    """One row in `VENDORED_CRATES` paired with its observed version.

    Built up while iterating `VENDORED_CRATES` in `main`; each field
    has exactly one source of truth and stays positionally stable
    across the script's three downstream consumers (write_baseline,
    drift comparison, success print).
    """

    key: str
    marker: str
    manifest_rel: str
    version: str


# Vendored grammar crate metadata: the baseline section key, the
# marker dependency name, and the relative path to the manifest.
VENDORED_CRATES: tuple[tuple[str, str, str], ...] = (
    ("mozjs", "tree-sitter-javascript", "tree-sitter-mozjs/Cargo.toml"),
    ("mozcpp", "tree-sitter-cpp", "tree-sitter-mozcpp/Cargo.toml"),
)

# Cargo dependency sections to consult. The markers currently live
# under `[build-dependencies]`, but moving them to another standard
# section is a legitimate refactor and should not blind the gate.
_CARGO_DEP_SECTIONS = (
    "build-dependencies",
    "dependencies",
    "dev-dependencies",
)


def read_marker(manifest: pathlib.Path, marker: str) -> str | None:
    """Extract the marker version from a Cargo manifest.

    Uses a real TOML parse so the bare-string form
    (`marker = "X.Y.Z"`), the inline-table form
    (`marker = { version = "...", features = [...] }`), and a
    marker name that legitimately appears inside a TOML multi-line
    string literal (delimited by three double-quotes) all
    resolve correctly.

    Returns None if the manifest is missing, malformed, or does
    not declare the marker in any standard dependency section.
    """
    try:
        text = manifest.read_text(encoding="utf-8")
    except FileNotFoundError:
        return None
    try:
        data: dict[str, Any] = tomllib.loads(text)
    except tomllib.TOMLDecodeError:
        return None
    for section_name in _CARGO_DEP_SECTIONS:
        section = data.get(section_name)
        if not isinstance(section, dict):
            continue
        entry = section.get(marker)
        if isinstance(entry, str):
            return entry
        if isinstance(entry, dict):
            version = entry.get("version")
            if isinstance(version, str):
                return version
    return None


def load_baseline() -> dict[str, dict[str, str]] | None:
    """Parse the baseline file.

    Returns None when the baseline is missing so the caller can
    surface the actionable "run --update to create it" hint. Exits
    with code 2 on malformed TOML — leaking the raw `tomllib`
    traceback to the user is a UX regression compared to the rest
    of the script's error reporting.

    Scalars inside each section are coerced to strings via `str()`
    rather than filtered, so a hand-edited `version = 0.25` (TOML
    float) surfaces as a wrong-value drift rather than as
    "baseline None" / "key missing".
    """
    if not BASELINE_PATH.exists():
        return None
    raw = BASELINE_PATH.read_text(encoding="utf-8")
    try:
        parsed: dict[str, Any] = tomllib.loads(raw)
    except tomllib.TOMLDecodeError as exc:
        sys.stderr.write(
            f"error: {BASELINE_PATH.relative_to(REPO_ROOT)} "
            f"is not valid TOML: {exc}\n"
        )
        sys.exit(2)
    out: dict[str, dict[str, str]] = {}
    for key, value in parsed.items():
        if not isinstance(value, dict):
            continue
        out[key] = {k: str(v) for k, v in value.items()}
    return out


# Section header matchers. Both accept an inline trailing comment
# (`[mozjs] # the JS one`), which is valid TOML.
_NAMED_SECTION_HEADER_RE_CACHE: dict[str, re.Pattern[str]] = {}
_ANY_SECTION_HEADER_RE = re.compile(r"^\[[^\]]+\]\s*(?:#.*)?$")
_TOP_SECTION_HEADER_RE = re.compile(r"^\[([^.\]]+)\]\s*(?:#.*)?$")


def _named_section_header_re(section: str) -> re.Pattern[str]:
    cached = _NAMED_SECTION_HEADER_RE_CACHE.get(section)
    if cached is not None:
        return cached
    compiled = re.compile(rf"^\[{re.escape(section)}\]\s*(?:#.*)?$")
    _NAMED_SECTION_HEADER_RE_CACHE[section] = compiled
    return compiled


# Lines we know how to rewrite in place. We capture the
# pre-quote prefix (`group(1)`) and the post-quote suffix
# (`group(2)`, typically a trailing inline comment) so the
# rewrite preserves surrounding whitespace and comments.
_MARKER_LINE_RE = re.compile(r'^(\s*marker\s*=\s*)"[^"]*"(.*)$')
_VERSION_LINE_RE = re.compile(r'^(\s*version\s*=\s*)"[^"]*"(.*)$')


def _update_section(
    text: str, section: str, marker: str, version: str
) -> str:
    """Update `[section]`'s `marker` and `version` lines in place.

    Behavior:
    * If `[section]` is missing, append a fresh template at end of
      file (the only path that creates a new section).
    * If `[section]` exists, update `marker = "..."` and
      `version = "..."` in place. Missing fields are INSERTED at
      the end of the section body — never as a duplicate section
      header. The body ends at the next `[...]` header line of
      ANY shape (including dotted children such as
      `[mozjs.notes]`), so a hand-added child table doesn't pull
      its `version = "..."` line into `[section]`'s scope.
    """
    lines = text.splitlines(keepends=True)
    named = _named_section_header_re(section)

    sec_idx = -1
    for i, line in enumerate(lines):
        if named.match(line.rstrip("\n")):
            sec_idx = i
            break

    if sec_idx < 0:
        return _append_new_section(text, section, marker, version)

    # Body spans (sec_idx, end_idx]. End at the next [...] header
    # of ANY shape — dotted child tables count as boundaries.
    end_idx = len(lines)
    for i in range(sec_idx + 1, len(lines)):
        if _ANY_SECTION_HEADER_RE.match(lines[i].rstrip("\n")):
            end_idx = i
            break

    marker_seen = False
    version_seen = False
    for i in range(sec_idx + 1, end_idx):
        stripped = lines[i].rstrip("\n")
        m = _MARKER_LINE_RE.match(stripped)
        if m:
            lines[i] = f'{m.group(1)}"{marker}"{m.group(2)}\n'
            marker_seen = True
            continue
        v = _VERSION_LINE_RE.match(stripped)
        if v:
            lines[i] = f'{v.group(1)}"{version}"{v.group(2)}\n'
            version_seen = True

    inserts: list[str] = []
    if not marker_seen:
        inserts.append(f'marker = "{marker}"\n')
    if not version_seen:
        inserts.append(f'version = "{version}"\n')

    if inserts:
        # Insert at the end of the body, before any trailing
        # blank lines so per-section audit comments stay
        # visually adjacent to their section.
        insert_at = end_idx
        while insert_at > sec_idx + 1 and lines[insert_at - 1].strip() == "":
            insert_at -= 1
        lines = lines[:insert_at] + inserts + lines[insert_at:]

    return "".join(lines)


def _append_new_section(
    text: str, section: str, marker: str, version: str
) -> str:
    """Append a minimal `[section]` template at end of `text`."""
    if text and not text.endswith("\n"):
        text += "\n"
    if text and not text.endswith("\n\n"):
        text += "\n"
    return (
        text
        + f"[{section}]\n"
        + f'marker = "{marker}"\n'
        + f'version = "{version}"\n'
    )


# Minimal header for `--update` runs that have no on-disk baseline
# to start from (only fires on a fresh checkout that deleted the
# file). Kept intentionally short so it cannot drift far from the
# canonical on-disk header — a test asserts the live file starts
# with these lines.
_BASELINE_HEADER = """\
# Grammar-marker-sync baseline.
# Records the upstream-grammar crate version that was in effect
# when the vendored sources under tree-sitter-{mozjs,mozcpp}/src/
# were last regenerated. See #400 for context.

"""


def write_baseline(entries: list[CrateMarker]) -> list[str]:
    """Rewrite the baseline in place.

    Returns the list of orphan section keys (present in the
    baseline but absent from `entries`) so `main` can surface a
    non-fatal warning. Orphans are NOT automatically removed —
    silently deleting hand-written content is a footgun.
    """
    if BASELINE_PATH.exists():
        text = BASELINE_PATH.read_text(encoding="utf-8")
    else:
        text = _BASELINE_HEADER

    for entry in entries:
        text = _update_section(text, entry.key, entry.marker, entry.version)

    BASELINE_PATH.write_text(text, encoding="utf-8")

    # Re-parse for orphan detection. Failure here means a section
    # name collision (e.g. dotted child colliding with parent),
    # which is fatal in `tomllib`.
    try:
        parsed: dict[str, Any] = tomllib.loads(text)
    except tomllib.TOMLDecodeError:
        return []
    known = {e.key for e in entries}
    return [k for k in parsed if k not in known and isinstance(parsed[k], dict)]


def _read_current() -> tuple[list[CrateMarker], list[str]]:
    """Walk `VENDORED_CRATES` and read each manifest's marker.

    Returns (entries, missing_messages). Each entry has its
    `manifest_rel` set so downstream failure messages can cite the
    exact manifest without a second lookup.
    """
    current: list[CrateMarker] = []
    missing: list[str] = []
    for crate_key, marker, manifest_rel in VENDORED_CRATES:
        manifest = REPO_ROOT / manifest_rel
        version = read_marker(manifest, marker)
        if version is None:
            missing.append(
                f"{manifest_rel}: no {marker!r} marker line found"
            )
            continue
        current.append(
            CrateMarker(
                key=crate_key,
                marker=marker,
                manifest_rel=manifest_rel,
                version=version,
            )
        )
    return current, missing


def main() -> int:
    # Static description — `__doc__.splitlines()[0]` would crash
    # under `python3 -OO` (docstrings stripped → `__doc__ is None`).
    parser = argparse.ArgumentParser(
        prog="check-grammar-marker-sync",
        description=(
            "Block grammar-marker bumps in tree-sitter-{mozjs,mozcpp}/"
            "Cargo.toml that aren't accompanied by a matching source "
            "regen. See AGENTS.md and #400 for context."
        ),
    )
    parser.add_argument(
        "--update",
        action="store_true",
        help=(
            "Overwrite the baseline's `marker` and `version` values "
            "from the current Cargo.toml entries. Per-section "
            "comments and ordering are preserved. Run only after "
            "you have re-executed the matching generate-*.sh "
            "script and verified the source diff."
        ),
    )
    args = parser.parse_args()

    current, missing = _read_current()
    if missing:
        sys.stderr.write("grammar-marker-sync: marker line(s) not found\n")
        for line in missing:
            sys.stderr.write(f"  {line}\n")
        return 2

    if args.update:
        orphans = write_baseline(current)
        rel = BASELINE_PATH.relative_to(REPO_ROOT)
        print(f"Baseline updated: {rel}")
        for entry in current:
            print(f"  [{entry.key}] {entry.marker} = {entry.version}")
        for orphan in orphans:
            sys.stderr.write(
                f"warning: baseline contains [{orphan}] section not in "
                f"VENDORED_CRATES; remove it manually if no longer needed.\n"
            )
        return 0

    baseline = load_baseline()
    if baseline is None:
        sys.stderr.write(
            f"error: baseline file missing: "
            f"{BASELINE_PATH.relative_to(REPO_ROOT)}\n"
            "       run with --update to create it.\n"
        )
        return 2

    # Surface orphans on the gate path too, so a stale section
    # (e.g. left behind after a vendored crate is dropped) gets
    # caught at the next CI run instead of festering.
    known_keys = {e.key for e in current}
    for orphan in baseline:
        if orphan not in known_keys:
            sys.stderr.write(
                f"warning: baseline contains [{orphan}] section not in "
                f"VENDORED_CRATES; remove it manually if no longer needed.\n"
            )

    failures: list[str] = []
    for entry in current:
        section = baseline.get(entry.key)
        if section is None:
            failures.append(
                f"baseline missing [{entry.key}] section "
                f'(expected `marker = "{entry.marker}"`, '
                f'`version = "{entry.version}"`)'
            )
            continue
        baseline_marker = section.get("marker")
        baseline_version = section.get("version")
        if baseline_marker != entry.marker:
            failures.append(
                f"[{entry.key}] marker name drift: "
                f"baseline {baseline_marker!r}, "
                f"Cargo.toml {entry.marker!r}"
            )
            continue
        if baseline_version != entry.version:
            failures.append(
                f"[{entry.key}] {entry.marker}: "
                f"baseline {baseline_version!r}, "
                f"{entry.manifest_rel} {entry.version!r}"
            )

    if failures:
        sys.stderr.write(
            "grammar-marker-sync: marker drifted from baseline\n"
        )
        for line in failures:
            sys.stderr.write(f"  {line}\n")
        sys.stderr.write(
            "\nThis usually means either:\n"
            "  - The marker was bumped without re-running the matching\n"
            "    `./generate-grammars/generate-*.sh` script. Re-run it,\n"
            "    commit the regenerated `src/parser.c` (+ scanner.c /\n"
            "    grammar.json / node-types.json) in the same PR, then\n"
            "    refresh the baseline:\n"
            "        ./check-grammar-marker-sync.py --update\n"
            "  - The sources were regenerated but the baseline was not\n"
            "    refreshed. Run the --update command above.\n"
            "\nSee AGENTS.md and #400 for context.\n"
        )
        return 1

    print("grammar-marker-sync: OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
