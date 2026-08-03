#!/usr/bin/env python3
"""check-rustfmt-bail

Ratchet on the number of ``match`` arms rustfmt refuses to format.

``cargo fmt --check`` is not a complete formatting gate. rustfmt
declines to format some regions and says nothing about it: it emits the
region verbatim, exits ``0``, and reports the file as clean. Everything
inside such a region is outside the formatting gate entirely, which is
where #1086 shipped hanging-argument calls and over-length lines through
a green ``cargo fmt --all --check``.

This gate measures that directly rather than trying to predict it. For
each Rust file it over-indents *every* match-arm header to
``PROBE_INDENT`` columns — an indent rustfmt can never produce, since it
indents in multiples of four — pipes the result through
``rustfmt --emit stdout``, and counts the arm headers that come back
still over-indented. Those are the arms rustfmt declined to touch.
Per-file counts are tracked in a checked-in baseline; any *increase*
fails, *decreases* are silent and can be locked in with ``--update``.

Probing every arm is load-bearing. The bail is scoped to the enclosing
``match``, so a file whose first match formats cleanly still hides a
later one that does not; the first-arm-only version of this probe gave
11 false verdicts out of 18 bailing modules in ``src/getter/`` (#1136).

## Three causes, one measurement

This gate cannot tell you *why* rustfmt declined, and there are at least
three reasons, with different remedies:

1. **A comment inside a match pattern** — between the pattern and its
   ``=>``, or between ``|`` alternatives. This is the one
   ``.claude/rules/formatting.md`` is about, and the fix is to hoist the
   comment above the arm, outside the pattern. Never delete it:
   ``.claude/rules/macro-comments.md`` explains why that per-arm
   rationale is load-bearing where it sits.
2. **A ``macro_rules!`` body rustfmt cannot parse** — a metavariable in
   a position that is not valid Rust, such as the pattern repetition
   ``$ternary $(| $short_circuit)+`` in
   ``impl_cyclomatic_c_family!``. No comment is involved and hoisting
   one fixes nothing; the count is simply a fact about that macro.

3. **An arm wider than rustfmt's ``max_width``** (100 columns). A
   single over-long arm header makes rustfmt emit the *whole* match
   verbatim, with no comment and no macro involved — measured: a
   128-column arm bails, the same match with a short variant name does
   not. The fix is to wrap or shorten the arm, and it is the one cause
   of the three that a reader can act on immediately.

Do not start hoisting comments out of a file on this gate's say-so.
Run ``--show`` first: it names the stuck arms, which distinguishes all
three. An arm past 100 columns is cause 3; an in-pattern comment is
cause 1; neither, inside a ``macro_rules!`` body, is cause 2 and
permanent.

Reads and writes are anchored at the repository root rather than the
cwd, so the gate runs correctly from anywhere.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import shutil
import subprocess
import sys
from collections import Counter, OrderedDict
from concurrent.futures import ThreadPoolExecutor

# `parents[1]`, not `parent`: these gates live in `utils/` but every
# path they read or write is anchored at the repository root.
REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
DEFAULT_BASELINE = REPO_ROOT / ".rustfmt-bail-baseline.txt"

# Edition to parse with when a file's owning manifest does not say.
# rustfmt needs the right one: 2024 to accept let-chains, 2021 to accept
# an identifier such as `gen` that became a keyword in 2024.
DEFAULT_EDITION = "2024"

# Cache of manifest directory -> edition, so resolving an edition costs
# one read per crate rather than one per file.
_EDITION_CACHE: dict[pathlib.Path, str] = {}


def edition_for(path: pathlib.Path, root: pathlib.Path) -> str:
    """The Rust edition of the crate owning ``path``.

    The workspace is 2024, but five vendored grammar crates
    (`tree-sitter-{tcl,preproc,mozjs,mozcpp,ccomment}`) are 2021. Parsing
    one of their files as 2024 works today only because none of them
    spells a 2024 keyword; the day one does, the gate exits 2 with
    "rustfmt refused to parse these files", which reads as a broken repo
    rather than as an edition mismatch.
    """
    for parent in [path.parent, *path.parents]:
        if parent in _EDITION_CACHE:
            return _EDITION_CACHE[parent]
        manifest = parent / "Cargo.toml"
        if manifest.is_file():
            edition = DEFAULT_EDITION
            for line in manifest.read_text(encoding="utf-8").splitlines():
                stripped = line.strip()
                if stripped.startswith("edition"):
                    _, _, value = stripped.partition("=")
                    value = value.strip().strip('"')
                    # `edition.workspace = true` inherits the root's.
                    if value.isdigit():
                        edition = value
                    break
            _EDITION_CACHE[parent] = edition
            return edition
        if parent == root:
            break
    return DEFAULT_EDITION

# The probe indent. Deliberately not a multiple of four: rustfmt indents
# in multiples of four, so a surviving line at exactly this column can
# only be one rustfmt never rewrote.
PROBE_INDENT = 30
PAD = " " * PROBE_INDENT

# An arm header: at least one indent level in, starting with `_`, an
# identifier, a literal, or a leading `|` alternative, up to the `=>`
# (with an optional match guard in between).
#
# Deliberately conservative — it does not match a pattern opening with
# `(`, `[` or `&`, because `(` also opens a `macro_rules!` matcher. That
# costs nothing in coverage: the bail is scoped to the whole `match`, so
# probing any one of its arms detects it, and no match in this workspace
# is built exclusively from tuple, slice and reference patterns.
ARM_RE = re.compile(r"^\s{4,}(?:_|[0-9A-Za-z_|].*?)\s*(?:if .*)?=>")

# Directories with no Rust worth probing, used only by the non-git
# fallback discovery path.
SKIP_DIRS = {".git", "target", "node_modules", ".venv", "__pycache__"}


class IgnoreSpans:
    """Comment and string-literal index ranges within one source file.

    An arm-header regex applied to raw text matches plenty of text that
    is not a match arm: C# and JS fixtures embedded in Rust string
    literals (``const f = (x) => x``), a spaceship operator in a Groovy
    fixture (``a <=> b``), a ``// _ => …`` in a commented-out block.
    rustfmt never rewrites any of those, so every one would be counted
    as a permanent bail. Excluding them is what makes the count mean
    "arms rustfmt declined to format" rather than "lines containing
    ``=>``".
    """

    def __init__(self) -> None:
        self.spans: list[tuple[int, int]] = []

    def contains(self, idx: int) -> bool:
        return any(start <= idx < end for start, end in self.spans)


def char_literal_end(source: str, i: int) -> int | None:
    """End index (exclusive) of the char literal at ``i``, else ``None``.

    Rust spells lifetimes (``'a``), anonymous lifetimes (``'_``) and loop
    labels (``'outer:``) with the same leading quote and no terminator,
    so the two are told apart by looking for the closing ``'`` rather
    than by the opener alone. Returning ``None`` for a lifetime is what
    keeps it from opening a span that swallows the rest of the file.
    """
    n = len(source)
    j = i + 1
    if j >= n:
        return None
    if source[j] == "\\":
        j += 1
        if j >= n:
            return None
        if source[j] == "u":
            # '\u{1F600}' — the braced form is the only multi-char escape.
            close = source.find("}", j)
            if close == -1:
                return None
            j = close + 1
        else:
            j += 1
    else:
        j += 1
    return j + 1 if j < n and source[j] == "'" else None


def scan_ignore_spans(source: str) -> IgnoreSpans:
    """Classify comment and literal spans in one source walk.

    String literals are skipped before comment openers are tested, so a
    ``//`` inside a string (or a ``"`` inside a comment) is never
    misread. Rust block comments nest, so their scan tracks depth.

    Char literals are classified for one reason: a ``'"'`` or ``b'"'``
    holds an unpaired double quote, which would otherwise open a bogus
    string span running to the next ``"`` anywhere later in the file and
    hide every match arm in between. That is the "reads as clean"
    failure this gate exists to prevent, so it must not be the gate's
    own failure mode — before this was handled,
    ``src/vcs/git/diff_parse.rs`` probed 4 of its 15 arms and a file
    could bail invisibly.
    """
    out = IgnoreSpans()
    i = 0
    n = len(source)
    while i < n:
        ch = source[i]
        if ch == "/" and i + 1 < n and source[i + 1] == "/":
            nl = source.find("\n", i)
            end = n if nl == -1 else nl
            out.spans.append((i, end))
            i = end
            continue
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
            out.spans.append((start, i))
            continue
        # Raw string: r"…", r#"…"#, br##"…"##, …
        if ch in "rb" and i + 1 < n:
            j = i
            if source[j] == "b" and j + 1 < n and source[j + 1] == "r":
                j += 1
            if source[j] == "r":
                j += 1
                hashes = 0
                while j < n and source[j] == "#":
                    hashes += 1
                    j += 1
                if j < n and source[j] == '"':
                    close = '"' + ("#" * hashes)
                    end = source.find(close, j + 1)
                    stop = n if end == -1 else end + len(close)
                    out.spans.append((i, stop))
                    i = stop
                    continue
        if ch == "'":
            stop = char_literal_end(source, i)
            if stop is not None:
                out.spans.append((i, stop))
                i = stop
                continue
            i += 1
            continue
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
            out.spans.append((i, stop))
            i = stop
            continue
        i += 1
    return out


def find_arm_lines(source: str) -> list[int]:
    """Return 0-based indices of lines carrying a real match-arm header.

    A line qualifies when the arm regex matches *and* the ``=>`` it
    matched is live code — not inside a string literal or a comment.
    """
    spans = scan_ignore_spans(source)
    arms: list[int] = []
    offset = 0
    for lineno, line in enumerate(source.split("\n")):
        match = ARM_RE.match(line)
        if match and not spans.contains(offset + match.end() - 2):
            arms.append(lineno)
        offset += len(line) + 1
    return arms


def probe_arms(
    source: str,
    arms: list[int],
    rustfmt: str = "rustfmt",
    edition: str = DEFAULT_EDITION
) -> tuple[str, int, str]:
    """Probe the given arm lines. Returns ``(status, count, message)``.

    ``status`` is ``"SKIP"`` (nothing to probe), ``"ERROR"`` (rustfmt
    refused the input outright — a parse failure, not a bail) or ``"ok"``
    with the number of arms rustfmt declined to re-indent.

    The text is fed to rustfmt on **stdin**. That matters: given a path,
    rustfmt resolves and recurses into ``mod`` declarations, so probing
    ``src/getter.rs`` or ``src/metrics/cognitive.rs`` standalone errors
    out with "file not found for module" — an error that reads exactly
    like a bail if stderr is discarded, and the reason
    ``.claude/rules/formatting.md`` calls those two files unprobeable.
    On stdin there is nothing to resolve, so they probe like any other.
    """
    lines = source.split("\n")
    if not arms:
        return ("SKIP", 0, "")
    want: Counter[str] = Counter()
    for i in arms:
        stripped = lines[i].strip()
        want[stripped] += 1
        lines[i] = PAD + stripped
    run = subprocess.run(
        [rustfmt, "--edition", edition, "--emit", "stdout"],
        input="\n".join(lines),
        capture_output=True,
        text=True,
        check=False,
    )
    # Never discard stderr: rustfmt reports a refusal there while still
    # emitting the input on stdout, which is indistinguishable from a
    # bail if only stdout is read.
    if run.returncode != 0 or run.stderr.strip():
        first = (run.stderr.strip().splitlines() or ["unknown rustfmt failure"])[0]
        return ("ERROR", 0, first)
    got: Counter[str] = Counter()
    for line in run.stdout.split("\n"):
        if line.startswith(PAD) and line[PROBE_INDENT:PROBE_INDENT + 1].strip():
            got[line[PROBE_INDENT:].rstrip()] += 1
    stuck = sum(min(count, got[text]) for text, count in want.items())
    return ("ok", stuck, "")


def probe_source(
    source: str, rustfmt: str = "rustfmt", edition: str = DEFAULT_EDITION
) -> tuple[str, int, str]:
    """Probe every match-arm header in ``source`` at once."""
    return probe_arms(source, find_arm_lines(source), rustfmt, edition)


def probe_file(
    path: pathlib.Path,
    rustfmt: str = "rustfmt",
    root: pathlib.Path = REPO_ROOT,
) -> tuple[str, int, str]:
    return probe_source(
        path.read_text(encoding="utf-8"), rustfmt, edition_for(path, root)
    )


def discover_targets(root: pathlib.Path) -> list[pathlib.Path]:
    """Every Rust file in the tree.

    The sweep is workspace-wide on purpose. The site list in
    ``.claude/rules/formatting.md`` was scoped to ``src/metrics/`` for
    two revisions, which is exactly why ``src/getter/`` — where the bail
    is close to universal — went unmentioned: a directory nobody swept
    reads as clean.
    """
    listed = subprocess.run(
        ["git", "ls-files", "-z", "*.rs"],
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
    )
    if listed.returncode == 0 and listed.stdout:
        return sorted(root / rel for rel in listed.stdout.split("\0") if rel)
    return sorted(
        p
        for p in root.rglob("*.rs")
        if p.is_file() and not SKIP_DIRS.intersection(p.relative_to(root).parts)
    )


def baseline_key(path: pathlib.Path, root: pathlib.Path) -> str:
    """The baseline's spelling of ``path``: repo-relative when it is under
    ``root``, absolute otherwise.

    A named file outside the repository has no repo-relative spelling, and
    ``Path.relative_to`` raises rather than saying so — an argument typo
    then surfaces as a traceback instead of a probe result.
    """
    try:
        return path.relative_to(root).as_posix()
    except ValueError:
        return path.as_posix()


def collect_counts(
    files: list[pathlib.Path], root: pathlib.Path, rustfmt: str = "rustfmt"
) -> tuple["OrderedDict[str, int]", "OrderedDict[str, str]"]:
    """Probe every file, returning per-file bail counts and rustfmt errors."""
    with ThreadPoolExecutor() as pool:
        results = list(pool.map(lambda p: probe_file(p, rustfmt), files))
    counts: "OrderedDict[str, int]" = OrderedDict()
    errors: "OrderedDict[str, str]" = OrderedDict()
    for path, (status, count, message) in zip(files, results):
        rel = baseline_key(path, root)
        if status == "ERROR":
            errors[rel] = message
        elif count:
            counts[rel] = count
    return counts, errors


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


# Separates the generated header from hand-written notes. `--update`
# rewrites everything above it and preserves every comment line below,
# so the header and the notes can drift independently without either
# being duplicated or lost.
NOTES_SENTINEL = "# ---- notes below are preserved across --update ----"

BASELINE_HEADER = """\
# Match arms rustfmt declines to format, per file.
# Maintained by check-rustfmt-bail.py; see AGENTS.md "Validation gates".
# Lower-or-equal current counts pass; any increase fails CI.
# Regenerate with: ./utils/check-rustfmt-bail.py --update
#
# An entry means rustfmt emitted that many match-arm headers verbatim,
# so everything in the enclosing `match` sits outside `cargo fmt
# --check`. There are two causes and this gate cannot tell them apart:
#
#   1. A comment inside a match *pattern* (between the pattern and its
#      `=>`, or between `|` alternatives). Fix by hoisting the comment
#      above the arm, outside the pattern — never by deleting it. See
#      .claude/rules/formatting.md.
#   2. A `macro_rules!` body rustfmt cannot parse, typically a
#      metavariable in a non-Rust position such as the pattern
#      repetition `$ternary $(| $short_circuit)+`. No comment is
#      involved; hoisting fixes nothing and the entry is permanent.
#   3. An arm wider than rustfmt's `max_width` of 100 columns. One
#      over-long arm header makes rustfmt emit the whole match
#      verbatim, with no comment and no macro in sight. Wrap or
#      shorten it; this is the cause you can act on immediately.
#
# Check which one you have (`./utils/check-rustfmt-bail.py --show FILE`
# names the stuck arms) before editing anything: an arm past 100
# columns is cause 3, an in-pattern comment is cause 1, and neither --
# inside a `macro_rules!` body -- is cause 2.
#
""" + NOTES_SENTINEL + """
"""


def write_baseline(
    path: pathlib.Path, counts: dict[str, int], notes: str | None = None
) -> None:
    body = [BASELINE_HEADER.rstrip("\n")]
    if notes:
        body.append(notes.rstrip("\n"))
    body.append("")
    body.extend(f"{rel} {count}" for rel, count in sorted(counts.items()))
    path.write_text("\n".join(body) + "\n", encoding="utf-8")


def read_notes(path: pathlib.Path) -> str | None:
    """Preserve hand-written comment blocks across an ``--update``.

    The decision to *leave* a bail in place — because its comment reads
    worse hoisted — is recorded as a comment in the baseline, which is
    where the next person will actually see it. Regenerating the file
    must not silently discard those.
    """
    if not path.exists():
        return None
    lines = path.read_text(encoding="utf-8").splitlines()
    # Split on the sentinel rather than on a positional prefix. The
    # header lives in two places by construction -- this module's
    # constant and the checked-in file -- so they drift, and a
    # positional compare then treats the file's whole header as a note
    # and `--update` writes it out a second time.
    if NOTES_SENTINEL in lines:
        rest = lines[lines.index(NOTES_SENTINEL) + 1 :]
    else:
        # A baseline written before the sentinel existed. The old
        # prefix strip is correct whenever the header has not drifted,
        # and keeping notes twice is better than losing them.
        header = BASELINE_HEADER.splitlines()
        rest = lines[len(header) :] if lines[: len(header)] == header else lines
    kept = [line for line in rest if line.startswith("#")]
    return "\n".join(kept) if kept else None


def _show(files: list[pathlib.Path], root: pathlib.Path, rustfmt: str) -> int:
    """Name the stuck arms per file, so the cause can be diagnosed."""
    for path in files:
        source = path.read_text(encoding="utf-8")
        status, count, message = probe_file(path, rustfmt)
        rel = baseline_key(path, root)
        if status == "ERROR":
            print(f"ERROR {rel}: {message}")
            continue
        if not count:
            continue
        print(f"BAILS {rel} {count}")
        lines = source.split("\n")
        # Re-probe one arm at a time so each stuck one can be named with
        # its real line number, which the aggregate count cannot give.
        for i in find_arm_lines(source):
            one_status, one_count, _ = probe_arms(source, [i], rustfmt)
            if one_status == "ok" and one_count:
                print(f"      {rel}:{i + 1}: {lines[i].strip()[:100]}")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=__doc__.splitlines()[2],
        epilog=(
            "Two different causes produce an entry here: a comment inside a "
            "match pattern (hoist it above the arm) and a macro_rules! body "
            "rustfmt cannot parse (nothing to fix). This gate cannot tell "
            "them apart -- run --show and look for the in-pattern comment "
            "before editing. See .claude/rules/formatting.md."
        ),
    )
    parser.add_argument(
        "files",
        nargs="*",
        type=pathlib.Path,
        help="Rust files to probe (defaults to every tracked *.rs file).",
    )
    parser.add_argument(
        "--baseline",
        type=pathlib.Path,
        default=DEFAULT_BASELINE,
        help=f"Baseline file (default: {DEFAULT_BASELINE.name}).",
    )
    parser.add_argument(
        "--root",
        type=pathlib.Path,
        default=REPO_ROOT,
        help=argparse.SUPPRESS,
    )
    parser.add_argument(
        "--update",
        action="store_true",
        help="Overwrite the baseline with current counts and exit 0.",
    )
    parser.add_argument(
        "--show",
        action="store_true",
        help="Print each bailing file with the source line of every stuck arm.",
    )
    args = parser.parse_args(argv)

    rustfmt = shutil.which("rustfmt")
    if rustfmt is None:
        print("rustfmt-bail: rustfmt not found, skipping")
        return 0

    root = args.root.resolve()
    targets = (
        [p.resolve() for p in args.files] if args.files else discover_targets(root)
    )

    if args.show:
        return _show(targets, root, rustfmt)

    counts, errors = collect_counts(targets, root, rustfmt)

    if errors:
        sys.stderr.write("error: rustfmt refused to parse these files\n")
        for rel, message in errors.items():
            sys.stderr.write(f"  {rel}: {message}\n")
        sys.stderr.write(
            "\nThis is a parse failure, not a formatting bail. Fix the file\n"
            "(or the probe) before trusting the counts below.\n"
        )
        return 2

    if args.update:
        write_baseline(args.baseline, counts, read_notes(args.baseline))
        total = sum(counts.values())
        print(
            f"Baseline updated: {args.baseline.name} "
            f"({total} arms across {len(counts)} files)"
        )
        return 0

    # Absence of the file is the error; an entry-free file is a
    # legitimate end state (every bail hoisted away) and must pass.
    if not args.baseline.exists():
        sys.stderr.write(
            f"error: baseline file not found: {args.baseline}\n"
            "       run with --update to create it.\n"
        )
        return 2
    baseline = load_baseline(args.baseline)

    regressions = [
        (rel, baseline.get(rel, 0), count)
        for rel, count in counts.items()
        if count > baseline.get(rel, 0)
    ]
    if regressions:
        sys.stderr.write("error: rustfmt-bail count regressed\n")
        for rel, allowed, count in regressions:
            sys.stderr.write(
                f"  {rel}: baseline {allowed}, current {count} (+{count - allowed})\n"
            )
        sys.stderr.write(
            "\nrustfmt is now emitting more match arms verbatim in these files,\n"
            "so `cargo fmt --check` no longer sees them. Run\n"
            "`./utils/check-rustfmt-bail.py --show <file>` first -- it names the\n"
            "stuck arms, which tells you which of the three causes you have:\n"
            "  * an arm wider than 100 columns -> wrap or shorten it;\n"
            "  * a comment inside a match pattern (between the pattern and its\n"
            "    `=>`, or between `|` alternatives) -> hoist it above the arm,\n"
            "    never delete it;\n"
            "  * neither, inside a `macro_rules!` body -> rustfmt cannot parse\n"
            "    the body and there is nothing to fix.\n"
            ".claude/rules/formatting.md has the full rule. If the increase is\n"
            "deliberate and unavoidable, ratchet with:\n"
            "  ./utils/check-rustfmt-bail.py --update\n"
        )
        return 1

    print("rustfmt-bail: OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
