"""Type stubs for the compiled ``big_code_analysis._native`` extension.

Kept in lockstep with ``src/lib.rs`` by hand — PyO3 does not generate
stubs today. The public ``big_code_analysis.__init__`` re-exports
every name listed here, so callers can ``from big_code_analysis
import analyze`` and have it resolve under ``mypy --strict``.
"""

from __future__ import annotations

import os
from typing import Any

__version__: str

class UnsupportedLanguageError(ValueError):
    """Raised when a file extension or explicit language is unknown."""

class ParseError(ValueError):
    """Raised when the tree-sitter parser fails on the supplied source."""

def analyze(
    path: str | os.PathLike[str],
    /,
    *,
    exclude_tests: bool = False,
) -> dict[str, Any]:
    """Compute metrics for the file at ``path``.

    The returned ``dict`` matches the JSON emitted by ``bca metrics
    --output-format json`` for the same file at the ``FuncSpace``
    serialisation layer: identical field order (``name``,
    ``start_line``, ``end_line``, ``kind``, ``spaces``, ``metrics``),
    identical numeric formatting, identical shape. Both sides
    serialise through ``serde_json::to_string``; the bindings parse
    that JSON with ``json.loads``, which preserves insertion order
    on CPython 3.7+.

    Pass ``exclude_tests=True`` to mirror the CLI's global
    ``--exclude-tests`` flag (``bca metrics --exclude-tests
    --output-format json``). The bindings then thread
    ``MetricsOptions::default().with_exclude_tests(True)`` into the
    analysis: language checkers that override
    ``should_skip_subtree`` (today: Rust — ``#[test]``,
    ``#[cfg(test)]``, ``#[tokio::test]``, ``#[rstest]``,
    ``#![cfg(test)]``) prune the matching subtrees before any
    per-metric ``compute`` runs. Languages without that override
    ignore the flag, matching CLI behaviour.

    Parity is exact **only when** every condition below holds; phase-1
    of the bindings (#265) intentionally scopes the parity claim to
    the FuncSpace boundary and defers the surrounding CLI behaviours
    to follow-up issues:

    * Language detection mirrors the CLI's ``guess_language``: the
      path extension wins when recognised, otherwise the first
      line is checked for a ``#!`` shebang (``#!/usr/bin/env
      python``, ``#!/bin/bash``, …) and the leading / trailing
      lines for an emacs ``-*- mode: … -*-`` (or vim modeline)
      declaration. An extension-less script with no detectable
      interpreter still raises :class:`UnsupportedLanguageError`.
    * Non-UTF-8 path bytes raise ``ValueError`` here (see Raises);
      the CLI substitutes U+FFFD via ``Path::to_string_lossy`` —
      see #316.
    * Generated files (CLI's ``is_generated`` filter) are NOT
      skipped by the bindings; the CLI emits no record for files
      marked ``@generated`` / ``DO NOT EDIT`` / ``GENERATED CODE``,
      the bindings emit a populated ``FuncSpace`` — see #317.

    Raises
    ------
    UnsupportedLanguageError
        If ``path``'s extension is unknown AND no shebang or
        emacs-mode declaration resolves to a supported language.
    ParseError
        If the tree-sitter parser fails on the source.
    ValueError
        If ``path`` is not valid UTF-8 and cannot be used as a
        ``FuncSpace`` name. (``UnsupportedLanguageError`` and
        ``ParseError`` are also ``ValueError`` subclasses, so a
        single ``except ValueError`` covers all three.)
    OSError
        For any underlying I/O failure. Dispatches to the canonical
        subclass (``FileNotFoundError``, ``PermissionError``,
        ``IsADirectoryError``, …) based on ``errno``, with
        ``err.errno`` and ``err.filename`` populated.
    """

def analyze_source(
    code: str | bytes | bytearray,
    language: str,
    /,
    *,
    exclude_tests: bool = False,
) -> dict[str, Any]:
    """Compute metrics for an in-memory source buffer.

    ``language`` is a name returned by :func:`supported_languages`
    (case-insensitive). ``code`` may be ``str`` (encoded as UTF-8),
    ``bytes``, or ``bytearray``. The returned ``dict`` matches the
    ``FuncSpace`` shape used by :func:`analyze`, with ``name`` set
    to ``None`` because no path is associated with an in-memory
    buffer. ``exclude_tests`` mirrors ``bca metrics
    --exclude-tests`` — see :func:`analyze` for the full parity
    contract and the language-checker semantics it triggers.

    Raises
    ------
    UnsupportedLanguageError
        If ``language`` is not a known language name.
    ParseError
        If the tree-sitter parser fails on the source.
    ValueError
        If ``code`` is a ``str`` containing unpaired surrogates
        (legal in CPython, not valid UTF-8), or is not one of the
        accepted buffer types.
    """

def language_for_file(path: str | os.PathLike[str], /) -> str | None:
    """Return the language name for ``path``'s extension, or ``None``.

    Never raises — agrees with the Rust
    ``big_code_analysis::get_language_for_file`` for every supported
    extension.
    """

def supported_languages() -> list[str]:
    """Return the supported language names, in declaration order."""

def language_extensions(language: str, /) -> list[str]:
    """Return the file extensions registered for ``language``.

    Raises
    ------
    UnsupportedLanguageError
        If ``language`` is not a known language name.
    """
