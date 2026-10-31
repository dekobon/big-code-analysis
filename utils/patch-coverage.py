#!/usr/bin/env python3
"""Approximate patch coverage from a `cargo llvm-cov report --json` file.

Usage: patch-coverage.py <coverage.json> <base-ref> [<repo-root>]

Reads the added lines of `git diff <base-ref>` and reports, for every
changed file that llvm-cov instrumented, how many of those lines carry
an execution count above zero. The project figure is computed from the
same JSON over the files `codecov.yml`'s `ignore:` list keeps, so the
two percentages share one denominator rule.

This is an LLVM line-coverage *approximation* of what Codecov will
say, not a reproduction. CI uploads `cargo llvm-cov --codecov`, whose
converter scores a line with several regions as a fraction (a line
that is 3/5 covered there counts as covered here), and its denominator
is llvm-cov's own per-line summary rather than the segment walk below.
On one 397-file report the two disagreed on 712 lines. Use the number
to find the missed lines before pushing; quote Codecov's own status
for the figure.

Not a gate: it takes its inputs as arguments and is run by hand before
quoting a patch-coverage number on a PR (see
`.claude/rules/testing.md`, "Coverage measures execution, not
discrimination"). The JSON it reads is a tier-2 artifact under
`.claude/rules/temp-files.md`; this driver is tier 1, which is why it
lives here rather than beside the JSON.
"""

import json
import re
import subprocess
import sys
from fnmatch import fnmatch
from pathlib import Path


def ignored_globs(root: Path) -> list[str]:
    globs = []
    text = (root / "codecov.yml").read_text()
    in_ignore = False
    for line in text.splitlines():
        if line.startswith("ignore:"):
            in_ignore = True
            continue
        if in_ignore:
            m = re.match(r'\s+-\s+"([^"]+)"', line)
            if m:
                globs.append(m.group(1))
            elif line.strip() and not line.lstrip().startswith("#"):
                in_ignore = False
    return globs


def is_ignored(rel: str, globs: list[str]) -> bool:
    for g in globs:
        if fnmatch(rel, g) or fnmatch("/" + rel, "/" + g):
            return True
        # `**/tests/**` must also match a path containing `/tests/`.
        if g.startswith("**/") and g.endswith("/**"):
            seg = g[3:-3]
            if f"/{seg}/" in f"/{rel}/":
                return True
    return False


def added_lines(root: Path, base: str) -> dict[str, set[int]]:
    # `-c diff.noprefix=false` and `--src-prefix` / `--dst-prefix` pin
    # the `+++ b/` header the parser keys on regardless of the user's
    # git config; `-c core.quotePath=false` keeps non-ASCII names
    # unquoted; `--no-ext-diff` / `--no-textconv` keep the output git's.
    out = subprocess.run(
        [
            "git",
            "-c",
            "diff.noprefix=false",
            "-c",
            "core.quotePath=false",
            "diff",
            "--unified=0",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            base,
            "--",
            "*.rs",
        ],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    files: dict[str, set[int]] = {}
    cur = None
    for line in out.splitlines():
        if line.startswith("+++ "):
            # `+++ /dev/null` is a deletion: nothing added, skip its hunks.
            cur = line[4:]
            cur = cur[2:] if cur.startswith("b/") else None
            if cur:
                files.setdefault(cur, set())
        elif line.startswith("@@") and cur:
            m = re.search(r"\+(\d+)(?:,(\d+))?", line)
            start = int(m.group(1))
            count = int(m.group(2)) if m.group(2) is not None else 1
            files[cur].update(range(start, start + count))
    return files


def line_counts(data: dict) -> dict[str, dict[int, int]]:
    """Per file, max execution count per line, from the region table."""
    per_file: dict[str, dict[int, int]] = {}
    for export in data["data"]:
        for f in export["files"]:
            segs = f["segments"]
            counts: dict[int, int] = per_file.setdefault(f["filename"], {})
            # Segments: [line, col, count, has_count, is_region_entry, is_gap]
            prev = None
            for seg in segs:
                line, _col, count, has_count, _entry, _gap = seg
                if prev is not None:
                    pl, pcount, phas = prev
                    if phas:
                        for ln in range(pl, line + 1):
                            counts[ln] = max(counts.get(ln, 0), pcount)
                prev = (line, count, has_count)
    return per_file


def main() -> int:
    if len(sys.argv) not in (3, 4):
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    cov = Path(sys.argv[1])
    base = sys.argv[2]
    root = Path(sys.argv[3]).resolve() if len(sys.argv) > 3 else Path.cwd().resolve()
    data = json.loads(cov.read_text())
    globs = ignored_globs(root)
    counts = line_counts(data)

    # Project figure: llvm-cov's own per-file line summary, over kept
    # files. A file outside `root` — a report taken in another checkout,
    # or a dependency llvm-cov instrumented — cannot be matched against
    # `codecov.yml`, so it is skipped and counted rather than guessed at.
    proj_cov = proj_tot = outside = 0
    for export in data["data"]:
        for f in export["files"]:
            try:
                rel = str(Path(f["filename"]).resolve().relative_to(root))
            except ValueError:
                outside += 1
                continue
            if is_ignored(rel, globs):
                continue
            s = f["summary"]["lines"]
            proj_cov += s["covered"]
            proj_tot += s["count"]

    added = added_lines(root, base)
    patch_cov = patch_tot = 0
    rows = []
    for rel, lines in sorted(added.items()):
        if is_ignored(rel, globs):
            continue
        key = str(root / rel)
        if key not in counts:
            rows.append((rel, "not instrumented", len(lines), 0, 0))
            continue
        c = counts[key]
        instrumented = [ln for ln in lines if ln in c]
        covered = [ln for ln in instrumented if c[ln] > 0]
        missed = sorted(ln for ln in instrumented if c[ln] == 0)
        patch_cov += len(covered)
        patch_tot += len(instrumented)
        rows.append((rel, missed, len(lines), len(instrumented), len(covered)))

    if proj_tot:
        print(
            f"project (codecov ignore applied): {proj_cov}/{proj_tot} = {100 * proj_cov / proj_tot:.2f}%"
        )
    else:
        print("project (codecov ignore applied): no instrumented files after ignore")
    if outside:
        print(
            f"  ({outside} instrumented files lie outside {root} and were skipped;"
            " was the report taken in this checkout?)"
        )
    if patch_tot:
        print(f"patch:   {patch_cov}/{patch_tot} = {100 * patch_cov / patch_tot:.2f}%")
    else:
        print("patch:   no instrumented added lines")
    for rel, missed, n_added, n_inst, n_cov in rows:
        print(
            f"  {rel}: added={n_added} instrumented={n_inst} covered={n_cov} missed={missed}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
