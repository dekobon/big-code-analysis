"""Type stubs for the compiled ``big_code_analysis._native`` extension.

Kept in lockstep with ``src/lib.rs`` by hand — PyO3 does not generate
stubs today. The public ``big_code_analysis.__init__`` re-exports
every name listed here, so callers can ``from big_code_analysis
import analyze`` and have it resolve under ``mypy --strict``.
"""

from __future__ import annotations

import os
from typing import Any, Iterable, Literal

__version__: str

class UnsupportedLanguageError(ValueError):
    """Raised when a file extension or explicit language is unknown."""

class ParseError(ValueError):
    """Raised when the tree-sitter parser fails on the supplied source."""

class AnalysisError:
    """Structured per-file failure returned by :func:`analyze_batch`.

    Instances are **returned**, never raised — :func:`analyze_batch`
    interleaves them with successful ``dict`` results so a single
    pipeline failure does not break the rest of the batch. Use
    ``isinstance(r, AnalysisError)`` as the discriminator:

    .. code-block:: python

        for r in bca.analyze_batch(paths):
            if isinstance(r, bca.AnalysisError):
                log.warning("%s (%s): %s", r.path, r.error_kind, r.error)
            else:
                process(r)

    ``path`` is the caller-supplied path as a string. The class is
    frozen (immutable) and implements ``__eq__`` / ``__hash__`` /
    ``__repr__``, so callers may put errors in ``set`` / ``dict``
    keys to deduplicate. It is **not** a subclass of ``Exception``.
    """

    path: str
    error: str
    error_kind: Literal["UnsupportedLanguage", "ParseError", "IoError"]

    def __init__(
        self,
        path: str,
        error: str,
        error_kind: Literal["UnsupportedLanguage", "ParseError", "IoError"],
        /,
    ) -> None: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...

def analyze(
    path: str | os.PathLike[str],
    /,
    *,
    exclude_tests: bool = False,
    allow_lossy_path: bool = False,
    skip_generated: bool = True,
) -> dict[str, Any] | None:
    """Compute metrics for the file at ``path``.

    Returns a ``dict`` matching the JSON emitted by ``bca metrics
    --output-format json`` for the same file at the ``FuncSpace``
    serialisation layer: identical field order (``name``,
    ``start_line``, ``end_line``, ``kind``, ``spaces``, ``metrics``),
    identical numeric formatting, identical shape. Both sides
    serialise through ``serde_json::to_string``; the bindings parse
    that JSON with ``json.loads``, which preserves insertion order
    on CPython 3.7+.

    Returns ``None`` when ``skip_generated=True`` (the default) and
    the file's leading window matches the CLI walker's
    ``is_generated`` predicate — see ``skip_generated`` below.
    Callers must therefore handle the optional return:

    .. code-block:: python

        result = bca.analyze(path)
        if result is None:
            # File is marked `@generated` / `DO NOT EDIT` /
            # `GENERATED CODE`; the CLI walker would skip it too.
            continue
        process(result)

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

    Pass ``allow_lossy_path=True`` to mirror the CLI's non-UTF-8
    path handling: bytes that are not valid UTF-8 are replaced
    with U+FFFD (Unicode REPLACEMENT CHARACTER) via
    ``Path::to_string_lossy`` before being written into the
    returned ``FuncSpace.name``. The default (``False``) keeps the
    strict policy: non-UTF-8 paths raise :class:`ValueError` so
    ``name`` remains a round-trippable identifier and cannot
    silently collapse two distinct paths onto the same lossy key
    (#316).

    Pass ``skip_generated=False`` to bypass the CLI's
    ``is_generated`` walker filter. The default (``True``) matches
    the CLI walker: a file whose leading ~5 KiB / first 50 lines
    carry an ``@generated`` / ``DO NOT EDIT`` / ``GENERATED CODE``
    marker (case-insensitive for ``@generated``) returns ``None``
    without paying parse cost. The check runs *before* language
    inference, so a generated file with an unrecognised extension
    still returns ``None`` rather than raising
    :class:`UnsupportedLanguageError` (#317).

    Parity with ``bca metrics --output-format json`` is now exact
    at the ``FuncSpace`` boundary in the default configuration:

    * Language detection mirrors the CLI's ``guess_language``: the
      path extension wins when recognised, otherwise the first
      line is checked for a ``#!`` shebang (``#!/usr/bin/env
      python``, ``#!/bin/bash``, …) and the leading / trailing
      lines for an emacs ``-*- mode: … -*-`` (or vim modeline)
      declaration. An extension-less, non-generated script with no
      detectable interpreter still raises
      :class:`UnsupportedLanguageError`.
    * Non-UTF-8 path bytes match the CLI byte-for-byte when
      ``allow_lossy_path=True``; the default still raises
      ``ValueError`` so the strict identifier contract is opt-out,
      not opt-in.
    * Generated files (CLI's ``is_generated`` filter) are skipped
      on both sides when ``skip_generated=True`` (the default):
      the bindings return ``None``, the CLI walker emits no
      record. Pass ``skip_generated=False`` on both sides to opt
      out symmetrically.

    Raises
    ------
    UnsupportedLanguageError
        If ``path``'s extension is unknown AND no shebang or
        emacs-mode declaration resolves to a supported language.
        Not raised when ``skip_generated=True`` and the file
        matches the ``is_generated`` predicate — ``None`` is
        returned instead.
    ParseError
        If the tree-sitter parser fails on the source.
    ValueError
        If ``path`` is not valid UTF-8 and ``allow_lossy_path`` is
        ``False`` (the default). Pass ``allow_lossy_path=True`` to
        opt into U+FFFD substitution and match the CLI.
        (``UnsupportedLanguageError`` and ``ParseError`` are also
        ``ValueError`` subclasses, so a single ``except
        ValueError`` covers all three.)
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

def analyze_batch(
    paths: Iterable[str | os.PathLike[str]],
    /,
    *,
    metrics: list[str] | None = None,
) -> list[dict[str, Any] | AnalysisError]:
    """Compute metrics for every path in ``paths``.

    Returns a list of length ``len(paths)`` in the **same order** as
    the input iterable. Each element is either:

    * a ``dict`` matching :func:`analyze`'s output shape, or
    * an :class:`AnalysisError` describing the per-file failure.

    The function **never raises on per-file errors** — a missing
    file, an unknown extension, or a parser failure becomes an
    :class:`AnalysisError` in the matching result slot instead. It
    still raises on *programmer* errors:

    * ``TypeError`` if ``paths`` is not iterable, or an element is
      not ``str``/``os.PathLike[str]``.
    * ``ValueError`` if ``metrics`` is an explicitly empty list. A
      ``None`` ``metrics`` (the default) is fine and means "all".

    ``paths`` is consumed lazily, so generators work — only the
    yielded paths are materialised on the Rust side.

    Unlike :func:`analyze`, ``analyze_batch`` runs with the
    ``is_generated`` walker filter **off** so every input position
    yields either a ``dict`` or an :class:`AnalysisError` (never
    ``None``). Call :func:`analyze` per-file with the default
    ``skip_generated=True`` if you need the CLI walker's skip
    behaviour.

    ``metrics`` is reserved for the per-metric selection work
    landing in a follow-up phase; the kwarg is accepted (and
    validated) today so existing call sites do not need to change
    when the selection plumbing arrives.

    There is no built-in parallelism — the recommended pattern is
    ``concurrent.futures.ThreadPoolExecutor.map(bca.analyze, paths)``
    when the GIL release inside the Rust parser yields enough
    headroom for your workload.
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
