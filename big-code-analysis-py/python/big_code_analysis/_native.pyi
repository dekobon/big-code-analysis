"""Type stubs for the compiled ``big_code_analysis._native`` extension.

Kept in lockstep with ``src/lib.rs`` by hand — PyO3 does not generate
stubs today. The public ``big_code_analysis.__init__`` re-exports
every name from the compiled extension listed here, so callers can
``from big_code_analysis import analyze`` and have it resolve under
``mypy --strict``. Pure-Python helpers (e.g. ``flatten_spaces``
from ``_flatten.py``) are also re-exported from ``__init__`` and
carry their own inline type annotations.
"""

from __future__ import annotations

import os
from collections.abc import Iterable, Sequence
from typing import Any, Literal

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

    The class is frozen (immutable) and implements ``__eq__`` /
    ``__hash__`` / ``__repr__`` over **all three** of
    ``(path, error, error_kind)``, so callers may put errors in
    ``set`` / ``dict`` keys to deduplicate. Two failures of the
    same kind on the same path but with differing ``error``
    messages remain distinct under set membership — bucket on
    ``(r.path, r.error_kind)`` explicitly if message drift across
    runs (locale, OS version) is undesirable for the dedup key.

    Not a subclass of :class:`Exception`.

    Taxonomy notes for ``error_kind``:

    * ``"UnsupportedLanguage"`` — file extension and shebang /
      emacs-mode resolution both came up empty, or the upstream
      language is disabled in this build.
    * ``"ParseError"`` — the tree-sitter parser failed, or
      (forward-looking) a future strict-parse mode rejected
      the input. Also the bucket for internal JSON-serialisation
      failures of the resulting ``FuncSpace`` (rare; reserved
      upstream); the error message is prefixed with ``"internal:
      serialization error: "`` in that case (the synthetic
      analyze_batch errors share the same ``"internal:
      <subkind>: <detail>"`` shape). A retry classifier
      keyed on ``error_kind`` cannot distinguish a real parse
      failure from a serialisation failure — inspect the
      ``error`` string for the prefix when the distinction
      matters (serialisation failures are NOT recoverable by
      re-reading the file; parse failures *may* be, with a
      future strict-parse toggle).
    * ``"IoError"`` — the most common kind: ``std::fs::read``
      failed. Also folds in non-UTF-8 path errors (the path
      cannot be encoded as a ``FuncSpace.name``); the issue spec
      pins the taxonomy at three kinds, so the path-encoding
      case is surfaced here rather than as a distinct value.

    For ``"IoError"`` instances the underlying OS error code (when
    available) is preserved in the ``error`` string via Rust's
    ``std::io::Error`` default formatting (``"<msg> (os error
    <N>)"`` on Unix). Parse it with ``re.search(r"\\(os error
    (\\d+)\\)$", err.error)`` if you need ``errno`` for retry
    classification — single-file :func:`analyze` raises a typed
    :class:`OSError` subclass instead (e.g. ``FileNotFoundError``,
    ``PermissionError``), which is the recommended path when
    structured error dispatch matters.
    """

    @property
    def path(self) -> str:
        """Caller-supplied path that triggered the failure."""

    @property
    def error(self) -> str:
        """Human-readable failure message. See class docstring for
        ``error_kind``-specific formatting notes (notably the
        ``(os error N)`` errno suffix on ``"IoError"`` entries).
        """

    @property
    def error_kind(self) -> Literal["UnsupportedLanguage", "ParseError", "IoError"]:
        """Closed taxonomy discriminator — see class docstring."""

    def __init__(
        self,
        path: str,
        error: str,
        error_kind: Literal["UnsupportedLanguage", "ParseError", "IoError"],
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
    metrics: Sequence[str] | None = None,
) -> list[dict[str, Any] | AnalysisError]:
    """Compute metrics for every path in ``paths``.

    Returns a list with one element per yielded path, in the same
    order as the input iterable, so ``zip(paths, results)`` lines
    up by index. Each element is either:

    * a ``dict`` matching :func:`analyze`'s output shape, or
    * an :class:`AnalysisError` describing the per-file failure.

    The function **never raises on per-file errors** — a missing
    file, an unknown extension, or a parser failure becomes an
    :class:`AnalysisError` in the matching result slot instead. It
    still raises on *programmer* errors:

    * ``TypeError`` if ``paths`` is not iterable, or an element is
      not ``str``/``os.PathLike[str]``. Note that this aborts the
      whole call: any successful results computed before the bad
      element are discarded (the function does not return a
      partial list).
    * ``ValueError`` if ``metrics`` is an explicitly empty
      sequence. ``None`` (the default) is fine and means "all".

    There is a third raise path that is **not** a programmer
    error: any exception raised by the input iterator itself
    (e.g. a generator that ``raise``s mid-yield, or a custom
    container whose ``__len__`` raises a non-``TypeError``) also
    propagates out and discards results computed so far. The
    *per-file* never-raise guarantee covers the analysis of
    each yielded path — not the act of yielding the paths in
    the first place. Wrap your generator with a guard (or
    materialise to a list first) if you need the partial
    results preserved on a yield-time exception.

    ``paths`` is consumed lazily, so generators work — only the
    yielded paths are materialised on the Rust side. ``metrics``
    accepts any ``Sequence[str]`` (list, tuple, …); the kwarg is
    reserved for the per-metric selection work landing in a
    follow-up phase, validated (empty rejected) but not yet
    threaded through to the analysis.

    Unlike :func:`analyze`, ``analyze_batch`` runs with the
    ``is_generated`` walker filter **off** so every input position
    yields either a ``dict`` or an :class:`AnalysisError` (never
    ``None``). Call :func:`analyze` per-file with the default
    ``skip_generated=True`` if you need the CLI walker's skip
    behaviour. ``exclude_tests``, ``allow_lossy_path``, and
    ``skip_generated`` are all hardcoded today (a future phase may
    expose them as kwargs); the bridge runs with ``exclude_tests=False``,
    ``allow_lossy_path=False``, and ``skip_generated=False``. The
    ``skip_generated=False`` choice is the inverse of
    :func:`analyze`'s default — migrating
    ``[bca.analyze(p) for p in paths]`` to
    ``bca.analyze_batch(paths)`` changes generated-file handling.

    The GIL is released across each file's read + tree-sitter
    parse via PyO3's ``Python::detach``, so a multi-threaded
    caller wrapping ``analyze_batch`` (or per-file ``analyze``)
    in ``concurrent.futures.ThreadPoolExecutor.map`` actually
    parallelises the heavy work. There is no built-in concurrency
    inside ``analyze_batch`` itself — the entry point is a
    sequential sweep — but the GIL release means other Python
    threads in the process are not blocked for the duration.
    """

def language_for_file(path: str | os.PathLike[str], /) -> str | None:
    """Return the language name :func:`analyze` would dispatch for ``path``.

    Resolves through the same ``big_code_analysis::guess_language``
    pipeline :func:`analyze` uses: the path extension wins when
    recognised, otherwise the file's leading window is inspected for
    a ``#!`` shebang (``#!/usr/bin/env python``, ``#!/bin/bash``, …)
    or an emacs ``-*- mode: … -*-`` declaration. Returns ``None``
    only when none of those signals resolve.

    Reads the file before inspection (parity with :func:`analyze`,
    #318). The previous extension-only ``language_for_file`` could
    return ``None`` for an extension-less shebang script while
    :func:`analyze` on the same path succeeded — that asymmetry is
    closed at the cost of dropping the prior "Never raises" contract.

    Raises
    ------
    OSError
        For any underlying I/O failure. Dispatches to the canonical
        subclass (``FileNotFoundError``, ``PermissionError``,
        ``IsADirectoryError``, …) based on ``errno``, with
        ``err.errno`` and ``err.filename`` populated — same shape as
        :func:`analyze`. If you need the prior "extension only, never
        raises" semantics for a cheap path-only check, wrap the call
        in ``try / except OSError`` (or pre-check
        ``os.path.exists(path)``) — the extension table itself is
        unchanged.
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
