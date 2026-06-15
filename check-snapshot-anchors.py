#!/usr/bin/env python3
"""check-snapshot-anchors

Lint that blocks new bare ``insta::assert_json_snapshot!`` calls in
``src/metrics/``.

A snapshot call is *anchored* if at least one of these is true:

1. The macro body contains an inline expected block (``@r"…"``,
   ``@r#"…"#``, ``@r###"…"###``, etc.).
2. There is an ``assert_eq!`` on a headline accessor within the
   five non-blank source lines preceding the macro call.
3. There is an ``// expected:`` derivation comment within the five
   non-blank source lines preceding the macro call.

Anything else counts as *bare*. Per-file unanchored counts are tracked
in a checked-in baseline file. Any *increase* over the baseline fails;
*decreases* are silent (the baseline can be regenerated with
``--update`` after intentional cleanups).

See AGENTS.md "Validation gates" for the policy this enforces.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys
from collections import OrderedDict

REPO_ROOT = pathlib.Path(__file__).resolve().parent
METRICS_DIR = REPO_ROOT / "src" / "metrics"
DEFAULT_BASELINE = REPO_ROOT / ".snapshot-anchor-baseline.txt"

MACRO_RE = re.compile(r"insta::assert_json_snapshot!\s*\(")
# Matches insta inline anchors in both forms: the non-raw `@"…"` and
# the raw-string `@r"…"`, `@r#"…"#`, `@r###"…"###`, etc. The `r` is
# optional so `@"…"` is recognized as a valid anchor rather than
# mis-counted as bare (#876).
INLINE_ANCHOR_RE = re.compile(r"@r?#*\"")
ASSERT_EQ_RE = re.compile(r"\bassert_eq!\s*\(")
EXPECTED_COMMENT_RE = re.compile(r"//\s*expected\s*:", re.IGNORECASE)
LOOKBACK_LINES = 5


def find_macro_call_end(source: str, open_paren_idx: int) -> int:
    """Return the index *after* the closing ``)`` of a macro call.

    ``open_paren_idx`` must point at the ``(`` that opens the call.
    Tracks parens, skipping string literals (including Rust raw strings
    of the form ``r"…"``, ``r#"…"#``, …) and ``//`` line comments so
    nested ``)`` inside the inline ``@r###"…"###`` anchor does not
    confuse the depth counter.
    """
    depth = 0
    i = open_paren_idx
    n = len(source)
    while i < n:
        ch = source[i]
        # Line comment: consume to end of line.
        if ch == "/" and i + 1 < n and source[i + 1] == "/":
            nl = source.find("\n", i)
            i = n if nl == -1 else nl + 1
            continue
        # Raw string: r"…", r#"…"#, r##"…"##, …
        if ch == "r" and i + 1 < n and source[i + 1] in ('"', "#"):
            j = i + 1
            hashes = 0
            while j < n and source[j] == "#":
                hashes += 1
                j += 1
            if j < n and source[j] == '"':
                close = '"' + ("#" * hashes)
                end = source.find(close, j + 1)
                i = n if end == -1 else end + len(close)
                continue
        # Regular string literal.
        if ch == '"':
            j = i + 1
            while j < n:
                if source[j] == "\\" and j + 1 < n:
                    j += 2
                    continue
                if source[j] == '"':
                    break
                j += 1
            i = j + 1
            continue
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return n


class IgnoreSpans:
    """The three kinds of index ranges a bare-call scan must ignore.

    A single source walk classifies every ``//`` line comment, every
    ``/* … */`` block comment (Rust block comments nest), and every
    string literal (regular and Rust raw strings) into disjoint span
    lists. Keeping them separate lets ``count_bare`` answer two distinct
    questions with the same scan: *is this match inside a comment or
    string?* (block + string spans) and *is the ``//`` before it a real
    line comment, or just text inside a string?* (line-comment spans).
    """

    def __init__(self) -> None:
        self.line_comments: list[tuple[int, int]] = []
        self.block_comments: list[tuple[int, int]] = []
        self.strings: list[tuple[int, int]] = []


def scan_ignore_spans(source: str) -> IgnoreSpans:
    """Classify comment and string-literal spans in one source walk.

    The walk skips string literals before testing for comment openers,
    so a ``//`` or ``/*`` that appears inside a string (or a ``"``
    inside a comment) is never misread. This is the single string-aware
    scanner the bare-call counter relies on: over-counting a commented
    or quoted ``insta::assert_json_snapshot!`` occurrence produces a
    spurious gate failure, and under-counting (treating a ``//`` inside
    a string as a real comment) lets an unanchored snapshot through.
    """
    spans = IgnoreSpans()
    i = 0
    n = len(source)
    while i < n:
        ch = source[i]
        # Line comment: span covers `//` to (but not including) the newline.
        if ch == "/" and i + 1 < n and source[i + 1] == "/":
            nl = source.find("\n", i)
            end = n if nl == -1 else nl
            spans.line_comments.append((i, end))
            i = end
            continue
        # Block comment: track nesting depth to the matching close.
        if ch == "/" and i + 1 < n and source[i + 1] == "*":
            start = i
            depth = 1
            i += 2
            while i < n and depth > 0:
                if source[i] == "/" and i + 1 < n and source[i + 1] == "*":
                    depth += 1
                    i += 2
                    continue
                if source[i] == "*" and i + 1 < n and source[i + 1] == "/":
                    depth -= 1
                    i += 2
                    continue
                i += 1
            spans.block_comments.append((start, i))
            continue
        # Raw string: r"…", r#"…"#, …
        if ch == "r" and i + 1 < n and source[i + 1] in ('"', "#"):
            j = i + 1
            hashes = 0
            while j < n and source[j] == "#":
                hashes += 1
                j += 1
            if j < n and source[j] == '"':
                close = '"' + ("#" * hashes)
                end = source.find(close, j + 1)
                stop = n if end == -1 else end + len(close)
                spans.strings.append((i, stop))
                i = stop
                continue
        # Regular string literal.
        if ch == '"':
            j = i + 1
            while j < n:
                if source[j] == "\\" and j + 1 < n:
                    j += 2
                    continue
                if source[j] == '"':
                    break
                j += 1
            stop = j + 1
            spans.strings.append((i, stop))
            i = stop
            continue
        i += 1
    return spans


def _in_any_span(idx: int, spans: list[tuple[int, int]]) -> bool:
    return any(start <= idx < end for start, end in spans)


def has_preceding_anchor(source: str, macro_start_idx: int) -> bool:
    """Look backwards up to LOOKBACK_LINES non-blank lines."""
    prefix = source[:macro_start_idx].splitlines()
    seen = 0
    for line in reversed(prefix):
        stripped = line.strip()
        if not stripped:
            continue
        if ASSERT_EQ_RE.search(stripped) or EXPECTED_COMMENT_RE.search(stripped):
            return True
        seen += 1
        if seen >= LOOKBACK_LINES:
            break
    return False


def count_bare_in_source(source: str) -> int:
    spans = scan_ignore_spans(source)
    bare = 0
    for match in MACRO_RE.finditer(source):
        macro_start = match.start()
        # Skip occurrences that are themselves inside a string literal
        # (e.g. macro text embedded in an inline `@r###"…"###` body or a
        # fixture string) or inside a `/* … */` block comment — neither
        # is a live bare call, and counting it produces a spurious gate
        # failure (#875).
        if _in_any_span(macro_start, spans.strings):
            continue
        if _in_any_span(macro_start, spans.block_comments):
            continue
        # Skip occurrences commented out with `//`. A `//` inside a
        # string literal is not a comment, so the precomputed
        # line-comment spans are consulted instead of a raw substring
        # search (#875: `let url = "http://x"; …` must not be masked).
        if _in_any_span(macro_start, spans.line_comments):
            continue
        open_paren = match.end() - 1
        call_end = find_macro_call_end(source, open_paren)
        body = source[open_paren:call_end]
        if INLINE_ANCHOR_RE.search(body):
            continue
        if has_preceding_anchor(source, macro_start):
            continue
        bare += 1
    return bare


def count_bare(path: pathlib.Path) -> int:
    return count_bare_in_source(path.read_text(encoding="utf-8"))


def collect_counts(files: list[pathlib.Path]) -> "OrderedDict[str, int]":
    counts: OrderedDict[str, int] = OrderedDict()
    for path in sorted(files):
        rel = path.relative_to(REPO_ROOT).as_posix()
        counts[rel] = count_bare(path)
    return counts


def load_baseline(path: pathlib.Path) -> dict[str, int]:
    baseline: dict[str, int] = {}
    if not path.exists():
        return baseline
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        rel, _, count = line.rpartition(" ")
        if not rel or not count.isdigit():
            sys.stderr.write(f"baseline: malformed line: {raw!r}\n")
            sys.exit(2)
        baseline[rel] = int(count)
    return baseline


def write_baseline(path: pathlib.Path, counts: dict[str, int]) -> None:
    body = [
        "# Bare insta::assert_json_snapshot! call counts per metric file.",
        "# Maintained by check-snapshot-anchors.py; see AGENTS.md.",
        "# Lower-or-equal current counts pass; any increase fails CI.",
        "# Regenerate with: ./check-snapshot-anchors.py --update",
        "",
    ]
    for rel, count in counts.items():
        body.append(f"{rel} {count}")
    path.write_text("\n".join(body) + "\n", encoding="utf-8")


def default_targets() -> list[pathlib.Path]:
    return sorted(p for p in METRICS_DIR.glob("*.rs") if p.is_file())


def _self_test() -> int:
    """Prove the bare-call scanner classifies the tricky cases.

    Each case is a small Rust snippet with its expected bare count.
    """
    cases: list[tuple[str, str, int]] = [
        # Anchored: insta's non-raw inline anchor must be recognized (#876).
        ("non-raw inline anchor", 'insta::assert_json_snapshot!(m.x, @"123");', 0),
        # Anchored: raw inline anchor (existing behavior, no regression).
        ("raw inline anchor", 'insta::assert_json_snapshot!(m.x, @r###"1"###);', 0),
        # Bare: no anchor at all.
        ("bare call", "insta::assert_json_snapshot!(m.x);", 1),
        # #875 false positive: macro text inside an inline anchor body
        # (a fixture string of Rust source) must not be counted as a
        # second, bare call.
        (
            "macro text inside string body",
            'insta::assert_json_snapshot!(m.x, @r###"\n'
            '{ "n": "insta::assert_json_snapshot!(other)" }\n'
            '"###);\n',
            0,
        ),
        # #875 false negative: a `//` inside a string earlier on the line
        # must not suppress a genuinely bare call.
        (
            "// inside a string before a bare call",
            'let url = "http://x"; insta::assert_json_snapshot!(m.x);\n',
            1,
        ),
        # A real `//`-commented-out snapshot stays uncounted.
        ("commented-out call", "// insta::assert_json_snapshot!(m.x);\n", 0),
        # A `/* … */`-commented-out snapshot stays uncounted.
        ("block-commented call", "/* insta::assert_json_snapshot!(m.x); */", 0),
    ]
    ok = True
    for name, source, expected in cases:
        got = count_bare_in_source(source)
        good = got == expected
        ok = ok and good
        print(
            f"  [{'ok' if good else 'WRONG'}] {name}: bare={got} (expected {expected})"
        )
    print("\nSelf-test:", "all expectations met." if ok else "SCANNER IS WRONG.")
    return 0 if ok else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "files",
        nargs="*",
        type=pathlib.Path,
        help="Metric files to scan (defaults to src/metrics/*.rs).",
    )
    parser.add_argument(
        "--baseline",
        type=pathlib.Path,
        default=DEFAULT_BASELINE,
        help=f"Baseline file (default: {DEFAULT_BASELINE.relative_to(REPO_ROOT)}).",
    )
    parser.add_argument(
        "--update",
        action="store_true",
        help="Overwrite the baseline with current counts and exit 0.",
    )
    parser.add_argument(
        "--show",
        action="store_true",
        help="Print current per-file counts and exit 0.",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="Run the scanner's built-in self-test and exit.",
    )
    args = parser.parse_args()

    if args.self_test:
        return _self_test()

    targets = [p.resolve() for p in args.files] if args.files else default_targets()
    counts = collect_counts(targets)

    if args.show:
        for rel, count in counts.items():
            print(f"{rel} {count}")
        return 0

    if args.update:
        write_baseline(args.baseline, counts)
        print(f"Baseline updated: {args.baseline.relative_to(REPO_ROOT)}")
        for rel, count in counts.items():
            print(f"  {rel} {count}")
        return 0

    baseline = load_baseline(args.baseline)
    if not baseline:
        sys.stderr.write(
            f"error: baseline file not found or empty: {args.baseline}\n"
            "       run with --update to create it.\n"
        )
        return 2

    regressions: list[tuple[str, int, int]] = []
    for rel, count in counts.items():
        allowed = baseline.get(rel, 0)
        if count > allowed:
            regressions.append((rel, allowed, count))

    if regressions:
        sys.stderr.write("error: bare insta::assert_json_snapshot! count regressed\n")
        for rel, allowed, count in regressions:
            sys.stderr.write(
                f"  {rel}: baseline {allowed}, current {count} "
                f"(+{count - allowed})\n"
            )
        sys.stderr.write(
            "\nAnchor each new snapshot with one of:\n"
            "  - inline expected block: insta::assert_json_snapshot!(metric.X, @r###\"…\"###)\n"
            "  - adjacent assert_eq! on a headline accessor (within 5 non-blank lines)\n"
            "  - // expected: <derivation> comment (within 5 non-blank lines)\n"
            "See AGENTS.md \"Validation gates\". If the increase is intentional\n"
            "(e.g. new tests already anchored differently), regenerate the\n"
            "baseline with: ./check-snapshot-anchors.py --update\n"
        )
        return 1

    print("snapshot-anchors: OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
