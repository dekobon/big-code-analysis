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
existing sections in place, preserving per-section audit
comments, ordering, and any maintainer-added subsections. New
sections are appended with a minimal template. The baseline
itself is REQUIRED on disk — if it has been deleted (e.g.,
during a merge conflict), restore it from git history rather
than regenerating from --update.

See AGENTS.md "Validation gates" and #400 for context.
"""

from __future__ import annotations

import argparse
import datetime as _dt
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

# `--update` produced invalid TOML for a tree that load_baseline
# itself accepts. Distinct from a user-side file corruption
# (exit 2) — this is a bug in the editor and warrants a different
# signal so it can be filtered in CI dashboards.
_EXIT_EDITOR_BUG = 3


class CrateMarker(NamedTuple):
    """One row in `VENDORED_CRATES` paired with its observed version."""

    key: str
    marker: str
    manifest_rel: str
    version: str


class CargoTomlParseError(Exception):
    """`read_marker` could not parse a manifest.

    Carries the manifest-relative path and the underlying cause so
    main() can produce a structured exit-2 message rather than
    letting tomllib's traceback leak through.
    """


# Vendored grammar crate metadata: the baseline section key, the
# marker dependency name, and the relative path to the manifest.
VENDORED_CRATES: tuple[tuple[str, str, str], ...] = (
    ("mozjs", "tree-sitter-javascript", "tree-sitter-mozjs/Cargo.toml"),
    ("mozcpp", "tree-sitter-cpp", "tree-sitter-mozcpp/Cargo.toml"),
)

# How deep the recursive Cargo dep-tree walk goes. Cargo's
# deepest standard form is `[target.'cfg(...)'.build-dependencies]`
# (3 levels); 6 gives headroom for future workspace / profile
# nesting without admitting pathological recursion.
_DEP_SCAN_MAX_DEPTH = 6


def _scan_for_marker(data: Any, marker: str, depth: int = 0) -> str | None:
    """Recursively walk a TOML tree for `marker` as a dep entry.

    Accepts both the bare-string form (`marker = "X.Y.Z"`) and the
    inline-table form (`marker = { version = "...", features = [...] }`).
    Walks every nested table so `[build-dependencies]`,
    `[dev-dependencies]`, `[workspace.dependencies]`,
    `[target.'cfg(unix)'.build-dependencies]`, and any other
    legitimate Cargo dep-bearing section all resolve.
    """
    if depth > _DEP_SCAN_MAX_DEPTH or not isinstance(data, dict):
        return None
    for key, value in data.items():
        if key == marker:
            if isinstance(value, str):
                return value
            if isinstance(value, dict):
                version = value.get("version")
                if isinstance(version, str):
                    return version
        elif isinstance(value, dict):
            found = _scan_for_marker(value, marker, depth + 1)
            if found is not None:
                return found
    return None


def read_marker(manifest: pathlib.Path, marker: str) -> str | None:
    """Extract the marker version from a Cargo manifest.

    Returns None if the marker is genuinely absent. Raises
    `CargoTomlParseError` if the manifest can't be read or parsed
    — main() catches this and surfaces a structured exit-2
    message rather than leaking a Python traceback.
    """
    try:
        text = manifest.read_text(encoding="utf-8")
    except FileNotFoundError:
        return None
    except OSError as exc:
        raise CargoTomlParseError(f"cannot read {manifest}: {exc}") from exc
    try:
        data: dict[str, Any] = tomllib.loads(text)
    except tomllib.TOMLDecodeError as exc:
        raise CargoTomlParseError(
            f"{manifest} is not valid TOML: {exc}"
        ) from exc
    return _scan_for_marker(data, marker)


def _coerce_baseline_value(
    section_key: str, field_key: str, value: Any
) -> str | None:
    """Convert a parsed baseline value to its string form.

    Strings pass through. Scalars (int / float / bool / TOML
    date) are coerced to their canonical text form so the drift
    message surfaces the actual value the user typed. Composite
    types (lists, sub-tables) return None and emit a warning —
    they can't be a marker name or a version and the user must
    fix the type.
    """
    if isinstance(value, str):
        return value
    if isinstance(value, bool):
        # Test bool BEFORE int — `bool` is a subclass of `int` in
        # Python, so a naked `isinstance(v, int)` would swallow
        # booleans into the scalar branch and lose the warning.
        coerced = "true" if value else "false"
    elif isinstance(value, (int, float)):
        coerced = str(value)
    elif isinstance(value, (_dt.date, _dt.datetime, _dt.time)):
        coerced = value.isoformat()
    else:
        sys.stderr.write(
            f"warning: baseline [{section_key}].{field_key} is "
            f"{type(value).__name__}; expected a quoted string. "
            "Treating as missing — quote the value in the baseline.\n"
        )
        return None
    sys.stderr.write(
        f"warning: baseline [{section_key}].{field_key} = {value!r} "
        f"is {type(value).__name__}; expected a quoted string. "
        "Drift message will show the coerced form.\n"
    )
    return coerced


def load_baseline() -> dict[str, dict[str, str]] | None:
    """Parse the baseline file.

    Returns None when the baseline is missing (caller renders the
    "run --update to create it" hint — but `--update` itself
    refuses this case; see `write_baseline`). Exits with code 2
    on malformed TOML. Non-string scalars trip a warning and are
    coerced so the drift message remains informative; non-scalar
    values (lists, tables) trip a warning and are treated as
    missing.
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
        coerced: dict[str, str] = {}
        for field_key, field_value in value.items():
            text = _coerce_baseline_value(key, field_key, field_value)
            if text is not None:
                coerced[field_key] = text
        out[key] = coerced
    return out


# Section-header detector. Tolerates leading whitespace (valid
# TOML), trailing inline comments, and arbitrarily-spaced
# brackets. Captures the (possibly dotted) section name with
# internal whitespace stripped.
_SECTION_HEADER_RE = re.compile(
    r"^\s*\[\s*([^\]]+?)\s*\]\s*(?:#.*)?$"
)
# Array-of-tables header (`[[name]]`). Treated as a section
# boundary for body-range scans so a hand-maintained
# `[[mozjs.audit]]` subsection doesn't get clobbered.
_AOT_HEADER_RE = re.compile(
    r"^\s*\[\[\s*([^\]]+?)\s*\]\]\s*(?:#.*)?$"
)


def _is_section_boundary(line: str) -> bool:
    stripped = line.rstrip("\n")
    return bool(
        _SECTION_HEADER_RE.match(stripped) or _AOT_HEADER_RE.match(stripped)
    )


def _replace_field_value(
    line: str, field_name: str, new_value: str
) -> str | None:
    """Return `line` with `field_name`'s quoted value replaced.

    Recognizes both TOML string forms: basic (`"..."`) and
    literal (`'...'`). Output is always normalized to the basic
    form, which is the canonical shape on disk. Returns None if
    the line does not match the `<field> = <string>` shape.
    """
    pattern = re.compile(
        rf'^(\s*{re.escape(field_name)}\s*=\s*)'
        rf"(?:\"[^\"]*\"|'[^']*')"
        r"(.*)$"
    )
    body, sep, _ = line.partition("\n")
    m = pattern.match(body)
    if m is None:
        return None
    rewritten = f'{m.group(1)}"{new_value}"{m.group(2)}'
    return rewritten + sep


def _named_section_header(section: str, line: str) -> bool:
    """True if `line` is a `[section]` header for `section`."""
    stripped = line.rstrip("\n")
    m = _SECTION_HEADER_RE.match(stripped)
    return m is not None and m.group(1) == section


def _update_section(
    text: str, section: str, marker: str, version: str
) -> str:
    """Update `[section]`'s `marker` and `version` lines in place.

    Behavior:
    * If `[section]` is missing, append a fresh template at end
      of file (the only path that creates a new section).
    * If `[section]` exists, rewrite the first `marker = "..."`
      and first `version = "..."` line in its body. Missing
      fields are INSERTED at the end of the body. The body ends
      at the next `[...]` or `[[...]]` header line of any shape
      (including dotted child tables and AoT subsections), so
      hand-maintained child content is never pulled into
      `[section]`'s scope. The body insertion path also ensures
      a trailing newline on the prior body line so the inserted
      field doesn't concatenate onto a comment or value.
    """
    lines = text.splitlines(keepends=True)

    sec_idx = -1
    for i, line in enumerate(lines):
        if _named_section_header(section, line):
            sec_idx = i
            break

    if sec_idx < 0:
        return _append_new_section(text, section, marker, version)

    # Body spans (sec_idx, end_idx]. End at the next `[...]` or
    # `[[...]]` header of any shape — dotted child tables and
    # AoT subsections both count as boundaries.
    end_idx = len(lines)
    for i in range(sec_idx + 1, len(lines)):
        if _is_section_boundary(lines[i]):
            end_idx = i
            break

    marker_seen = False
    version_seen = False
    for i in range(sec_idx + 1, end_idx):
        # Break on first match per field so a maintainer-added
        # secondary `version = "..."` annotation in the body is
        # NOT silently clobbered along with the canonical one.
        if not marker_seen:
            new = _replace_field_value(lines[i], "marker", marker)
            if new is not None:
                lines[i] = new
                marker_seen = True
                continue
        if not version_seen:
            new = _replace_field_value(lines[i], "version", version)
            if new is not None:
                lines[i] = new
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
        # Ensure the line immediately preceding the insertion
        # ends with a newline. Without this guard, a section
        # whose last body line lacks `\n` (hand-edit, partial
        # merge) would have the inserted `version = "..."` line
        # concatenated onto the prior text, silently corrupting
        # the TOML (the value or comment swallows the field).
        if insert_at > 0 and lines[insert_at - 1] and not lines[insert_at - 1].endswith("\n"):
            lines[insert_at - 1] = lines[insert_at - 1] + "\n"
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


def write_baseline(entries: list[CrateMarker]) -> list[str]:
    """Rewrite the baseline in place.

    Validates the existing file before editing — duplicate
    sections, malformed TOML, or any other pre-existing issue
    fail loudly with exit 2 rather than silently producing more
    corruption. Validates the post-edit result before writing —
    a `_update_section` bug fails with exit 3 and a "this is an
    editor bug" message rather than shipping a broken file.

    Refuses to operate when the baseline is missing on disk: a
    `--update` from-scratch path would diverge from the canonical
    on-disk header. Restore from git history instead.

    Returns the list of orphan section keys (present in the
    baseline but absent from `entries`) so main() can surface a
    non-fatal warning.
    """
    if not BASELINE_PATH.exists():
        rel = BASELINE_PATH.relative_to(REPO_ROOT)
        sys.stderr.write(
            f"error: {rel} is missing.\n"
            "       --update refuses to regenerate from scratch (a fresh\n"
            "       baseline would lack the per-section audit history\n"
            "       checked into git). Restore the file with:\n"
            f"           git checkout HEAD -- {rel}\n"
            "       then re-run --update.\n"
        )
        sys.exit(2)

    text = BASELINE_PATH.read_text(encoding="utf-8")
    try:
        existing: dict[str, Any] = tomllib.loads(text)
    except tomllib.TOMLDecodeError as exc:
        sys.stderr.write(
            f"error: existing {BASELINE_PATH.relative_to(REPO_ROOT)} "
            f"is not valid TOML: {exc}\n"
            "       Fix the baseline manually before running --update.\n"
        )
        sys.exit(2)

    for entry in entries:
        text = _update_section(text, entry.key, entry.marker, entry.version)

    # Verify the edit. A failure here means `_update_section`
    # produced invalid TOML — the editor has a bug. Refuse to
    # write so the on-disk file remains the (still valid)
    # pre-edit content.
    try:
        new_parsed: dict[str, Any] = tomllib.loads(text)
    except tomllib.TOMLDecodeError as exc:
        sys.stderr.write(
            "error: --update produced invalid TOML; refusing to write.\n"
            f"       This is a bug in _update_section: {exc}\n"
            "       Please file an issue with the pre-edit baseline content.\n"
        )
        sys.exit(_EXIT_EDITOR_BUG)

    BASELINE_PATH.write_text(text, encoding="utf-8")

    known = {e.key for e in entries}
    return [
        k for k in new_parsed
        if k not in known and isinstance(new_parsed[k], dict)
    ]


def _read_current() -> tuple[list[CrateMarker], list[str]]:
    """Walk `VENDORED_CRATES` and read each manifest's marker.

    Returns (entries, error_messages). On any `CargoTomlParseError`
    the manifest's relative path and the underlying cause are
    appended to error_messages and processing continues with the
    next manifest, so the user sees all problems in one go.
    """
    current: list[CrateMarker] = []
    problems: list[str] = []
    for crate_key, marker, manifest_rel in VENDORED_CRATES:
        manifest = REPO_ROOT / manifest_rel
        try:
            version = read_marker(manifest, marker)
        except CargoTomlParseError as exc:
            problems.append(str(exc))
            continue
        if version is None:
            problems.append(
                f"{manifest_rel}: marker {marker!r} not found in any "
                f"dependency table (scanned to depth "
                f"{_DEP_SCAN_MAX_DEPTH})"
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
    return current, problems


def _warn_orphans(orphans: list[str]) -> None:
    """Emit a non-fatal warning for each baseline section not in
    VENDORED_CRATES. Same shape on both the gate and --update paths."""
    for orphan in orphans:
        sys.stderr.write(
            f"warning: baseline contains [{orphan}] section not in "
            f"VENDORED_CRATES; remove it manually if no longer needed.\n"
        )


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

    current, problems = _read_current()
    if problems:
        sys.stderr.write("grammar-marker-sync: cannot read manifest(s)\n")
        for line in problems:
            sys.stderr.write(f"  {line}\n")
        return 2

    if args.update:
        orphans = write_baseline(current)
        rel = BASELINE_PATH.relative_to(REPO_ROOT)
        print(f"Baseline updated: {rel}")
        for entry in current:
            print(f"  [{entry.key}] {entry.marker} = {entry.version}")
        _warn_orphans(orphans)
        return 0

    baseline = load_baseline()
    if baseline is None:
        sys.stderr.write(
            f"error: baseline file missing: "
            f"{BASELINE_PATH.relative_to(REPO_ROOT)}\n"
            "       restore it from git history.\n"
        )
        return 2

    known_keys = {e.key for e in current}
    _warn_orphans([k for k in baseline if k not in known_keys])

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
