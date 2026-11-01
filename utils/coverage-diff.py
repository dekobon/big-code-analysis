#!/usr/bin/env python3
"""Patch coverage against the project figure, by covered *count*
(#1697 plan §6.7, landed by #1744).

usage: coverage-diff.py --base-json B --head-json H --base-rev R0 --head-rev R1
                        --repo DIR --codecov codecov.yml --out DIR

`utils/patch-coverage.py` is the quick single-report approximation;
this one takes two reports and compares per-file covered counts rather
than percentages (`.claude/rules/testing.md`, "Coverage measures
execution, not discrimination": the denominator moves for reasons
unrelated to the change).

Inputs are two `cargo llvm-cov report --workspace --json` exports — the
LLVM coverage export format (`data[0].files[*].segments`, `.summary`) —
produced on the base and head revisions with the same commands CI runs.
The script:

1. Parses the full local zero-context diff `git diff -U0 R0 R1` under
   `--repo` (never a hosting API's truncated diff), keeping each head
   file's set of added / modified lines. Renames are followed; a deleted
   file contributes no head lines.
2. Applies the `ignore:` globs from `codecov.yml` to both the project and
   the patch denominators, with Codecov's glob semantics (`**` spans
   directories, `*` does not).
3. Derives per-line counts from the segments exactly as `llvm-cov` does
   for its line table: a line's count is the maximum execution count of
   the regions that *start* on it or are active through it, skipping gap
   segments; a line with no region is not executable. A line is
   `covered` (count > 0), `missed` (count == 0), or `partial` — count > 0
   with at least one zero-count region starting on it, Codecov's
   "partial" reading of a line that holds both hit and unhit branches.
4. Reports, as JSON and Markdown: project line coverage for base and
   head (executable lines, covered, partials, percent — partials count as
   covered in the percent, as Codecov's `coverage` figure does; a second
   conservative percent counts them as missed), patch coverage
   (numerator / denominator over changed executable lines in head),
   per-file rows, and every changed file that is unmapped (in the diff,
   not ignored, not in the head report — an omission to explain, not a
   zero) or non-executable (changed lines hold no region).
5. Verdict: `patch_pct >= max(base_project_pct, head_project_pct)` under
   the Codecov reading; the conservative reading is printed beside it.

Region counts per changed file (`regions_covered` / `regions_total` from
the summary) are reported as supporting evidence only.

Self-tested by `utils/coverage-diff-test.py` (`make coverage-diff-test`).
Nothing lints `utils/*.py` today (#1477); the self-test is the only
gate.
"""
from __future__ import annotations

import argparse
import json
import pathlib
import re
import subprocess
import sys
from dataclasses import dataclass, field


# --- codecov.yml ----------------------------------------------------------


def codecov_ignores(path: pathlib.Path) -> list[str]:
    """The `ignore:` list, read without a YAML dependency: the block is a
    flat list of quoted globs under a top-level `ignore:` key."""
    globs: list[str] = []
    in_block = False
    for raw in path.read_text().splitlines():
        line = raw.split("#", 1)[0].rstrip()
        if not line.strip():
            continue
        if re.match(r"^ignore:\s*$", line):
            in_block = True
            continue
        if in_block:
            m = re.match(r'^\s+-\s+"?([^"]+)"?\s*$', line)
            if m:
                globs.append(m.group(1))
                continue
            if not line.startswith(" "):
                in_block = False
    return globs


def glob_to_regex(glob: str) -> re.Pattern[str]:
    out = []
    i = 0
    while i < len(glob):
        c = glob[i]
        if glob.startswith("**/", i):
            out.append("(?:.*/)?")
            i += 3
            continue
        if glob.startswith("**", i):
            out.append(".*")
            i += 2
            continue
        if c == "*":
            out.append("[^/]*")
        elif c == "?":
            out.append("[^/]")
        else:
            out.append(re.escape(c))
        i += 1
    return re.compile("^" + "".join(out) + "$")


def ignored(rel: str, patterns: list[re.Pattern[str]]) -> bool:
    return any(p.match(rel) for p in patterns)


# --- diff -----------------------------------------------------------------


def changed_lines(repo: pathlib.Path, base: str, head: str) -> dict[str, set[int]]:
    """Head-side line numbers added or modified per head path, from the
    full zero-context diff. Deleted files are absent."""
    out = subprocess.run(
        ["git", "-C", str(repo), "diff", "-U0", "-M", "--no-color", base, head],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return parse_diff(out)


def parse_diff(text: str) -> dict[str, set[int]]:
    files: dict[str, set[int]] = {}
    current: str | None = None
    for line in text.splitlines():
        if line.startswith("+++ "):
            target = line[4:]
            if target == "/dev/null":
                current = None
            else:
                current = target[2:] if target.startswith("b/") else target
                files.setdefault(current, set())
            continue
        if current is None:
            continue
        m = re.match(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@", line)
        if m:
            start = int(m.group(1))
            count = int(m.group(2)) if m.group(2) is not None else 1
            files[current].update(range(start, start + count))
    return files


# --- llvm-cov export ------------------------------------------------------


@dataclass
class FileCov:
    """Per-line counts for one file: `counts[line]` is the line's execution
    count; `partial_lines` the lines where a zero-count region starts beside
    a non-zero line count."""

    counts: dict[int, int] = field(default_factory=dict)
    partial_lines: set[int] = field(default_factory=set)
    regions_total: int = 0
    regions_covered: int = 0


def line_counts(segments: list[list]) -> FileCov:
    """llvm-cov's line table from the segment list, following
    `LineCoverageStats` (llvm/lib/ProfileData/Coverage/CoverageMapping.cpp).

    Segments are `[line, col, count, has_count, is_region_entry, is_gap]`,
    sorted by position. For each line, with `wrapped` the last segment
    before the line: the line is *mapped* when `wrapped` has a count or a
    counted non-gap region entry starts on it, except that a line whose
    first own segment is a region entry without a count (the start of a
    skipped region) is unmapped unless some counted entry also starts on
    it. Its count is `wrapped`'s count, maxed with every counted non-gap
    region entry starting on it. A mapped line with count > 0 and a
    zero-count region start is recorded as *partial* (the line holds both
    a hit and an unhit branch — Codecov's partial reading).
    """
    cov = FileCov()
    starts: dict[int, list[tuple[int, bool, bool, bool]]] = {}
    for seg in segments:
        line, _col, count, has_count, is_entry, is_gap = seg[:6]
        starts.setdefault(line, []).append((int(count), bool(has_count), bool(is_entry), bool(is_gap)))
    if not starts:
        return cov
    lines = sorted(starts)
    wrapped: tuple[int, bool, bool, bool] | None = None
    for ln in range(lines[0], lines[-1] + 1):
        own = starts.get(ln, [])
        region_starts = [c for c, has, entry, gap in own if has and entry and not gap]
        counted_entry = any(has and entry for _, has, entry, _ in own)
        skipped_start = bool(own) and not own[0][1] and own[0][2]
        mapped = (not skipped_start and ((wrapped is not None and wrapped[1]) or bool(region_starts))) or counted_entry
        if mapped:
            value = wrapped[0] if wrapped is not None else 0
            for c in region_starts:
                value = max(value, c)
            cov.counts[ln] = value
            if value > 0 and any(c == 0 for c in region_starts):
                cov.partial_lines.add(ln)
        if own:
            wrapped = own[-1]
    return cov


def load_export(path: pathlib.Path, repo: pathlib.Path) -> dict[str, FileCov]:
    data = json.loads(path.read_text())
    files: dict[str, FileCov] = {}
    root = str(repo.resolve()) + "/"
    for f in data["data"][0]["files"]:
        name = f["filename"]
        rel = name[len(root):] if name.startswith(root) else name
        cov = line_counts(f.get("segments", []))
        summ = f.get("summary", {}).get("regions", {})
        cov.regions_total = int(summ.get("count", 0))
        cov.regions_covered = int(summ.get("covered", 0))
        files[rel] = cov
    return files


# --- calculation ----------------------------------------------------------


@dataclass
class Tally:
    executable: int = 0
    covered: int = 0
    partial: int = 0

    @property
    def missed(self) -> int:
        return self.executable - self.covered

    def pct(self, partial_as_covered: bool = True) -> float | None:
        if self.executable == 0:
            return None
        hit = self.covered if partial_as_covered else self.covered - self.partial
        return 100.0 * hit / self.executable

    def add(self, count: int, partial: bool) -> None:
        self.executable += 1
        if count > 0:
            self.covered += 1
            if partial:
                self.partial += 1


def project_tally(files: dict[str, FileCov], patterns: list[re.Pattern[str]]) -> Tally:
    t = Tally()
    for rel, cov in files.items():
        if ignored(rel, patterns):
            continue
        for ln, count in cov.counts.items():
            t.add(count, ln in cov.partial_lines)
    return t


def patch_tally(
    changed: dict[str, set[int]],
    head: dict[str, FileCov],
    patterns: list[re.Pattern[str]],
) -> tuple[Tally, list[dict], list[str], list[str], list[str]]:
    total = Tally()
    rows: list[dict] = []
    unmapped: list[str] = []
    non_exec: list[str] = []
    skipped: list[str] = []
    for rel in sorted(changed):
        lines = changed[rel]
        if ignored(rel, patterns):
            skipped.append(rel)
            continue
        if not rel.endswith(".rs"):
            skipped.append(rel)
            continue
        cov = head.get(rel)
        if cov is None:
            unmapped.append(rel)
            continue
        t = Tally()
        missed_lines: list[int] = []
        partial_lines: list[int] = []
        for ln in sorted(lines):
            if ln not in cov.counts:
                continue
            count = cov.counts[ln]
            partial = ln in cov.partial_lines
            t.add(count, partial)
            if count == 0:
                missed_lines.append(ln)
            elif partial:
                partial_lines.append(ln)
        if t.executable == 0:
            non_exec.append(rel)
            continue
        total.executable += t.executable
        total.covered += t.covered
        total.partial += t.partial
        rows.append(
            {
                "file": rel,
                "changed_lines": len(lines),
                "executable": t.executable,
                "covered": t.covered,
                "partial": t.partial,
                "missed": t.missed,
                "pct": t.pct(),
                "pct_conservative": t.pct(False),
                "missed_lines": missed_lines,
                "partial_lines": partial_lines,
                "regions_covered": cov.regions_covered,
                "regions_total": cov.regions_total,
            }
        )
    return total, rows, unmapped, non_exec, skipped


def fmt_pct(v: float | None) -> str:
    return "n/a" if v is None else f"{v:.2f}%"


def render_markdown(result: dict) -> str:
    p = result["project"]
    pt = result["patch"]
    out = [
        "# Patch coverage vs project (plan §6.7)",
        "",
        f"Base `{result['base_rev']}` → head `{result['head_rev']}`; "
        f"ignores: {len(result['ignore_globs'])} globs from codecov.yml.",
        "",
        "| scope | executable | covered | partial | missed | % (partial=hit) | % (partial=miss) |",
        "|---|---:|---:|---:|---:|---:|---:|",
    ]
    for label, t in (("project (base)", p["base"]), ("project (head)", p["head"]), ("patch", pt["total"])):
        out.append(
            f"| {label} | {t['executable']} | {t['covered']} | {t['partial']} | {t['missed']} | "
            f"{fmt_pct(t['pct'])} | {fmt_pct(t['pct_conservative'])} |"
        )
    out += [
        "",
        f"**Floor:** patch ≥ max(base, head) project = {fmt_pct(result['floor'])} → "
        f"**{'MET' if result['verdict'] else 'NOT MET'}** (Codecov reading); "
        f"conservative reading {'met' if result['verdict_conservative'] else 'not met'}.",
        "",
        "## Changed files",
        "",
        "| file | changed | exec | covered | partial | missed | % | regions |",
        "|---|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for r in pt["files"]:
        out.append(
            f"| `{r['file']}` | {r['changed_lines']} | {r['executable']} | {r['covered']} | {r['partial']} | "
            f"{r['missed']} | {fmt_pct(r['pct'])} | {r['regions_covered']}/{r['regions_total']} |"
        )
    for title, key in (
        ("Unmapped (in diff, not ignored, absent from the head report)", "unmapped"),
        ("Changed with no executable line", "non_executable"),
        ("Skipped (ignored glob or not Rust)", "skipped"),
    ):
        out += ["", f"## {title}", ""]
        items = pt[key]
        out += [f"- `{x}`" for x in items] or ["- (none)"]
    missed = [(r["file"], r["missed_lines"]) for r in pt["files"] if r["missed_lines"]]
    if missed:
        out += ["", "## Missed changed lines", ""]
        for f, lines in missed:
            out.append(f"- `{f}`: {', '.join(map(str, lines))}")
    return "\n".join(out) + "\n"


def tally_dict(t: Tally) -> dict:
    return {
        "executable": t.executable,
        "covered": t.covered,
        "partial": t.partial,
        "missed": t.missed,
        "pct": t.pct(),
        "pct_conservative": t.pct(False),
    }


def compute(
    base_files: dict[str, FileCov],
    head_files: dict[str, FileCov],
    changed: dict[str, set[int]],
    globs: list[str],
    base_rev: str,
    head_rev: str,
) -> dict:
    patterns = [glob_to_regex(g) for g in globs]
    base_t = project_tally(base_files, patterns)
    head_t = project_tally(head_files, patterns)
    patch_t, rows, unmapped, non_exec, skipped = patch_tally(changed, head_files, patterns)
    floor_candidates = [v for v in (base_t.pct(), head_t.pct()) if v is not None]
    floor = max(floor_candidates) if floor_candidates else None
    floor_c = max(v for v in (base_t.pct(False), head_t.pct(False)) if v is not None) if floor_candidates else None
    patch_pct = patch_t.pct()
    patch_pct_c = patch_t.pct(False)
    verdict = patch_pct is not None and floor is not None and patch_pct >= floor
    verdict_c = patch_pct_c is not None and floor_c is not None and patch_pct_c >= floor_c
    return {
        "base_rev": base_rev,
        "head_rev": head_rev,
        "ignore_globs": globs,
        "project": {"base": tally_dict(base_t), "head": tally_dict(head_t)},
        "patch": {
            "total": tally_dict(patch_t),
            "files": rows,
            "unmapped": unmapped,
            "non_executable": non_exec,
            "skipped": skipped,
        },
        "floor": floor,
        "floor_conservative": floor_c,
        "verdict": verdict,
        "verdict_conservative": verdict_c,
    }


# --- main -----------------------------------------------------------------


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--base-json", type=pathlib.Path, required=True)
    ap.add_argument("--head-json", type=pathlib.Path, required=True)
    ap.add_argument("--base-rev", required=True)
    ap.add_argument("--head-rev", required=True)
    ap.add_argument("--repo", type=pathlib.Path, required=True)
    ap.add_argument("--codecov", type=pathlib.Path, required=True)
    ap.add_argument("--out", type=pathlib.Path, required=True)
    args = ap.parse_args(argv)
    globs = codecov_ignores(args.codecov)
    base_files = load_export(args.base_json, args.repo)
    head_files = load_export(args.head_json, args.repo)
    changed = changed_lines(args.repo, args.base_rev, args.head_rev)
    result = compute(base_files, head_files, changed, globs, args.base_rev, args.head_rev)
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "coverage-diff.json").write_text(json.dumps(result, indent=1) + "\n")
    (args.out / "coverage-diff.md").write_text(render_markdown(result))
    print(render_markdown(result))
    return 0 if result["verdict"] else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
