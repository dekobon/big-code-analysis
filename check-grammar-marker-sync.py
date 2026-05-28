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

When you bump the marker and DO NOT regenerate the source, the
gate trips on the Cargo.toml change. When you regenerate the
source and DO NOT update the baseline, the gate trips on the
baseline-vs-Cargo.toml mismatch. Either direction surfaces the
drift before it lands in `main`.

Regenerate the baseline after a verified regen with:

    ./check-grammar-marker-sync.py --update

`--update` rewrites only the `version` values of existing
sections — per-section audit comments, ordering, and the
file-level header are preserved verbatim. New sections are
appended with a minimal template.

See AGENTS.md "Validation gates" and #400 for context.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys
import tomllib
from typing import Any

REPO_ROOT = pathlib.Path(__file__).resolve().parent
BASELINE_PATH = REPO_ROOT / ".grammar-marker-baseline.toml"

# Each vendored crate, the marker it pins, and the relative path to
# its Cargo.toml. The marker name is duplicated into the baseline so
# the gate refuses to validate against a stale or mis-keyed entry.
VENDORED_CRATES: tuple[tuple[str, str, str], ...] = (
    ("mozjs", "tree-sitter-javascript", "tree-sitter-mozjs/Cargo.toml"),
    ("mozcpp", "tree-sitter-cpp", "tree-sitter-mozcpp/Cargo.toml"),
)

# `<marker> = "X.Y.Z"` (or `<marker>="X.Y.Z"`), optionally indented.
# Anchored to start-of-line in MULTILINE so a list-element form
# (`build = ["tree-sitter-javascript"]` in the cargo-udeps ignore
# block) cannot accidentally match.
def _marker_re(marker: str) -> re.Pattern[str]:
    return re.compile(
        rf'^\s*{re.escape(marker)}\s*=\s*"([^"]+)"',
        re.MULTILINE,
    )


def read_marker(manifest: pathlib.Path, marker: str) -> str | None:
    text = manifest.read_text(encoding="utf-8")
    m = _marker_re(marker).search(text)
    return m.group(1) if m else None


def load_baseline() -> dict[str, dict[str, str]] | None:
    """Parse the baseline.

    Returns `None` when the file is missing so the caller can
    surface the actionable "run with --update" hint. Exits with
    code 2 on malformed TOML — leaking the raw `tomllib`
    traceback to the user is a UX regression compared to the
    rest of the script's error reporting.
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
    # Narrow `Any` to the `dict[str, str]` shape we expect for
    # each crate section; ignore non-string scalars defensively.
    out: dict[str, dict[str, str]] = {}
    for key, value in parsed.items():
        if not isinstance(value, dict):
            continue
        out[key] = {k: str(v) for k, v in value.items() if isinstance(v, str)}
    return out


# Matches `[section]` headers anchored to start-of-line, used to
# carve each section's body out of the baseline file in-place.
_SECTION_HEADER_RE = re.compile(r"^\[([^\]]+)\]\s*$", re.MULTILINE)
# Matches a `version = "..."` line within a section body.
_VERSION_LINE_RE = re.compile(r'^(\s*version\s*=\s*)"([^"]+)"', re.MULTILINE)


def _replace_section_version(text: str, section: str, new_version: str) -> str | None:
    """Replace `version = "..."` inside `[section]` in `text`.

    Returns the updated text on success, or `None` if the
    section header is absent or has no `version` line — the
    caller then appends a fresh template instead.
    """
    section_pat = re.compile(rf"^\[{re.escape(section)}\]\s*$", re.MULTILINE)
    m = section_pat.search(text)
    if not m:
        return None
    body_start = m.end()
    next_section = _SECTION_HEADER_RE.search(text, body_start)
    body_end = next_section.start() if next_section else len(text)
    body = text[body_start:body_end]
    new_body, n = _VERSION_LINE_RE.subn(
        lambda mb: f'{mb.group(1)}"{new_version}"',
        body,
        count=1,
    )
    if n == 0:
        return None
    return text[:body_start] + new_body + text[body_end:]


_BASELINE_HEADER = """\
# Grammar-marker-sync baseline
#
# Records the upstream-grammar crate version that was in effect
# when the vendored sources under `tree-sitter-{mozjs,mozcpp}/src/`
# were last regenerated. The marker dependency in each vendored
# crate's Cargo.toml is a Dependabot notification hook only —
# Cargo never compiles it, and bumping it does NOT regenerate the
# bundled `src/parser.c` / `src/scanner.c`.
#
# `check-grammar-marker-sync.py` compares the live marker line in
# each `Cargo.toml` against the value below; drift fails the gate.
# Regenerate via `./check-grammar-marker-sync.py --update` after a
# verified source regen — existing per-section comments survive
# the update.
#
# See #400 for the audit trail and the policy this enforces.

"""


def write_baseline(entries: list[tuple[str, str, str, str]]) -> None:
    """Update version values in place, preserving existing comments.

    `entries` is `[(crate_key, marker, manifest_rel, version), ...]`.
    Existing sections are updated by replacing only their `version`
    line — header text, per-section audit notes, ordering, and
    formatting all survive. Sections not yet in the file are
    appended with a minimal template.
    """
    if BASELINE_PATH.exists():
        text = BASELINE_PATH.read_text(encoding="utf-8")
    else:
        text = _BASELINE_HEADER
    for crate_key, marker, _manifest_rel, version in entries:
        updated = _replace_section_version(text, crate_key, version)
        if updated is not None:
            text = updated
            continue
        if text and not text.endswith("\n"):
            text += "\n"
        text += (
            f'\n[{crate_key}]\n'
            f'marker = "{marker}"\n'
            f'version = "{version}"\n'
        )
    BASELINE_PATH.write_text(text, encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--update",
        action="store_true",
        help=(
            "Overwrite the baseline version values from current "
            "Cargo.toml markers and exit 0. Per-section comments "
            "are preserved. Only run after you have re-executed "
            "the matching generate-*.sh script and verified the "
            "source diff."
        ),
    )
    args = parser.parse_args()

    # 4-tuple — manifest_rel is carried alongside so the failure
    # message can cite the exact path without a second lookup.
    current: list[tuple[str, str, str, str]] = []
    missing: list[str] = []
    for crate_key, marker, manifest_rel in VENDORED_CRATES:
        manifest = REPO_ROOT / manifest_rel
        version = read_marker(manifest, marker)
        if version is None:
            missing.append(
                f"{manifest_rel}: no {marker!r} marker line found"
            )
            continue
        current.append((crate_key, marker, manifest_rel, version))

    if missing:
        sys.stderr.write("grammar-marker-sync: marker line(s) not found\n")
        for line in missing:
            sys.stderr.write(f"  {line}\n")
        return 2

    if args.update:
        write_baseline(current)
        rel = BASELINE_PATH.relative_to(REPO_ROOT)
        print(f"Baseline updated: {rel}")
        for crate_key, marker, _manifest_rel, version in current:
            print(f"  [{crate_key}] {marker} = {version}")
        return 0

    baseline = load_baseline()
    if baseline is None:
        sys.stderr.write(
            f"error: baseline file missing: "
            f"{BASELINE_PATH.relative_to(REPO_ROOT)}\n"
            "       run with --update to create it.\n"
        )
        return 2

    failures: list[str] = []
    for crate_key, marker, manifest_rel, version in current:
        entry = baseline.get(crate_key)
        if entry is None:
            failures.append(
                f"baseline missing [{crate_key}] section "
                f'(expected `marker = "{marker}"`, '
                f'`version = "{version}"`)'
            )
            continue
        baseline_marker = entry.get("marker")
        baseline_version = entry.get("version")
        if baseline_marker != marker:
            failures.append(
                f"[{crate_key}] marker name drift: "
                f"baseline {baseline_marker!r}, Cargo.toml {marker!r}"
            )
            continue
        if baseline_version != version:
            failures.append(
                f"[{crate_key}] {marker}: baseline {baseline_version!r}, "
                f"{manifest_rel} {version!r}"
            )

    if failures:
        sys.stderr.write("grammar-marker-sync: marker drifted from baseline\n")
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
