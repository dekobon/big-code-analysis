#!/usr/bin/env python3
"""check-safety-doc-pin

Guard that the ``tree-sitter`` version cited by the soundness argument
in ``big-code-analysis-py/src/node.rs`` is the version the workspace
actually pins in the root ``Cargo.toml``.

That module doc is the canonical justification for the workspace's only
sanctioned ``unsafe`` block (AGENTS.md, "Rust conventions"), and part of
its argument — that ``Tree``/``Node``/``TreeCursor`` are ``Send + Sync``,
that ``Node<'tree>`` is ``(ffi::TSNode, PhantomData<&'tree ()>)``, that
``Tree::edit`` takes ``&mut self`` — is a claim about a *specific*
release. The version literal is what makes the claim checkable: a reader
can open that version of the crate and confirm it. When the pin moves and
the literal does not, the argument keeps reading as verified while
silently describing a crate nobody is compiling against. That is #1057:
the doc said ``=0.26.9`` two patch releases after the pin had moved.

The gate does not verify the *argument* — no gate can. It forces a diff
on that line at bump time, which is the prompt to re-read the argument.

Two directions are enforced:

* every backticked ``=X.Y.Z`` literal in the module doc equals the pin;
* at least one such literal is present. Dropping the literal in favour of
  a version-free phrase ("under the pinned version") was considered and
  declined in #1057: it removes the stale number by making the claim
  unfalsifiable at read time, which moves the staleness from visible to
  invisible rather than fixing it.

Scope is deliberately one file. Two other places name this pin in
prose — the runtime-exemption rationale in ``AGENTS.md`` and the same
rationale echoed in ``utils/check-excluded-manifests.py`` — but both sit
in text that also cites *grammar* pins illustratively (``=0.23.5``,
``=0.26.10``), so a literal-scanning rule there would fail on versions
that are supposed to differ. In ``node.rs`` an ``=X.Y.Z`` literal can
only mean the tree-sitter pin, which is what makes the rule crisp here.

See AGENTS.md "Validation gates" for the policy this enforces.
"""

from __future__ import annotations

import pathlib
import re
import sys
import tomllib

# `parents[1]`, not `parent`: these gates live in `utils/` but every
# path they read is anchored at the repository root.
REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
ROOT_MANIFEST = REPO_ROOT / "Cargo.toml"
SAFETY_DOC = REPO_ROOT / "big-code-analysis-py" / "src" / "node.rs"

# The dependency whose pin the safety argument is about.
PINNED_CRATE = "tree-sitter"

# A backticked exact-version requirement, e.g. `=0.26.12`. Backticks are
# required so ordinary prose containing a version-shaped substring is not
# swept in.
CITED_VERSION = re.compile(r"`=(\d+\.\d+\.\d+)`")


class ManifestError(Exception):
    """The root manifest did not carry a readable ``tree-sitter`` pin."""


def module_doc_lines(text: str) -> list[tuple[int, str]]:
    """Return every ``//!`` line in the file as ``(line_no, body)``.

    Line numbers are 1-based and file-relative, so a failure can name the
    offending line directly.

    There is deliberately **no** stop condition. The first version of this
    gate collected only the file's leading block, skipping blank lines and
    inner attributes so a ``#![allow(…)]`` between doc paragraphs did not
    truncate the scan — but that handled a *single-line* attribute only. A
    multi-line one leaves its continuation and ``)]`` lines looking like
    ordinary code, so the block ended at the attribute and every citation
    below the cut became invisible while the gate printed OK (#1345). Both
    directions were live: a stale citation below the cut passed silently,
    and a file whose only citation sat below it failed the "no longer
    cites" branch while being correct.

    Tracking bracket depth would fix that by adding a lexer to a script
    whose whole value is being obvious. Scanning the file entire fixes it
    by leaving nothing to truncate. ``//!`` is only legal as a module doc,
    so the rule this yields — *any* ``=X.Y.Z`` in a module doc in this one
    file is the pin — is the rule the gate wants anyway. ``///`` item docs
    and code stay out of scope, which is what the leading-block form was
    there for.

    The residual imprecision is a ``//!``-shaped line inside a string
    literal, which this would read as a citation. That is the acceptable
    direction: it fails loudly on a correct file rather than passing
    quietly on a stale one, and nothing in ``node.rs`` has ever had one.
    """
    out: list[tuple[int, str]] = []
    for line_no, raw in enumerate(text.splitlines(), start=1):
        line = raw.strip()
        if line.startswith("//!"):
            out.append((line_no, line[len("//!") :].strip()))
    return out


def cited_versions(doc: list[tuple[int, str]]) -> list[tuple[int, str]]:
    """Return every ``(line_no, version)`` the module doc cites."""
    return [
        (line_no, match.group(1))
        for line_no, body in doc
        for match in CITED_VERSION.finditer(body)
    ]


def pinned_version(manifest_text: str) -> str:
    """Return the exact version ``[workspace.dependencies]`` pins.

    Accepts both the string form (``tree-sitter = "=0.26.12"``) and the
    table form (``tree-sitter = { version = "=0.26.12", … }``), so a
    future manifest restructure does not read as a missing pin.

    Raises :class:`ManifestError` if the entry is absent or is not an
    exact ``=X.Y.Z`` requirement — a caret range would make the cited
    literal unverifiable, which is the same defect from the other end.
    """
    data = tomllib.loads(manifest_text)
    entry = data.get("workspace", {}).get("dependencies", {}).get(PINNED_CRATE)
    if isinstance(entry, dict):
        entry = entry.get("version")
    if not isinstance(entry, str):
        raise ManifestError(
            f"[workspace.dependencies].{PINNED_CRATE} is missing or is not "
            f"a version string (got {entry!r})"
        )
    match = re.fullmatch(r"=\s*(\d+\.\d+\.\d+)", entry.strip())
    if match is None:
        raise ManifestError(
            f"[workspace.dependencies].{PINNED_CRATE} = {entry!r} is not an "
            f"exact `=X.Y.Z` pin"
        )
    return match.group(1)


def main() -> int:
    try:
        pin = pinned_version(ROOT_MANIFEST.read_text(encoding="utf-8"))
    except (OSError, ManifestError) as exc:
        sys.stderr.write(
            f"error: could not read the {PINNED_CRATE} pin from "
            f"{ROOT_MANIFEST}: {exc}\n"
        )
        return 2

    try:
        doc = module_doc_lines(SAFETY_DOC.read_text(encoding="utf-8"))
    except OSError as exc:
        sys.stderr.write(f"error: could not read {SAFETY_DOC}: {exc}\n")
        return 2

    rel = SAFETY_DOC.relative_to(REPO_ROOT)
    cited = cited_versions(doc)

    if not cited:
        sys.stderr.write(
            f"error: {rel} no longer cites a `{PINNED_CRATE}` version in "
            f"its module doc\n"
            f"       The soundness argument for this workspace's only\n"
            f"       sanctioned `unsafe` block makes claims about a\n"
            f"       specific release; naming it is what lets a reader\n"
            f"       check them. Restore the literal as `={pin}` (see\n"
            f"       #1057 — the version-free phrasing was declined).\n"
        )
        return 1

    stale = [(line_no, version) for line_no, version in cited if version != pin]
    if stale:
        sys.stderr.write(
            f"error: {rel} cites a {PINNED_CRATE} version the workspace "
            f"does not pin\n"
        )
        for line_no, version in stale:
            sys.stderr.write(f"  {rel}:{line_no}: cites `={version}`, pin is `={pin}`\n")
        sys.stderr.write(
            f"\nThe module doc is the canonical soundness argument for the\n"
            f"only `unsafe` block in this workspace, and it reasons about\n"
            f"the pinned {PINNED_CRATE} release specifically. Re-read that\n"
            f"argument against {pin} — `Tree(NonNull<ffi::TSTree>)`,\n"
            f"`Node<'tree>(ffi::TSNode, PhantomData<&'tree ()>)`,\n"
            f"`Tree::edit(&mut self)`, `Send + Sync` — and then update the\n"
            f"literal. See #1057.\n"
        )
        return 1

    print(f"safety-doc-pin: OK ({PINNED_CRATE} `={pin}`, {len(cited)} citation(s))")
    return 0


if __name__ == "__main__":
    sys.exit(main())
