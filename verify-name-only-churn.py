#!/usr/bin/env python3
"""Verify that a bulk insta snapshot regen is *name-only churn*.

A path-normalization refactor rewrites source-file path strings (the
`name` field in raw metric output, `uri` in SARIF, a column in CSV, …)
and nothing else. This script proves that claim mechanically so a bulk
`cargo insta accept` cannot silently absorb a metric-value regression
(the #94/#95 trap: a real bug hidden inside hundreds of churned lines).

The guarantee checked, format-agnostically: **every numeric token and
every non-path text token is byte-identical between the old `.snap` and
the regenerated `.snap.new`; only path-shaped strings may differ.** It
holds for JSON, SARIF, CSV, checkstyle, and HTML snapshots alike, since
none of them put a metric value anywhere but in a number.

Method: strip the insta header, mask every source-file-path substring to
a sentinel, and compare the two bodies as a sorted multiset of lines.
Equal => only paths moved. Unequal => something else changed; the first
divergences are printed.

Failure modes this catches (all plausible outcomes of a name refactor):
  * a metric value drifts (wrong io_path read, double-count)  -> a number differs
  * a file is dropped or duplicated (dedup wrong)             -> a line is added/removed
  * a non-path identifier (function name) is corrupted        -> non-path text differs
  * a path is mangled into a non-path (empty/`..` name)        -> surfaced in the rename set
The one thing it cannot distinguish — two documents swapping identical
value-sets — is unreachable by a relabeling refactor that never touches
metric computation.

Usage:
    ./verify-name-only-churn.py [ROOT ...]      # default roots: . and the submodule
    ./verify-name-only-churn.py --self-test     # prove the verifier itself
Exit status: 0 if every changed snapshot is name-only; 1 otherwise.
"""

from __future__ import annotations

import re
import sys
from collections import Counter
from pathlib import Path

# A token is treated as a source-file path if it is a multi-segment /
# rooted path (`a/b`, `./a/b`, `/abs/a/b`) — which never matches a lone
# `/` division operator — or a bare filename ending in a source-code
# extension (`foo.rs`). Extensionless rooted paths still match the first
# arm; a genuinely extensionless bare name (e.g. `Makefile`) is NOT
# masked, so a change to it is *flagged* for review rather than hidden —
# the safe direction.
_SOURCE_EXT = (
    r"rs|pyi?|c|cc|cpp|cxx|h|hh|hpp|hxx|js|jsx|mjs|cjs|ts|tsx|mts|cts|"
    r"java|go|rb|tcl|kt|kts|cs|mm?|swift|php|sh|bash|zsh|irule|irules"
)
PATH_RE = re.compile(
    r"(?:\.{1,2}/|/)?(?:[\w\-.]+/)+[\w\-.]+"  # multi-segment or rooted path
    r"|[\w\-.]+\.(?:" + _SOURCE_EXT + r")\b"  # bare filename + source ext
)
_PATH_SENTINEL = "<PATH>"


def strip_header(text: str) -> str:
    """Drop the insta `---`...`---` metadata header (source/expression/
    assertion_line), keeping only the snapshot body. assertion_line and
    source paths in the header are not metric data and may legitimately
    move, so they are out of scope for the value check."""
    lines = text.splitlines(keepends=True)
    if not lines or lines[0].rstrip("\n") != "---":
        return text
    for i in range(1, len(lines)):
        if lines[i].rstrip("\n") == "---":
            return "".join(lines[i + 1 :])
    return text


def mask_paths(text: str) -> str:
    return PATH_RE.sub(_PATH_SENTINEL, text)


def fingerprint(text: str) -> Counter:
    """Order-insensitive, path-blind value/structure fingerprint."""
    return Counter(mask_paths(strip_header(text)).splitlines())


def paths_in(text: str) -> Counter:
    return Counter(PATH_RE.findall(strip_header(text)))


def verify_pair(
    old_text: str, new_text: str
) -> tuple[bool, list[str], Counter, Counter]:
    """Return (ok, divergences, removed_paths, added_paths)."""
    fp_old, fp_new = fingerprint(old_text), fingerprint(new_text)
    ok = fp_old == fp_new
    divergences: list[str] = []
    if not ok:
        only_old = fp_old - fp_new
        only_new = fp_new - fp_old
        for line in list(only_old.elements())[:5]:
            divergences.append(f"  - {line.strip()}")
        for line in list(only_new.elements())[:5]:
            divergences.append(f"  + {line.strip()}")
    p_old, p_new = paths_in(old_text), paths_in(new_text)
    return ok, divergences, p_old - p_new, p_new - p_old


def find_pairs(roots: list[Path]) -> list[Path]:
    news: list[Path] = []
    for root in roots:
        if root.exists():
            news.extend(sorted(root.rglob("*.snap.new")))
    return news


def run(roots: list[Path]) -> int:
    news = find_pairs(roots)
    if not news:
        print("No *.snap.new files found — nothing to verify.")
        return 0

    failures: list[Path] = []
    orphans: list[Path] = []
    renames: Counter = Counter()
    for new_path in news:
        old_path = new_path.with_suffix("")  # strip the trailing `.new`
        if not old_path.exists():
            # A brand-new snapshot cannot be a *rename*; flag it. A pure
            # name refactor must not introduce new snapshots.
            orphans.append(new_path)
            continue
        old_text = old_path.read_text(encoding="utf-8", errors="surrogateescape")
        new_text = new_path.read_text(encoding="utf-8", errors="surrogateescape")
        ok, divergences, removed, added = verify_pair(old_text, new_text)
        renames.update(
            ("%s -> %s" % (r, a)) for r, a in zip(removed.elements(), added.elements())
        )
        if not ok:
            failures.append(new_path)
            print(f"FAIL (non-path change): {new_path}")
            for line in divergences:
                print(line)

    total = len(news)
    print()
    print(f"Checked {total} regenerated snapshot(s).")
    if renames:
        print(f"Observed {sum(renames.values())} path rewrite(s); sample:")
        for rename, _ in renames.most_common(10):
            print(f"    {rename}")
    if orphans:
        print(
            f"\n{len(orphans)} new snapshot(s) with no prior `.snap` (NOT a rename — review):"
        )
        for o in orphans:
            print(f"    {o}")
    if failures or orphans:
        print(
            f"\nRESULT: {len(failures)} non-path change(s), {len(orphans)} orphan(s). NOT name-only."
        )
        return 1
    print(
        "\nRESULT: every changed snapshot is name-only churn (values/structure invariant). PASS."
    )
    return 0


_HEADER = "---\nsource: tests/x.rs\nexpression: out\n---\n"


def _self_test() -> int:
    """Prove the verifier FAILs on value/structure/identifier changes and
    PASSes on a pure path rename — the test-via-revert discipline applied
    to the verifier itself."""
    base = _HEADER + (
        "{\n"
        '  "name": "/abs/repo/src/foo.rs",\n'
        '  "kind": "unit",\n'
        '  "spaces": [\n'
        '    { "name": "compute", "metrics": { "cyclomatic": 5, "nargs": 2 } },\n'
        '    { "name": "helper", "metrics": { "cyclomatic": 1, "nargs": 0 } }\n'
        "  ]\n"
        "}\n"
    )
    # PASS: only the top-level path string changes.
    rename = base.replace("/abs/repo/src/foo.rs", "./foo.rs")
    # FAIL: a metric value drifts.
    value = base.replace('"cyclomatic": 5', '"cyclomatic": 6')
    # FAIL: a non-path identifier (function name) changes.
    ident = base.replace('"compute"', '"comput3"')
    # FAIL: a document is dropped.
    drop = base.replace(
        '    { "name": "helper", "metrics": { "cyclomatic": 1, "nargs": 0 } }\n', ""
    )
    # FAIL: a value change *masquerading* under a simultaneous rename
    # (the #94/#95 scenario the gate exists to catch).
    sneaky = rename.replace('"nargs": 2', '"nargs": 9')

    cases = [
        ("pure path rename", base, rename, True),
        ("metric value drift", base, value, False),
        ("function-name change", base, ident, False),
        ("dropped document", base, drop, False),
        ("value change hidden behind a rename", base, sneaky, False),
    ]
    ok = True
    for name, old, new, expect_pass in cases:
        passed = verify_pair(old, new)[0]
        verdict = "PASS" if passed else "FAIL"
        good = passed == expect_pass
        ok = ok and good
        print(
            f"  [{'ok' if good else 'WRONG'}] {name}: verifier says {verdict} "
            f"(expected {'PASS' if expect_pass else 'FAIL'})"
        )
    print("\nSelf-test:", "all expectations met." if ok else "VERIFIER IS WRONG.")
    return 0 if ok else 1


def main(argv: list[str]) -> int:
    args = argv[1:]
    if "--self-test" in args:
        return _self_test()
    roots = [Path(a) for a in args] or [
        Path("."),
        Path("tests/repositories/big-code-analysis-output"),
    ]
    return run(roots)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
