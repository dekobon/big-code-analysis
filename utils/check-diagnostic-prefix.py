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
  that merely contains the word ("… on Error: retry" in a doc comment)
  is not a hit.
* Whole-line comments are skipped, so a ``//`` explaining this rule —
  including the examples above — does not trip it.
* An embedded fixture that legitimately contains such a literal (a C
  string in a test corpus, say) opts out with ``// diag-prefix-ok`` on
  the same line or in the comment block above it. One site uses it
  today: the #609 regression test that asserts the capitalised spelling
  is *absent* from `bca check` stderr.

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
SEVERITY_LITERAL = re.compile(
    r'(?:\br)?#*"(?P<word>Warning|WARNING|Error|ERROR|Note|NOTE):'
)

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


def scan_text(text: str) -> list[tuple[int, str, str]]:
    """Offending ``(line number, severity word, line)`` triples.

    Line numbers are 1-based so they paste straight into an editor.
    """
    offenders: list[tuple[int, str, str]] = []
    # `split("\n")`, not `splitlines()`: the latter also breaks on U+2028
    # and the vertical-tab family, which rustc does not treat as line
    # terminators, so one inside a string literal would shift every
    # reported line number past it.
    lines = text.split("\n")
    for index, line in enumerate(lines):
        stripped = line.lstrip()
        if stripped.startswith("//"):
            continue
        match = SEVERITY_LITERAL.search(line)
        if match and not _is_allowed(lines, index):
            offenders.append((index + 1, match.group("word"), stripped))
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
