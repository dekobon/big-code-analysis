#!/usr/bin/env python3
"""check-diagnostic-prefix

Block a capitalised ``Warning:`` / ``Error:`` / ``Note:`` severity prefix
from being baked into a Rust string literal.

Every diagnostic this workspace prints carries a *lowercase* prefix,
matching clap's own usage-error formatter and therefore the
rustc/cargo/git diagnostic family (#609). Each crate writes that prefix
in exactly one place — ``big-code-analysis-cli/src/diag.rs`` for the CLI,
``src/diag.rs`` for the library — so a call site passes the bare message
and never spells a severity itself.

The failure mode this gate exists for (#1199): a new
``eprintln!("Warning: …")`` reads as perfectly correct in review, and
nothing else in the workspace notices. Six such sites had accumulated
across two crates before anyone tallied them, in three different
spellings.

Remediation is always the same — drop the prefix from the literal and
emit through the crate's ``warn`` helper::

    -    eprintln!("Warning: skipping {path}");
    +    warn(format_args!("skipping {path}"));

Scope and limits:

* Rust sources only (``git ls-files '*.rs'``, falling back to a
  filesystem walk outside a git checkout). Python and shell helpers
  print to their own conventions.
* The match is anchored at the *start* of a string literal, so prose
  that merely contains the word — in a doc comment, or as an escaped
  inner quotation (``"he said \\"Error: no\\""``) — is not a hit.
* Whole-line comments are skipped, so a ``//`` explaining this rule —
  including the examples above — does not trip it.
* The interior of a *multi-line* raw string is skipped. That is where
  this workspace's embedded source fixtures live, real foreign code
  says ``std::cerr << "Warning: …"``, and neither opt-out position is
  reachable: both sit inside the fixture, so writing a marker would
  change the text the metric test measures.
* Anything else that legitimately contains such a literal opts out with
  ``// diag-prefix-ok`` on the same line or in the comment block above
  it. One site uses it today: the #609 regression test that asserts the
  capitalised spelling is *absent* from `bca check` stderr.

See AGENTS.md "Validation gates" for the policy this enforces.
"""

from __future__ import annotations

import pathlib
import re
import subprocess
import sys

# `parents[1]`, not `parent`: these gates live in `utils/` but every
# path they read is anchored at the repository root.
REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]

SKIP_DIRS = {".git", "target", "node_modules", ".venv", "__pycache__"}

# A string literal — plain, raw, or hash-delimited raw — whose first
# characters are a capitalised severity word followed by a colon.
#
# The `(?<!\\)` rejects an *escaped* quote, so a quotation embedded in a
# message (`"he said \"Error: no\""`) is prose rather than a prefix. The
# gate is about what a literal starts with; an inner quote never starts
# one.
SEVERITY_LITERAL = re.compile(
    r'(?:\br)?#*(?<!\\)"(?P<word>Warning|WARNING|Error|ERROR|Note|NOTE):'
)

# The opening delimiter of a raw (or raw byte) string literal, capturing
# its hash count so the matching terminator can be searched for.
#
# The lookbehind excludes `"` and `\` as well as word characters, because
# without them an ordinary string is read as opening a raw one and every
# line until the next quote is skipped. Both shapes are live in this
# tree: `line.strip_suffix(b"\r")` ends in `\r"`, and `Ruby::R => "r",`
# ends in `"r"`. Measured across 559 files, dropping the two characters
# turns 99 candidate multi-line opens into 27.
RAW_STRING_OPEN = re.compile(r'(?<![A-Za-z0-9_"\\])b?r(?P<hashes>#*)"')

# Opt-out marker for a literal that is data rather than a diagnostic.
ALLOW_MARKER = "diag-prefix-ok"


def discover_targets(root: pathlib.Path) -> list[pathlib.Path]:
    """Every tracked Rust file in the tree.

    Workspace-wide on purpose: the sites #1199 collected sat in the
    library, in the CLI, and in output writers, and a gate scoped to one
    of those directories would have reported the other two clean.
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


def _is_allowed(lines: list[str], index: int) -> bool:
    """True when line ``index`` carries an opt-out marker.

    The marker may sit on the line itself or anywhere in the comment
    block immediately above it. Scanning the whole block rather than one
    line matters because the rationale a marker requires rarely fits on
    one line, and a marker that silently stops applying when its own
    comment grows is worse than no escape hatch.
    """
    if ALLOW_MARKER in lines[index]:
        return True
    above = index - 1
    while above >= 0 and lines[above].lstrip().startswith("//"):
        if ALLOW_MARKER in lines[above]:
            return True
        above -= 1
    return False


def _unterminated_raw_open(segment: str) -> int | None:
    """Hash count of a raw string ``segment`` opens and does not close.

    ``None`` when every raw string opened here also closes here, which is
    the ordinary case — a one-line ``r"…"`` stays in scope for the scan.
    """
    pos = 0
    while (opening := RAW_STRING_OPEN.search(segment, pos)) is not None:
        terminator = '"' + "#" * len(opening.group("hashes"))
        close = segment.find(terminator, opening.end())
        if close < 0:
            return len(opening.group("hashes"))
        pos = close + len(terminator)
    return None


def scan_text(text: str) -> list[tuple[int, str, str]]:
    """Offending ``(line number, severity word, line)`` triples.

    Line numbers are 1-based so they paste straight into an editor.

    The interior of a *multi-line* raw string is skipped: those hold the
    embedded source fixtures this workspace tests against, and real
    foreign code says ``std::cerr << "Warning: …"``. Neither opt-out
    position works in there — the marker would have to sit on the
    offending line or in a ``//`` block above it, and both are inside the
    fixture, so writing one changes the very text the metric test
    measures. A raw string that opens and closes on one line is still
    scanned; a diagnostic is never emitted from inside a fixture, so the
    skip costs no coverage.
    """
    offenders: list[tuple[int, str, str]] = []
    # `split("\n")`, not `splitlines()`: the latter also breaks on U+2028
    # and the vertical-tab family, which rustc does not treat as line
    # terminators, so one inside a string literal would shift every
    # reported line number past it.
    lines = text.split("\n")
    open_hashes: int | None = None
    for index, line in enumerate(lines):
        if open_hashes is not None:
            terminator = '"' + "#" * open_hashes
            close = line.find(terminator)
            if close < 0:
                continue
            segment = line[close + len(terminator) :]
            open_hashes = None
        else:
            segment = line
        # A `//` line cannot open a raw string either, so this returns
        # before the open-tracking below.
        if segment.lstrip().startswith("//"):
            continue
        if not _is_allowed(lines, index):
            offenders.extend(
                (index + 1, match.group("word"), line.lstrip())
                for match in SEVERITY_LITERAL.finditer(segment)
            )
        open_hashes = _unterminated_raw_open(segment)
    return offenders


def _report(offenders: list[tuple[pathlib.Path, int, str, str]]) -> None:
    print(
        f"Capitalised severity prefix in {len(offenders)} string "
        "literal(s):\n",
        file=sys.stderr,
    )
    for path, line_no, word, line in offenders:
        print(f"  {path}:{line_no}: {word}: -> {line}", file=sys.stderr)
    print(
        "\nDiagnostics carry a lowercase prefix written in one place per\n"
        "crate: `warn` / `die` / `note` in\n"
        "`big-code-analysis-cli/src/diag.rs` (CLI) and `warn` in\n"
        "`src/diag.rs` (library). Drop the prefix from the literal and\n"
        "emit through the helper (#609, #1199). A literal that is data\n"
        "rather than a diagnostic opts out with a `diag-prefix-ok`\n"
        "comment on the same line or in the comment block above it.",
        file=sys.stderr,
    )


def main() -> int:
    targets = discover_targets(REPO_ROOT)
    offenders = [
        (path.relative_to(REPO_ROOT), line_no, word, line)
        for path in targets
        for line_no, word, line in scan_text(path.read_text(encoding="utf-8"))
    ]
    if offenders:
        _report(offenders)
        return 1

    print(f"Diagnostic prefixes OK ({len(targets)} Rust files checked).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
