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
* Comments are skipped — ``//`` and ``/* … */``, whole-line or
  trailing — so a comment explaining this rule, including the examples
  above, does not trip it. A comment is never a diagnostic.
* The interior of a string literal is skipped — only what a literal
  *starts with* is inspected. That matters most for the multi-line raw
  strings this workspace's embedded source fixtures live in, where real
  foreign code says ``std::cerr << "Warning: …"`` and neither opt-out
  position is reachable: both sit inside the fixture, so writing a
  marker would change the text the metric test measures.
* Anything else that legitimately contains such a literal opts out with
  ``// diag-prefix-ok`` on the same line or in the comment block above
  it. One site uses it today: the #609 regression test that asserts the
  capitalised spelling is *absent* from `bca check` stderr.

Which of those a given position falls under is decided by a single
left-to-right lexer (``_scan_literals``) rather than by matching each
line in isolation. That is not an implementation detail: three
false-clean windows came from deciding it per line, because something
that merely *looks* like a raw-string open — a plain string ending in
``r``, an unterminated ``r"`` inside a trailing or block comment — was
read as one, and every line to the next quote was then skipped (#1219).

See AGENTS.md "Validation gates" for the policy this enforces.
"""

from __future__ import annotations

import bisect
import pathlib
import re
import subprocess
import sys

# `parents[1]`, not `parent`: these gates live in `utils/` but every
# path they read is anchored at the repository root.
REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]

SKIP_DIRS = {".git", "target", "node_modules", ".venv", "__pycache__"}

# The severity word a literal's *content* may begin with, anchored: the
# gate is about what a literal starts with, not about the word appearing
# in one. `_scan_literals` supplies the content start, so the anchor is
# structural here rather than a lookbehind on the quote.
SEVERITY_PREFIX = re.compile(r"(?P<word>Warning|WARNING|Error|ERROR|Note|NOTE):")

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


def char_literal_end(source: str, i: int) -> int | None:
    """End index (exclusive) of the char literal at ``i``, else ``None``.

    Rust spells lifetimes (``'a``), anonymous lifetimes (``'_``) and loop
    labels (``'outer:``) with the same leading quote and no terminator,
    so the two are told apart by looking for the closing ``'``. Returning
    ``None`` for a lifetime is what keeps it from opening a span that
    swallows the rest of the file.
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
            # '\u{1F600}' — braced, variable length.
            close = source.find("}", j)
            if close == -1:
                return None
            j = close + 1
        elif source[j] == "x":
            # '\x41' — always exactly two hex digits.
            j += 3
        else:
            j += 1
    else:
        j += 1
    return j + 1 if j < n and source[j] == "'" else None


def raw_string_end(source: str, i: int) -> int | None:
    """End index (exclusive) of the raw string at ``i``, else ``None``.

    Covers ``r"…"``, ``r#"…"#`` and the byte spellings ``br"…"`` /
    ``br##"…"##``. A plain ``b"…"`` needs no special case: the ``b`` is
    consumed as an ordinary character and the ``"`` after it opens a
    regular literal, which escapes identically. An unterminated open
    returns the end of the source, so the rest of the file is one span —
    the same "skip to the close" behaviour the line-oriented scanner had,
    now reachable only from real code position.
    """
    n = len(source)
    j = i
    if source[j] == "b" and j + 1 < n and source[j + 1] == "r":
        j += 1
    if j >= n or source[j] != "r":
        return None
    j += 1
    hashes = 0
    while j < n and source[j] == "#":
        hashes += 1
        j += 1
    if j >= n or source[j] != '"':
        return None
    close = '"' + ("#" * hashes)
    end = source.find(close, j + 1)
    return n if end == -1 else end + len(close)


def regular_string_end(source: str, i: int) -> int:
    """End index (exclusive) of the ``"``-delimited literal at ``i``."""
    n = len(source)
    j = i + 1
    while j < n:
        if source[j] == "\\" and j + 1 < n:
            j += 2
            continue
        if source[j] == '"':
            break
        j += 1
    return j + 1


def _record(hits: list[tuple[int, str]], source: str, content_start: int) -> None:
    """Append a hit when the literal content at ``content_start`` offends."""
    match = SEVERITY_PREFIX.match(source, content_start)
    if match is not None:
        hits.append((content_start, match.group("word")))


def _scan_literals(source: str) -> list[tuple[int, str]]:
    """``(content start index, severity word)`` for every offending literal.

    One left-to-right walk that knows, at each position, whether it is in
    code, a comment, or a literal — ported from ``scan_ignore_spans`` in
    ``check-snapshot-anchors.py``, which needed the identical machine and
    is gated by its own self-tests (#1192). The two are siblings; fix a
    lexing bug in one and check the other. They are copies rather than a
    shared import because every gate under ``utils/`` is a standalone
    hyphen-named script that resolves the repository root from its own
    location, and a shared module would grow that concern on four sides.

    Deciding this from *state* rather than from a regex over each line is
    what closes three false-clean windows (#1219). All three are the same
    class — something that merely looks like a raw-string open is read as
    one, and every line until the next quote is then skipped, hiding any
    offender in between:

    * ``let p = "dir/r";`` — the closing quote of an ordinary string sits
      after ``r``, which is a legitimate raw-open context. No lookbehind
      can express the difference, because what distinguishes this case is
      that the ``"`` *closes* a literal rather than opening one.
    * ``let x = 1; // e.g. r"foo`` — a *trailing* comment. The previous
      scanner skipped a line only when the whole line was a comment.
    * ``let x = 1; /* r"foo */`` — a block comment, which it never saw at
      all.

    Stripping comments with a regex first cannot fix the latter two:
    ``"http://x"`` would be truncated mid-literal and could open a
    phantom span itself. Finding where a comment starts already requires
    knowing whether you are inside a string.

    A consequence worth stating: a severity literal quoted inside *any*
    comment is now skipped, where before only a whole-line comment was.
    That is the same direction the whole-line rule already took — a
    comment is never a diagnostic — and it removes a false-positive class
    that would otherwise have needed a ``diag-prefix-ok`` marker.
    """
    hits: list[tuple[int, str]] = []
    i = 0
    n = len(source)
    while i < n:
        ch = source[i]
        # Line comment: consume to (not including) the newline.
        if ch == "/" and i + 1 < n and source[i + 1] == "/":
            nl = source.find("\n", i)
            i = n if nl == -1 else nl
            continue
        # Block comment, which Rust allows to nest.
        if ch == "/" and i + 1 < n and source[i + 1] == "*":
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
            continue
        # Raw / byte-raw string. Only its opening delimiter is inspected:
        # the interior of a multi-line one holds this workspace's embedded
        # source fixtures, where neither opt-out position is reachable.
        if ch in "rb":
            stop = raw_string_end(source, i)
            if stop is not None:
                # Safe: between `i` and here `raw_string_end` consumed only
                # `b`, `r` and `#`, none of which is a quote, and it has
                # already verified the quote it stopped on.
                quote = source.index('"', i)
                _record(hits, source, quote + 1)
                i = stop
                continue
        # Char literal — or a lifetime, which `char_literal_end` rejects
        # so it cannot open a span that swallows the rest of the file.
        if ch == "'":
            stop = char_literal_end(source, i)
            i = stop if stop is not None else i + 1
            continue
        # Regular string literal. Escapes are consumed by
        # `regular_string_end`, so `"he said \"Error: no\""` is one span
        # whose content begins at `he`, and the inner quote — which never
        # starts a literal — cannot register as a prefix.
        if ch == '"':
            stop = regular_string_end(source, i)
            _record(hits, source, i + 1)
            i = stop
            continue
        i += 1
    return hits


def scan_text(text: str) -> list[tuple[int, str, str]]:
    """Offending ``(line number, severity word, line)`` triples.

    Line numbers are 1-based so they paste straight into an editor.

    Only what a string literal *starts with* is inspected, so the
    interior of every literal is skipped — including the multi-line raw
    strings this workspace's embedded source fixtures live in, where real
    foreign code says ``std::cerr << "Warning: …"``. Neither opt-out
    position works in there: the marker would have to sit on the
    offending line or in a ``//`` block above it, and both are inside the
    fixture, so writing one changes the very text the metric test
    measures. A diagnostic is never emitted from inside a fixture, so the
    skip costs no coverage.
    """
    # `split("\n")`, not `splitlines()`: the latter also breaks on U+2028
    # and the vertical-tab family, which rustc does not treat as line
    # terminators, so one inside a string literal would shift every
    # reported line number past it. `_line_of` counts the same `\n`, so
    # the two agree by construction.
    lines = text.split("\n")
    starts = _line_starts(text)
    return [
        (index + 1, word, lines[index].lstrip())
        for offset, word in _scan_literals(text)
        for index in (_line_of(starts, offset),)
        if not _is_allowed(lines, index)
    ]


def _line_starts(text: str) -> list[int]:
    """Start offset of each line, indexed as ``text.split("\\n")`` is."""
    starts = [0]
    pos = text.find("\n")
    while pos != -1:
        starts.append(pos + 1)
        pos = text.find("\n", pos + 1)
    return starts


def _line_of(starts: list[int], offset: int) -> int:
    """0-based index of the line containing ``offset``.

    ``bisect_left`` here would differ only for an ``offset`` that is
    itself a line start, and no recorded offset can be one: every offset
    ``_record`` receives is ``quote + 1`` for a position holding ``"``,
    so the character before it is never a newline. The two spellings are
    therefore indistinguishable by any input this gate can produce — a
    perturbation swapping them fails nothing, and that is a property of
    the call site rather than a gap in the tests. ``bisect_right`` is
    still the correct spelling of the intent.
    """
    return bisect.bisect_right(starts, offset) - 1


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
