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

See AGENTS.md "Validation gates" and #400 for context.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys
import tomllib

REPO_ROOT = pathlib.Path(__file__).resolve().parent
BASELINE_PATH = REPO_ROOT / ".grammar-marker-baseline.toml"

# Each vendored crate, the marker it pins, and the relative path to
# its Cargo.toml. The marker name is duplicated from the baseline so
# the gate refuses to validate against a stale or mis-keyed baseline
# entry.
VENDORED_CRATES: tuple[tuple[str, str, str], ...] = (
    ("mozjs", "tree-sitter-javascript", "tree-sitter-mozjs/Cargo.toml"),
    ("mozcpp", "tree-sitter-cpp", "tree-sitter-mozcpp/Cargo.toml"),
)


def _marker_re(marker: str) -> re.Pattern[str]:
    # Matches `<marker> = "X.Y.Z"` or `<marker>="X.Y.Z"`, optionally
    # preceded by whitespace. The version is captured raw — the
    # canonical form on disk has no leading `=` (it's a Dependabot
    # marker, not a Cargo pin) so we do not try to strip one here.
    return re.compile(
        rf'^\s*{re.escape(marker)}\s*=\s*"([^"]+)"',
        re.MULTILINE,
    )


def read_marker(manifest: pathlib.Path, marker: str) -> str | None:
    text = manifest.read_text(encoding="utf-8")
    m = _marker_re(marker).search(text)
    return m.group(1) if m else None


def load_baseline() -> dict[str, dict[str, str]]:
    if not BASELINE_PATH.exists():
        sys.exit(
            f"error: baseline file missing: "
            f"{BASELINE_PATH.relative_to(REPO_ROOT)}\n"
            "       run with --update to create it."
        )
    return tomllib.loads(BASELINE_PATH.read_text(encoding="utf-8"))


def write_baseline(entries: list[tuple[str, str, str]]) -> None:
    # entries: [(crate_key, marker, version), ...]
    body = [
        "# Grammar-marker-sync baseline",
        "#",
        "# Records the upstream-grammar crate version that was in",
        "# effect when the vendored sources under",
        "# `tree-sitter-{mozjs,mozcpp}/src/` were last regenerated.",
        "# The marker dependency in each vendored crate's Cargo.toml",
        "# is a Dependabot notification hook only — Cargo never",
        "# compiles it, and bumping it does NOT regenerate the",
        "# bundled `src/parser.c` / `src/scanner.c`.",
        "#",
        "# `check-grammar-marker-sync.py` compares the live marker",
        "# line in each `Cargo.toml` against the value below; drift",
        "# fails the gate. Regenerate via",
        "# `./check-grammar-marker-sync.py --update` after a verified",
        "# source regen.",
        "#",
        "# See #400 for the audit trail and the policy this",
        "# enforces.",
        "",
    ]
    for crate_key, marker, version in entries:
        body.append(f"[{crate_key}]")
        body.append(f'marker = "{marker}"')
        body.append(f'version = "{version}"')
        body.append("")
    BASELINE_PATH.write_text("\n".join(body).rstrip() + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--update",
        action="store_true",
        help=(
            "Overwrite the baseline from current Cargo.toml markers "
            "and exit 0. Only run after you have re-executed the "
            "matching generate-*.sh script and verified the source diff."
        ),
    )
    args = parser.parse_args()

    current: list[tuple[str, str, str]] = []
    missing: list[str] = []
    for crate_key, marker, manifest_rel in VENDORED_CRATES:
        manifest = REPO_ROOT / manifest_rel
        version = read_marker(manifest, marker)
        if version is None:
            missing.append(f"{manifest_rel}: no {marker!r} marker line found")
            continue
        current.append((crate_key, marker, version))

    if missing:
        sys.stderr.write("grammar-marker-sync: marker line(s) not found\n")
        for line in missing:
            sys.stderr.write(f"  {line}\n")
        return 2

    if args.update:
        write_baseline(current)
        rel = BASELINE_PATH.relative_to(REPO_ROOT)
        print(f"Baseline updated: {rel}")
        for crate_key, marker, version in current:
            print(f"  [{crate_key}] {marker} = {version}")
        return 0

    baseline = load_baseline()
    failures: list[str] = []
    for crate_key, marker, version in current:
        entry = baseline.get(crate_key)
        if entry is None:
            failures.append(
                f"baseline missing [{crate_key}] section "
                f"(expected `marker = \"{marker}\"`, "
                f"`version = \"{version}\"`)"
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
            manifest_rel = next(
                m for k, _, m in VENDORED_CRATES if k == crate_key
            )
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
