#!/usr/bin/env python3
"""check-grammar-build-watch

Guard that every vendored grammar crate's ``bindings/rust/build.rs``
still carries the hand-maintained block that ``rerun-if-changed``-watches
the headers under ``src/tree_sitter/``.

#1773 added that block to the five vendored build scripts. Before it, a
header-only edit (``parser.h``, ``alloc.h``, ``array.h``) left a stale
parser object behind a fresh ``build inputs:`` digest — a binary
advertising a tree it did not compile. The block is the one thing in
those files that is *not* generated binding boilerplate: AGENTS.md
exempts the vendored crates from the workspace lint tables because a
regeneration replaces their sources wholesale, and ``recreate-grammars.sh``
/ ``generate-grammars/*.sh`` happen to leave ``build.rs`` alone today.
Nothing but this gate would notice if a future regeneration, an upstream
template refresh, or a tidy-up dropped the block, and the symptom is
exactly the one it exists to prevent (#1778).

The check is verbatim, not shape-based. Each ``build.rs`` is stripped of
``//`` and ``/* */`` comments (string and char literals are left intact —
the block itself prints ``cargo:rerun-if-changed=``) and lexed into
tokens: a literal, a word, or one punctuation character each, with the
whitespace between them discarded. The canonical block, lexed the same
way, must appear as a contiguous run of that token sequence. Comparing
tokens rather than whitespace-collapsed text is what makes a rustfmt
re-wrap at a bracket (``println!(\n"…",\n…\n)``) a no-op without
admitting anything else; a shape check — "names ``tree_sitter``
somewhere near a ``rerun-if-changed``" — was considered and rejected,
since it admits a file that names the path but never watches it.

The crate list is derived from the root ``Cargo.toml``'s
``[workspace].exclude`` array, keeping every entry whose name contains
``tree-sitter``, so a sixth vendored grammar is in scope the moment it
is excluded. A listed crate with no directory, or one with no
``build.rs`` at all, fails: the latter *is* the regeneration-dropped-it
case and is never skipped.

``--root <dir>`` points the gate at another checkout, which is how the
self-tests drive it over scratch layouts.

See AGENTS.md "Rust conventions" for the exemption this qualifies.
"""

from __future__ import annotations

import argparse
import difflib
import pathlib
import re
import sys
import tomllib

# `parents[1]`, not `parent`: these gates live in `utils/` but every
# path they read is anchored at the repository root.
REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]

BUILD_SCRIPT = pathlib.Path("bindings") / "rust" / "build.rs"

# An excluded entry whose name contains this is a vendored grammar. A
# substring rather than a `tree-sitter-` prefix, for the reason
# `check-excluded-manifests.py` gives for its dependency rule: the five
# crates are published as `bca-tree-sitter-*`, and a sixth laid out under
# its published name — or a `dekobon-tree-sitter-*` — would otherwise be
# silently out of scope while the gate printed OK. The other excluded
# entries (`enums`, `fuzz`, `.claude/worktrees`) contain no such substring.
VENDORED_SUBSTRING = "tree-sitter"

# Lifted verbatim from tree-sitter-tcl/bindings/rust/build.rs (lines 9-19
# at #1773), where the other four crates carry the identical statement.
# Comments and whitespace are irrelevant to the comparison — both sides
# go through `tokens(…)` — so the block is kept in its source spelling
# for readability. A deliberate change to the watch in the build scripts
# is also a change to this constant; the gate fails until the two agree
# again, which is the point.
CANONICAL_BLOCK = """\
    let headers = src_dir.join("tree_sitter");
    if let Ok(entries) = std::fs::read_dir(&headers) {
        for entry in entries.flatten() {
            match entry.path().to_str() {
                Some(path) => println!("cargo:rerun-if-changed={path}"),
                // A name cargo cannot be told exactly: watch the directory
                // instead, which changes whenever that file is replaced.
                None => println!("cargo:rerun-if-changed={}", headers.display()),
            }
        }
    }
"""

# One string or char literal, matched at the current position. In order:
# a raw string `r"…"` / `r#"…"#` (the `\1` backreference closes it on the
# same number of hashes, and nothing inside is an escape), an ordinary
# string with `\"` skipped, and a char literal `'x'` / `'\n'` / `'\''`.
# Anything else after a `'` is a lifetime or label and carries no closing
# quote to pair with. The escape is `\\[\s\S]`, not `\\.`: a backslash
# at end of line is Rust's string continuation, and `.` without DOTALL
# stops at the newline, leaves the string unterminated, and mispairs
# every quote after it (stage-E R1).
LITERAL = r'b?r(#*)"[\s\S]*?"\1|"(?:\\[\s\S]|[^"\\])*"|\'(?:\\[\s\S]|[^\\\'])\''
LITERAL_AT = re.compile(LITERAL)

# A token is a literal, a word, or one punctuation character; whitespace
# separates tokens and is otherwise meaningless.
TOKEN = re.compile(LITERAL + r"|\w+|[^\w\s]")

# A candidate region less similar than this to the canonical block is
# not a damaged copy of it; the file simply does not carry the block.
# The anchor alone cannot decide that: every grammar build.rs shares
# `println!("cargo:rerun-if-changed={}",` with the block through its
# parser.c / scanner.c watches, so an anchor always exists.
MIN_SIMILARITY = 0.5


class ManifestError(Exception):
    """The root manifest did not yield a usable vendored-crate list."""


def vendored_crates(manifest_text: str) -> list[str]:
    """Return the ``[workspace].exclude`` entries naming ``tree-sitter``.

    Raises :class:`ManifestError` when the array is absent, malformed,
    or names no vendored grammar at all — an empty list would make the
    gate pass vacuously, which is the one verdict it must never give.
    """
    data = tomllib.loads(manifest_text)
    workspace = data.get("workspace")
    exclude = workspace.get("exclude") if isinstance(workspace, dict) else None
    if not isinstance(exclude, list) or not all(isinstance(e, str) for e in exclude):
        raise ManifestError("[workspace].exclude is missing or is not a string array")
    crates = [entry for entry in exclude if VENDORED_SUBSTRING in entry]
    if not crates:
        raise ManifestError(f"[workspace].exclude names no `*{VENDORED_SUBSTRING}*` crate")
    return crates


def strip_comments(source: str) -> str:
    """Remove ``//`` line comments and (nested) ``/* */`` block comments.

    String and char literals are copied through untouched, so a ``//``
    or ``/*`` inside ``"cargo:rerun-if-changed=…"`` cannot open a
    comment, a ``'"'`` char literal cannot open a string, and a raw
    string's trailing backslash cannot escape its closing quote. An
    unterminated literal is copied one character at a time, so it
    hides nothing after it.
    """
    out: list[str] = []
    i, n = 0, len(source)
    while i < n:
        two = source[i : i + 2]
        if m := LITERAL_AT.match(source, i):
            out.append(m.group(0))
            i = m.end()
        elif two == "//":
            end = source.find("\n", i)
            i = n if end == -1 else end
        elif two == "/*":
            depth, i = 1, i + 2
            while i < n and depth:
                if source.startswith("/*", i):
                    depth, i = depth + 1, i + 2
                elif source.startswith("*/", i):
                    depth, i = depth - 1, i + 2
                else:
                    i += 1
        else:
            out.append(source[i])
            i += 1
    return "".join(out)


def tokens(source: str) -> list[str]:
    """Lex comment-stripped Rust into literal / word / punctuation tokens."""
    return [m.group(0) for m in TOKEN.finditer(strip_comments(source))]


def canonical() -> list[str]:
    return tokens(CANONICAL_BLOCK)


def contains_run(haystack: list[str], needle: list[str]) -> bool:
    """True when ``needle`` occurs as a contiguous run inside ``haystack``."""
    width = len(needle)
    return any(haystack[i : i + width] == needle for i in range(len(haystack) - width + 1))


def closest_region(expected: list[str], haystack: list[str]) -> list[str] | None:
    """The run of ``haystack`` that best lines up with ``expected``.

    Anchored on the longest common token run; ``None`` when the region
    that anchor selects is not recognisably a copy of the block.
    """
    match = difflib.SequenceMatcher(None, expected, haystack, autojunk=False)
    longest = match.find_longest_match(0, len(expected), 0, len(haystack))
    start = max(0, longest.b - longest.a)
    region = haystack[start : start + len(expected)]
    if difflib.SequenceMatcher(None, expected, region, autojunk=False).ratio() < MIN_SIMILARITY:
        return None
    return region


def report_diff(expected: list[str], actual: list[str] | None, label: str) -> str:
    if actual is None:
        return "  block absent\n"
    lines = difflib.unified_diff(expected, actual, fromfile="expected", tofile=label, lineterm="")
    return "".join(f"  {line}\n" for line in lines)


def check_crate(root: pathlib.Path, crate: str) -> str | None:
    """Return a failure message for ``crate``, or ``None`` when it passes."""
    script = root / crate / BUILD_SCRIPT
    rel = script.relative_to(root)
    if not (root / crate).is_dir():
        return f"error: {crate}: listed in [workspace].exclude but is not a directory\n"
    try:
        source = script.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as exc:
        return f"error: {crate}: could not read {rel}: {exc}\n"
    expected = canonical()
    haystack = tokens(source)
    if contains_run(haystack, expected):
        return None
    return (
        f"error: {crate}: {rel} does not carry the src/tree_sitter/ header watch\n"
        + report_diff(expected, closest_region(expected, haystack), str(rel))
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--root",
        type=pathlib.Path,
        default=REPO_ROOT,
        help="repository root to check (default: the checkout this script lives in)",
    )
    root = parser.parse_args(argv).root.resolve()
    manifest = root / "Cargo.toml"

    try:
        crates = vendored_crates(manifest.read_text(encoding="utf-8"))
    except (OSError, ManifestError, tomllib.TOMLDecodeError) as exc:
        sys.stderr.write(
            f"error: could not derive the vendored crate list from {manifest}: {exc}\n"
        )
        return 2

    failures = [msg for crate in crates if (msg := check_crate(root, crate))]
    if failures:
        sys.stderr.writelines(failures)
        sys.stderr.write(
            "\nEach vendored grammar's bindings/rust/build.rs must watch every file\n"
            "under src/tree_sitter/ with cargo:rerun-if-changed, or a header-only\n"
            "edit leaves a stale parser object behind a fresh `build inputs:`\n"
            "digest (#1773). The block is hand-maintained — a regeneration does\n"
            "not restore it. Re-apply it from a sibling crate, or if the change\n"
            "is deliberate, update CANONICAL_BLOCK in utils/check-grammar-build-watch.py\n"
            "and every build.rs together (#1778).\n"
        )
        return 1

    print(f"grammar-build-watch: OK ({len(crates)} vendored crate(s) watch src/tree_sitter/)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
