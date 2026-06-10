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
from datetime import datetime
from typing import Any, Literal

from ._types import FuncSpaceDict

__version__: str

#: Canonical metric names accepted by the ``metrics=`` kwarg on
#: :func:`analyze`, :func:`analyze_source`, and :func:`analyze_batch`.
#:
#: Each entry corresponds to one variant of the upstream
#: ``big_code_analysis::Metric`` enum and to the metric's JSON output
#: key (``"nexits"`` for the exit-point metric, etc.); the parsing
#: layer also accepts the alias ``"exit"`` for backwards-compatibility
#: with the ``Metric`` Display spelling, but ``METRIC_NAMES`` itself
#: advertises only the canonical JSON-key spelling. The tuple is
#: immutable and alphabetically sorted; callers can ``assert name in
#: bca.METRIC_NAMES`` to validate user input client-side.
METRIC_NAMES: tuple[str, ...]

class UnsupportedLanguageError(ValueError):
    """Raised when a file extension or explicit language is unknown."""

class ParseError(ValueError):
    """Raised when the tree-sitter parser fails on the supplied source."""

class VcsError(ValueError):
    """Base class for the change-history (VCS) surface errors (#624).

    Subclasses :class:`ValueError`, so a single ``except ValueError``
    (or ``except VcsError``) catches every VCS failure raised by
    :func:`vcs_metrics`, :func:`vcs_trend`, :func:`vcs_jit`, and
    ``analyze(..., vcs=True)``. The three named subclasses below carve
    out the triggers a caller most plausibly branches on; the bare
    ``VcsError`` is itself raised for client-input option failures (a
    malformed window / timestamp / formula / file-type scope /
    bus-factor threshold / bot pattern / trend point count), where the
    message names the offending value.
    """

class NotARepositoryError(VcsError):
    """Raised when a path is not inside a supported VCS working tree.

    The variant a caller most plausibly branches on: "not a repo →
    skip this directory" rather than crash. ``analyze(..., vcs=True)``
    never raises this — a non-repository file silently yields no
    ``vcs`` block.
    """

class InvalidRevisionError(VcsError):
    """Raised when a ``reference`` / ``commit`` cannot be resolved."""

class InvalidDiffError(VcsError):
    """Raised when the ``diff`` passed to :func:`vcs_jit` is malformed."""

class VcsEnvironmentError(VcsError):
    """Raised when a VCS operation fails for an environment reason.

    Opening / discovering the repository (other than "not a repo"),
    walking history, diffing, applying ``.mailmap``, blaming, or
    persistent-cache I/O. These mirror the ``500`` (rather than
    ``400``) responses the web crate returns for the same
    ``vcs::Error`` variants (``is_client_input == false``, #641).
    """

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
    metrics: Sequence[str] | None = None,
    vcs: bool = False,
    vcs_per_function: bool = False,
) -> FuncSpaceDict | None:
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

    Pass ``metrics=`` to compute only a subset of the metric suite
    (#268). ``None`` (the default) computes everything. Each
    element is a canonical metric name from :data:`METRIC_NAMES`
    (strict lowercase). The empty list raises ``ValueError``; an
    unknown name raises ``ValueError`` with the valid list in the
    message. Validation runs **before** the file is read, so a bad
    selection raises without paying I/O cost. Unrequested metrics
    are **absent** from the result dict (not present with ``None``
    placeholders); selecting a derived metric (``"mi"``, ``"wmc"``)
    pulls in its dependencies automatically:

    * ``"mi"`` → also computes ``"loc"``, ``"cyclomatic"``,
      ``"halstead"``.
    * ``"wmc"`` → also computes ``"cyclomatic"``, ``"nom"``.

    The ``"exit"`` Metric-Display spelling is accepted as an alias
    for the canonical JSON key ``"nexits"``; both produce a
    ``"nexits"`` key in the result. Duplicates are silently
    collapsed.

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

    Pass ``vcs=True`` to attach a ``"vcs"`` block (change-history
    metrics) to the file's ``metrics`` from a one-shot history walk of
    the enclosing git repository (issue #328). A ``hotspot_score``
    (complexity × recent churn) is included only when ``cyclomatic`` is
    among the computed metrics (it is, unless restricted via
    ``metrics=``). The block is omitted when the file is untracked,
    binary, or outside any repository. For ranking a whole repository,
    prefer :func:`vcs_metrics`, which walks history once.

    Pass ``vcs_per_function=True`` to attach a ``"vcs"`` block to **each
    nested function / method / class space** (not just the file-level
    space) from a single ``git blame`` of the file (issue #329). This
    mirrors the CLI's ``bca metrics --vcs-per-function`` flag: every
    descendant space's ``metrics`` gains a ``"vcs"`` key whose shape is
    byte-identical to the file-level block, with a per-function
    ``hotspot_score`` derived from that space's own cyclomatic sum. The
    file is blamed exactly once and the result is shared across all of its
    spans. ``vcs_per_function`` is independent of ``vcs``: set ``vcs=True``
    for the file-level block, ``vcs_per_function=True`` for the
    per-function blocks, or both. Per-function blocks are omitted (the AST
    metrics still emit) when the file is outside any git working tree, is
    untracked, lies outside the work tree, or is otherwise unblameable —
    matching the CLI's per-file graceful degradation. A file with no
    nested spaces is returned unchanged.
    """

def vcs_metrics(
    repo_path: str | os.PathLike[str],
    /,
    *,
    long_window: str | None = None,
    recent_window: str | None = None,
    top: int | None = None,
    reference: str | None = None,
    risk_formula: str | None = None,
    file_types: Sequence[str] | str | None = None,
    full_history: bool = False,
    include_merges: bool = False,
    follow_renames: bool = True,
    exclude_bots: bool = True,
    bot_pattern: str | None = None,
    as_of: datetime | str | None = None,
    emit_author_details: bool = False,
    include_deleted: bool = False,
    bus_factor_threshold: float | None = None,
    no_cache: bool = False,
    cache_dir: str | os.PathLike[str] | None = None,
) -> dict[str, Any]:
    """Rank the files in a git repository by change-history risk.

    The programmatic analogue of ``bca vcs`` (issue #328). ``repo_path``
    is any path inside the working tree. Returns a ``dict`` with
    ``long_window_days``, ``recent_window_days``, ``risk_score_version``,
    ``vcs_schema_version``, ``truncated_shallow_clone``, and a ``files``
    list of per-file metric dicts (path plus the flat ``vcs`` fields)
    ranked by descending ``risk_score``.

    Windows accept ``12mo`` / ``2y`` / ``8w`` / ``90d`` or ISO 8601
    (``P1Y``). ``risk_formula`` is ``"weighted"`` (default) or
    ``"percentile"``. ``file_types`` scopes which files are ranked:
    ``"metrics"`` (default — only files bca has metrics for), ``"all"``
    (every tracked text file), a comma-separated extension allow-list
    (``"rs,py"``), or a sequence of extensions (``["rs", "py"]``).
    ``bus_factor_threshold`` (default ``0.5``) sets the
    coverage/abandonment fraction for the bus-factor flag. ``as_of``
    pins the reference "now" for reproducible snapshots; it accepts a
    ``datetime`` or a string (RFC 3339 / ``@unix`` / git date).

    The persistent change-history cache (issue #334) reuses prior work on
    an unchanged tree and walks only new commits when ``HEAD`` advances.
    Pass ``no_cache=True`` to skip it, or ``cache_dir`` (a ``str`` or
    ``os.PathLike``) to override its location (default: the platform cache
    directory).

    Raises
    ------
    NotARepositoryError
        When ``repo_path`` is not inside a git working tree.
    VcsError
        For a malformed window / timestamp / formula / file-type scope /
        bus-factor threshold (the option-validation base; all VCS
        exceptions subclass :class:`VcsError`, itself a ``ValueError``).
    VcsEnvironmentError
        When walking history, diffing, or cache I/O fails.
    """

def vcs_trend(
    repo_path: str | os.PathLike[str],
    /,
    *,
    points: int = 12,
    span: str | None = None,
    top: int | None = None,
    top_deltas: int | None = None,
    long_window: str | None = None,
    recent_window: str | None = None,
    reference: str | None = None,
    risk_formula: str | None = None,
    file_types: Sequence[str] | str | None = None,
    full_history: bool = False,
    include_merges: bool = False,
    follow_renames: bool = True,
    exclude_bots: bool = True,
    bot_pattern: str | None = None,
    as_of: datetime | str | None = None,
    emit_author_details: bool = False,
    include_deleted: bool = False,
    bus_factor_threshold: float | None = None,
) -> dict[str, Any]:
    """Sample change-history metrics over time as a per-file trend.

    The programmatic analogue of ``bca vcs trend`` (issue #333).
    ``points`` (>= 2) evenly-spaced samples cover ``span`` (default
    ``12mo``), ending at ``as_of`` (or wall-clock now). Returns a
    ``dict`` with ``trend_schema_version``, ``vcs_schema_version``,
    ``risk_score_version``, the window lengths,
    ``truncated_shallow_clone``, ``as_of_points`` (sample timestamps,
    oldest-first), a ``files`` map from path to a point array aligned to
    ``as_of_points`` (a ``None`` element marks a point where the file did
    not exist), and a ``deltas`` summary splitting the most-``improved``
    and most-``regressed`` files by ``risk_score``.

    Each point re-anchors at the mainline tip of that moment, so it is a
    faithful historical snapshot rather than today's tree windowed
    differently. ``top`` caps how many files the series keeps (by
    most-recent risk); ``top_deltas`` trims each delta list. The other
    knobs match :func:`vcs_metrics`.

    Raises
    ------
    NotARepositoryError
        When ``repo_path`` is not inside a git working tree.
    VcsError
        For a malformed option, or a point count below 2 or above the
        supported maximum (the option-validation base; subclass of
        ``ValueError``).
    VcsEnvironmentError
        When walking history, diffing, or cache I/O fails.
    """

def vcs_jit(
    repo_path: str | os.PathLike[str] | None = None,
    /,
    *,
    commit: str = "HEAD",
    diff: str | None = None,
    long_window: str | None = None,
    recent_window: str | None = None,
    full_history: bool = False,
    include_merges: bool = False,
    follow_renames: bool = True,
    as_of: datetime | str | None = None,
) -> dict[str, Any]:
    """Score a single commit (or an arbitrary diff) for just-in-time risk.

    The programmatic analogue of ``bca vcs jit`` (issues #331 / #580).
    ``repo_path`` is any path inside the working tree; ``commit`` is any
    git revision spelling (default ``"HEAD"``), scored against its first
    parent. Returns a ``dict`` with ``jit_schema_version``,
    ``jit_score_version``, ``source == "commit"`` (the mode discriminator),
    the window lengths, the ordinal composite ``score``, the ``commit``
    block, the ``features`` (size / diffusion / history / experience), and
    the per-group ``contributions``.

    Pass ``diff`` (a unified diff string) to score a bare diff instead of a
    commit. A bare diff carries no author / parent / history, so only the
    size and diffusion groups are computable: the returned dict then has
    ``source == "diff"``, a ``partial_score`` that is **not comparable** to
    a commit ``score``, and **no** history / experience / purpose groups
    (they are absent, not zero, so an unavailable group can never be
    misread as "low risk"). In diff mode ``repo_path`` / ``commit`` and the
    window knobs are ignored.

    Raises
    ------
    NotARepositoryError
        When ``repo_path`` is not a git working tree (commit mode).
    InvalidRevisionError
        When ``commit`` cannot be resolved to a revision.
    InvalidDiffError
        When the supplied ``diff`` is malformed (diff mode).
    VcsError
        For a malformed window / timestamp (the option-validation base;
        subclass of ``ValueError``).
    """

def analyze_source(
    code: str | bytes | bytearray,
    language: str,
    /,
    *,
    exclude_tests: bool = False,
    metrics: Sequence[str] | None = None,
) -> FuncSpaceDict:
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

    Pass ``metrics=`` to compute only a subset of the metric suite
    (#268); see :func:`analyze` for the full contract. ``None``
    (the default) computes everything. Validation runs before the
    tree-sitter parse, so empty / unknown selections raise
    ``ValueError`` immediately.
    """

def analyze_batch(
    paths: Iterable[str | os.PathLike[str]],
    /,
    *,
    exclude_tests: bool = False,
    allow_lossy_path: bool = False,
    skip_generated: bool = True,
    metrics: Sequence[str] | None = None,
) -> list[FuncSpaceDict | AnalysisError]:
    """Compute metrics for every path in ``paths``.

    Returns a list whose elements preserve the input order, so
    ``zip(paths, results)`` lines up by index **when no path is
    skipped**. Each element is either:

    * a ``dict`` matching :func:`analyze`'s output shape, or
    * an :class:`AnalysisError` describing the per-file failure.

    A path that is skipped (``skip_generated=True`` and the file is
    generated) produces **no** element, so with skipping enabled the
    result list can be shorter than the input iterable. Pass
    ``skip_generated=False`` to guarantee one element per input.

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
      sequence, or contains a name not in :data:`METRIC_NAMES`.
      ``None`` (the default) means "compute the full suite".
      Validation runs **before** ``iter(paths)`` — a generator's
      ``__iter__`` is never invoked when ``metrics=`` is invalid,
      so its side effects (and any partial yields) are preserved.

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
    yielded paths are materialised on the Rust side. ``metrics=``
    selects which metrics to compute for every file in the batch
    (#268); see :func:`analyze` for the full contract on canonical
    names, dependency closure (``"mi"`` / ``"wmc"`` auto-pull
    inputs), and the ``"exit"`` / ``"nexits"`` alias. Unrequested
    metrics are absent from each result dict.

    ``exclude_tests``, ``allow_lossy_path``, and ``skip_generated``
    mirror the keyword-only kwargs on :func:`analyze` exactly (#542),
    so migrating ``[bca.analyze(p) for p in paths]`` to
    ``bca.analyze_batch(paths)`` is behaviour-preserving. In
    particular ``skip_generated`` defaults to ``True`` here too: a
    generated file is *skipped* (it yields no element), matching the
    CLI walker and :func:`analyze`'s ``None`` return. This default
    flipped at 2.0 — the pre-2.0 ``analyze_batch`` hardcoded
    ``skip_generated=False`` (always one element per input). Pass
    ``skip_generated=False`` to restore that behaviour.

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

def to_sarif(
    result: FuncSpaceDict
    | None
    | Iterable[FuncSpaceDict | AnalysisError | None],
    /,
    *,
    thresholds: dict[str, float] | None = None,
) -> str:
    """Render a SARIF 2.1.0 JSON document from analysis results.

    ``result`` accepts either a single ``dict`` returned by
    :func:`analyze` / :func:`analyze_source`, a scalar ``None`` (the
    documented return of :func:`analyze` for generated files; yields
    an empty SARIF run), or any iterable yielding such dicts,
    :class:`AnalysisError` instances, and/or ``None`` (the natural
    shape of :func:`analyze_batch`'s return value, or a list
    comprehension over :func:`analyze` which returns ``None`` for
    generated files). ``AnalysisError`` and ``None`` entries are
    skipped silently — they represent files for which no record was
    emitted (either the pipeline could not analyse them, or they
    were classified as generated), not findings.

    Pass ``thresholds={"cyclomatic": 15, "loc.lloc": 200, …}`` to
    drive finding emission. ``thresholds=None`` (the default) is
    equivalent to an empty dict and produces a well-formed SARIF
    document with empty ``results`` and ``rules``. This mirrors the
    CLI's posture (see ``big-code-analysis-cli/src/thresholds.rs``):
    the CLI ships **no built-in defaults**, every check run must
    supply its own thresholds, and the bindings adopt the same
    contract. Accepted threshold names mirror the CLI's
    ``EXTRACTORS`` table — e.g. ``"cognitive"``, ``"cyclomatic"``,
    ``"cyclomatic.modified"``, ``"halstead.volume"``,
    ``"halstead.difficulty"``, ``"halstead.effort"``, ``"loc.sloc"``,
    ``"loc.ploc"``, ``"loc.lloc"``, ``"loc.cloc"``, ``"loc.blank"``,
    ``"nom"``, ``"tokens"``, ``"nexits"``, ``"nargs"``,
    ``"mi.original"``, ``"mi.sei"``, ``"mi.visual_studio"``,
    ``"abc"``, ``"wmc"``, ``"npm"``, ``"npa"``. An unknown name
    raises :class:`ValueError` listing the accepted set, so a
    typo fails fast instead of silently producing an empty run.

    Returns a ``str`` (UTF-8 SARIF JSON). The output is produced by
    the upstream ``big_code_analysis::write_sarif`` writer — the
    same one driving ``bca check -O sarif`` — so the schema URL,
    tool driver name / version, and rule descriptions match the
    CLI byte-for-byte.

    Unit-level (file-scope) findings are emitted for every metric
    whose JSON headline at the file-level ``unit`` space matches
    the CLI's per-space accessor (``loc.*``, ``halstead.*``,
    ``mi.*``, ``nom``, ``nargs``, ``nexits``, ``tokens``, ``abc``,
    ``wmc``, ``npm``, ``npa``). For the three metrics whose CLI
    per-space accessor returns just the unit's own scalar while
    the JSON exposes the aggregate ``sum`` across children —
    ``cyclomatic``, ``cyclomatic.modified``, ``cognitive`` — the
    unit space is skipped (those metrics emit per-function only).
    Unit-level findings carry ``logicalLocations: [{"fullyQualifiedName":
    "<file>"}]``; nameless non-unit spaces carry ``"<unnamed>"`` —
    matching the CLI's ``function_token`` placeholder.

    Raises
    ------
    TypeError
        If ``result`` is not a dict / iterable of dicts, or a
        threshold value is not a number, or a threshold key is not
        a string.
    ValueError
        If a threshold limit is negative or non-finite, or names a
        metric outside the accepted set.
    """
