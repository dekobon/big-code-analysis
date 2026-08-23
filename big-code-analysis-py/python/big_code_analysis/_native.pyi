"""Type stubs for the compiled ``big_code_analysis._native`` extension.

Kept in lockstep with ``src/lib.rs`` by hand — PyO3 does not generate
stubs today — and verified against the compiled extension by
``make py-stubtest`` (``mypy stubtest``), which diffs names,
signatures, and **defaults** so this stub cannot silently drift from
the runtime (#673). The public ``big_code_analysis.__init__`` re-exports
every name from the compiled extension listed here, so callers can
``from big_code_analysis import analyze`` and have it resolve under
``mypy --strict``. Pure-Python helpers (e.g. ``flatten_spaces``
from ``_flatten.py``) are also re-exported from ``__init__`` and
carry their own inline type annotations.
"""

from __future__ import annotations

import os
from collections.abc import Iterable, Iterator, Sequence
from typing import Literal, final

from ._types import (
    AstNodeDict,
    FuncSpaceDict,
    FunctionSpanDict,
    OpsDict,
    SpanDict,
    SuppressionMarkerDict,
)

__version__: str

#: Canonical metric names accepted by the ``metrics=`` kwarg on
#: :func:`analyze`, :func:`analyze_source`, and :func:`analyze_batch`.
#:
#: Each entry corresponds to one variant of the upstream
#: ``big_code_analysis::Metric`` enum and to the metric's JSON output
#: key (``"nexits"`` for the exit-point metric, etc.). The legacy
#: ``"exit"`` parse alias was retired at 2.0, so ``METRIC_NAMES``
#: now lists the only accepted spelling for each metric. The tuple is
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
    ``big_code_analysis.vcs.rank`` / ``.trend`` / ``.commit`` /
    ``.score_diff`` and ``analyze(..., vcs=True)``. The three named
    subclasses below carve
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
    """Raised when the ``diff`` passed to ``vcs.score_diff`` is malformed."""

class VcsEnvironmentError(VcsError):
    """Raised when a VCS operation fails for an environment reason.

    Opening / discovering the repository (other than "not a repo"),
    walking history, diffing, applying ``.mailmap``, blaming, or
    persistent-cache I/O. These mirror the ``500`` (rather than
    ``400``) responses the web crate returns for the same
    ``vcs::Error`` variants (``is_client_input == false``, #641).
    """

@final
class AnalysisFailure:
    """Structured per-file failure returned by :func:`analyze_batch`.

    Instances are **returned**, never raised — :func:`analyze_batch`
    interleaves them with successful ``dict`` results so a single
    pipeline failure does not break the rest of the batch. The class is
    deliberately **not** an ``Exception`` subclass and was renamed from
    ``AnalysisError`` at 2.0 (#614): the ``…Error`` suffix that PEP 8
    reserves for raisable exceptions misled readers into
    ``except bca.AnalysisError:`` (a ``TypeError`` at the ``except``
    site, since it does not inherit ``BaseException``). Use
    ``isinstance(r, AnalysisFailure)`` as the discriminator:

    .. code-block:: python

        for r in bca.analyze_batch(paths):
            if isinstance(r, bca.AnalysisFailure):
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

    # PyO3 exposes the `#[new]` constructor as `__new__` (not
    # `__init__`); the signature mirrors the Rust `py_new`
    # `#[pyo3(signature = (path, error, error_kind))]`. stubtest (#673)
    # verifies these stay in lockstep.
    def __new__(
        cls,
        path: str,
        error: str,
        error_kind: Literal["UnsupportedLanguage", "ParseError", "IoError"],
    ) -> AnalysisFailure: ...
    # `value` is positional-only at runtime (the slot wrapper),
    # matching `object.__eq__`.
    def __eq__(self, value: object, /) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...

@final
class Ast:
    """A parsed source file: the AST plus its source bytes, from one parse.

    Parse a file **once** and draw both metrics and the AST from the same
    parse, instead of parsing twice (once in py-tree-sitter, once in
    :func:`analyze`). Construct with :meth:`parse` (in-memory source) or
    :meth:`from_path` (a file); the handle is immutable and thread-safe, so
    it composes with ``ThreadPoolExecutor`` fan-out like :func:`analyze`.

    Every accessor serializes through the same path the CLI / web surfaces
    use, so :meth:`dump` node shapes are byte-for-byte identical to ``bca
    dump`` / ``/ast`` and :meth:`metrics` matches :func:`analyze_source`.
    """

    # PyO3 `#[staticmethod]` — no `cls`/`self`. The signatures mirror the
    # Rust `#[pyo3(signature = ...)]`; stubtest (#673) verifies the
    # positional-only `/`, keyword-only `*`, and defaults stay in lockstep.
    @staticmethod
    def parse(
        code: str | bytes | bytearray,
        language: str,
        /,
        *,
        name: str | None = None,
    ) -> Ast:
        """Parse in-memory ``code`` in ``language`` (case-insensitive).

        ``name`` is an optional logical file name recorded on the top-level
        space; it need not be a real path. Raises
        :class:`UnsupportedLanguageError` when ``language`` is unknown or
        disabled in this build, and :class:`ValueError` when ``code`` is not
        ``str`` / ``bytes`` / ``bytearray``.
        """

    @staticmethod
    def from_path(path: str | os.PathLike[str], /) -> Ast:
        """Read, language-detect, and parse ``path`` in one call.

        Reads through the same text reader :func:`analyze` uses (so EOL
        normalization and metric values match), but is *no-magic*: it does
        not skip generated files and does not run the C/C++ preprocessor.
        Unlike :func:`analyze` it never silently returns nothing — it raises
        :class:`OSError` (the file could not be read),
        :class:`UnsupportedLanguageError` (no language registered for the
        path), or :class:`ValueError` (a non-UTF-8 path, or an empty /
        binary / non-UTF-8 file that cannot be parsed as text).
        """

    @property
    def language(self) -> str:
        """The canonical lowercase language slug that parsed this source."""

    @property
    def source(self) -> bytes:
        """The parsed source bytes, after EOL normalization. ``dump()`` span
        byte offsets index into exactly these bytes.
        """

    def metrics(
        self,
        *,
        exclude_tests: bool = False,
        metrics: Sequence[str] | None = None,
    ) -> FuncSpaceDict:
        """Compute metrics from the held parse (same shape as
        :func:`analyze_source`). ``metrics`` selects which to compute (all
        when omitted); two calls with different selections reuse the parse.
        """

    def dump(self, *, span: bool = True, comment: bool = False) -> AstNodeDict | None:
        """Return the AST node tree as nested dicts — the ``root`` of the
        tree ``bca dump`` / ``/ast`` emit. With ``span=True`` each node
        carries ``{start_line, start_col, end_line, end_col, start_byte,
        end_byte}`` (byte offsets index into :attr:`source`). ``comment``
        follows the CLI / ``/ast`` convention: ``comment=False`` (the
        default) keeps comment nodes, ``comment=True`` omits them. ``None``
        only if the parse produced no root.
        """

    def functions(self) -> list[FunctionSpanDict]:
        """Return each function's name and 1-based line range."""

    def ops(self) -> OpsDict:
        """Return the Halstead operator/operand tree (sorted, deduplicated
        ``operators`` / ``operands`` per space).
        """

    def count(self, filters: Sequence[str], /) -> tuple[int, int]:
        """Count nodes matching ``filters`` (tree-sitter kinds), returning
        ``(matching, total)`` — the pair ``bca count`` reports.
        """

    def strip_comments(self) -> bytes | None:
        """Return the source with comment nodes removed, or ``None`` when the
        grammar defines no comment nodes.
        """

    def suppressions(self) -> list[SuppressionMarkerDict]:
        """Return every in-source suppression marker with its location,
        scope, dialect, and enclosing function.
        """

    @property
    def root_node(self) -> Node:
        """The root :class:`Node` of the held parse, for lazy
        py-tree-sitter-style traversal without materialising the tree into
        dicts the way :meth:`dump` does (#728). Node kinds are the **raw**
        grammar kinds, not the ``Alterator``-curated kinds ``dump()`` emits.
        """

    def find(self, filters: Sequence[str], /) -> list[Node]:
        """Return every node whose kind matches one of ``filters`` as lazy
        :class:`Node` handles. ``filters`` accepts the same vocabulary as
        :meth:`count` (``all`` / ``call`` / ``comment`` / ``error`` /
        ``string`` / ``function`` / a numeric ``kind_id`` / an exact
        ``node.kind()``).
        """

    def __repr__(self) -> str: ...

@final
class Node:
    """A lazy handle to one node of a parsed :class:`Ast` (#728).

    A py-tree-sitter-style cursor into the parsed tree — ``kind``, byte
    offsets, points, ``children``, ``child_by_field_name``, ``text``,
    ``walk()`` — that does **not** materialise the tree into dicts the way
    :meth:`Ast.dump` does, so a selective extractor pays only for the nodes
    it visits. Reach one through :attr:`Ast.root_node` or :meth:`Ast.find`;
    it keeps its ``Ast`` alive, so it stays valid after every other
    reference to the parse is dropped.

    **Raw kinds.** :attr:`kind` is the unaltered grammar kind, not the
    ``Alterator``-curated kind ``dump()`` emits — the two intentionally
    disagree on altered nodes (string literals, etc.).

    **Coordinates.** Each node carries its location in every vocabulary:
    :attr:`start_byte` / :attr:`end_byte` (offsets into :attr:`Ast.source`);
    :attr:`start_point` / :attr:`end_point` (**0-based** ``(row, col)``,
    py-tree-sitter parity); and :attr:`start_line` / :attr:`end_line` plus
    :attr:`span` (**1-based**, matching ``dump()``). So ``start_line ==
    start_point[0] + 1``.
    """

    @property
    def kind(self) -> str:
        """The raw grammar kind (e.g. ``"function_item"``)."""

    @property
    def type(self) -> str:
        """py-tree-sitter-compatible alias for :attr:`kind`."""

    @property
    def kind_id(self) -> int:
        """The numeric grammar id behind :attr:`kind`."""

    @property
    def is_named(self) -> bool:
        """Whether this is a named production (vs. an anonymous token)."""

    @property
    def is_error(self) -> bool:
        """Whether this is an ``ERROR`` node."""

    @property
    def is_missing(self) -> bool:
        """Whether this is a zero-width ``MISSING`` recovery node."""

    @property
    def is_extra(self) -> bool:
        """Whether this is an ``extra`` node (e.g. a comment)."""

    @property
    def has_error(self) -> bool:
        """Whether this node or any descendant is an error/missing node."""

    @property
    def start_byte(self) -> int:
        """Start byte offset (inclusive) into :attr:`Ast.source`."""

    @property
    def end_byte(self) -> int:
        """End byte offset (exclusive) into :attr:`Ast.source`."""

    @property
    def start_point(self) -> tuple[int, int]:
        """0-based ``(row, column)`` of the start (py-tree-sitter parity)."""

    @property
    def end_point(self) -> tuple[int, int]:
        """0-based ``(row, column)`` of the end (py-tree-sitter parity)."""

    @property
    def start_line(self) -> int:
        """1-based start line (``start_point[0] + 1``)."""

    @property
    def end_line(self) -> int:
        """1-based end line (``end_point[0] + 1``)."""

    @property
    def span(self) -> SpanDict:
        """The 1-based ``{start_line, start_col, end_line, end_col,
        start_byte, end_byte}`` dict, identical to ``dump()``'s span.
        """

    @property
    def field_name(self) -> str | None:
        """The grammar field name the parent reaches this node through, or
        ``None`` for the root and field-less children.
        """

    @property
    def child_count(self) -> int:
        """The number of direct children (named and anonymous)."""

    @property
    def named_child_count(self) -> int:
        """The number of direct named children."""

    @property
    def children(self) -> list[Node]:
        """All direct children (named and anonymous), in document order."""

    @property
    def named_children(self) -> list[Node]:
        """The direct named children, in document order."""

    @property
    def parent(self) -> Node | None:
        """This node's parent, or ``None`` at the root."""

    @property
    def next_sibling(self) -> Node | None:
        """The next sibling (named or anonymous), or ``None``."""

    @property
    def prev_sibling(self) -> Node | None:
        """The previous sibling (named or anonymous), or ``None``."""

    @property
    def next_named_sibling(self) -> Node | None:
        """The next named sibling, or ``None``."""

    @property
    def prev_named_sibling(self) -> Node | None:
        """The previous named sibling, or ``None``."""

    def child(self, index: int, /) -> Node | None:
        """The child at ``index`` (all children counted), or ``None``."""

    def named_child(self, index: int, /) -> Node | None:
        """The named child at ``index``, or ``None``."""

    def child_by_field_name(self, name: str, /) -> Node | None:
        """The first child reached through field ``name``, or ``None``."""

    def children_by_field_name(self, name: str, /) -> list[Node]:
        """Every child reached through field ``name``, in order."""

    def field_name_for_child(self, index: int, /) -> str | None:
        """The field name this node reaches its child ``index`` through."""

    @property
    def text(self) -> bytes:
        """This node's ``source[start_byte:end_byte]`` slice (raw bytes)."""

    def walk(self) -> Iterator[Node]:
        """A lazy pre-order iterator over this node and its descendants
        (this node first), yielding handles one at a time.
        """

    def descendants_by_kind(self, kinds: Sequence[str], /) -> list[Node]:
        """Every node in this subtree (this node included) whose
        :attr:`kind` is in ``kinds``, in pre-order (exact raw-kind match).
        """

    # `value` is positional-only at runtime (the slot wrapper),
    # matching `object.__eq__`.
    def __eq__(self, value: object, /) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...

@final
class NodeWalk:
    """Lazy pre-order iterator over a node and its descendants, returned by
    :meth:`Node.walk`. Yields :class:`Node` handles one at a time.
    """

    def __iter__(self) -> NodeWalk: ...
    def __next__(self) -> Node: ...

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

    Returns ``None`` for any file the CLI walker would skip rather
    than emit a record for. The read goes through the walker's own
    ``read_file_with_eol`` gate, so ``None`` is returned when:

    * the file is three bytes or fewer (treated as empty),
    * its leading window is not valid UTF-8 (treated as binary),
    * (with ``skip_generated=True``, the default) its leading window
      matches the CLI walker's ``is_generated`` predicate — see
      ``skip_generated`` below.

    A UTF-8 BOM is stripped and CR/CRLF line endings are normalised to
    LF before analysis, matching the CLI byte-for-byte. A UTF-16 BOM is
    not stripped — it is one of the binary signals above (#803).
    Callers must therefore handle the optional return:

    .. code-block:: python

        result = bca.analyze(path)
        if result is None:
            # Empty / binary / generated: the CLI walker skips it too.
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

    The number-of-exit-points metric is spelled ``"nexits"`` (its
    canonical JSON key); the legacy ``"exit"`` alias was retired at
    2.0. Duplicates are silently collapsed.

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
    prefer ``big_code_analysis.vcs.rank``, which walks history once.

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

# The change-history (VCS) surface lives in the ``_native.vcs``
# submodule (issue #612): ``rank`` / ``trend`` / ``commit`` /
# ``score_diff`` plus the shared ``Options`` class. It is registered
# at runtime by the ``_native`` extension and re-exported, fully typed,
# by the pure-Python ``big_code_analysis.vcs`` facade
# (``big_code_analysis/vcs.py``) — the typed public seam, the same
# pattern ``__init__`` uses for the top-level surface.

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
    vcs: bool = False,
    vcs_per_function: bool = False,
) -> list[FuncSpaceDict | AnalysisFailure | None]:
    """Compute metrics for every path in ``paths``.

    Returns a list whose elements preserve the input order, so
    ``zip(paths, results)`` lines up by index **when no path is
    skipped**. Each element is either:

    * a ``dict`` matching :func:`analyze`'s output shape,
    * an :class:`AnalysisFailure` describing the per-file failure, or
    * ``None`` for a file the read gate declines to parse — three
      bytes or fewer, a UTF-16 BOM, a leading window that is not
      valid UTF-8, or (rarely) a file that shrank between the size
      probe and the read. This is the same ``None`` :func:`analyze`
      returns for those files, and it appears only under
      ``skip_generated=False``.

    A path that is skipped (``skip_generated=True`` and the file is
    generated *or* declined by the read gate) produces **no** element,
    so with skipping enabled the result list can be shorter than the
    input iterable. Pass ``skip_generated=False`` to guarantee one
    element per input: the generated-file filter is then off, and the
    read gate — which is unconditional — holds its slot with ``None``
    instead of dropping it (#1238). Before that fix a tiny or binary
    file silently shrank the list even with ``skip_generated=False``,
    so the endorsed ``zip`` mis-paired every later entry.

    The function **never raises on per-file errors** — a missing
    file, an unknown extension, or a parser failure becomes an
    :class:`AnalysisFailure` in the matching result slot instead. It
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
    inputs), and the canonical ``"nexits"`` spelling. Unrequested
    metrics are absent from each result dict.

    ``exclude_tests``, ``allow_lossy_path``, and ``skip_generated``
    mirror the keyword-only kwargs on :func:`analyze` exactly (#542),
    so migrating ``[bca.analyze(p) for p in paths]`` to
    ``bca.analyze_batch(paths)`` preserves each file's treatment. The
    list *shape* is preserved only under ``skip_generated=False``: with
    the default the comprehension keeps a ``None`` per skipped file
    (index-aligned) while the batch drops the slot. In
    particular ``skip_generated`` defaults to ``True`` here too: a
    generated file is *skipped* (it yields no element), matching the
    CLI walker and :func:`analyze`'s ``None`` return. This default
    flipped at 2.0 — the pre-2.0 ``analyze_batch`` hardcoded
    ``skip_generated=False`` (always one element per input). Pass
    ``skip_generated=False`` to restore that behaviour; a file the read
    gate declines occupies its slot as ``None`` rather than a ``dict``.

    The GIL is released across each file's read + tree-sitter
    parse via PyO3's ``Python::detach``, so a multi-threaded
    caller wrapping ``analyze_batch`` (or per-file ``analyze``)
    in ``concurrent.futures.ThreadPoolExecutor.map`` actually
    parallelises the heavy work. There is no built-in concurrency
    inside ``analyze_batch`` itself — the entry point is a
    sequential sweep — but the GIL release means other Python
    threads in the process are not blocked for the duration.

    ``vcs`` / ``vcs_per_function`` mirror :func:`analyze`'s kwargs
    (#670): pass ``vcs=True`` to attach a file-level ``"vcs"`` block,
    ``vcs_per_function=True`` to attach one to every nested space.
    Batch amortises the history walk — it builds **one** index / blame
    engine per containing repository and reuses it across every file in
    that repo, rather than the N one-shot walks a comprehension over
    ``analyze(p, vcs=True)`` would do. A VCS failure on one file leaves
    its AST metrics intact (it never becomes an :class:`AnalysisFailure`);
    a file outside any repository simply gets no ``"vcs"`` block. This
    keeps the "migrating the comprehension to ``analyze_batch`` is
    behaviour-preserving" claim true even when the comprehension used
    ``vcs=`` / ``vcs_per_function=``.
    """

def analyze_paths(
    *paths: str | os.PathLike[str],
    include: Sequence[str] | str | None = None,
    exclude: Sequence[str] | str | None = None,
    respect_gitignore: bool = True,
    exclude_tests: bool = False,
    allow_lossy_path: bool = False,
    skip_generated: bool = True,
    metrics: Sequence[str] | None = None,
    vcs: bool = False,
    vcs_per_function: bool = False,
) -> list[FuncSpaceDict | AnalysisFailure | None]:
    """Walk one or more path seeds and analyse every discovered file (#658).

    Each positional ``path`` may be a file or a directory; directories
    are **walked** with ``.gitignore`` awareness (the same ``ignore``
    crate the ``bca`` CLI walker uses), honouring the ``include`` /
    ``exclude`` globs and the generated-file filter. This is the
    discovery step :func:`analyze_batch` lacks — where
    :func:`analyze_batch` analyses an **explicit list** of paths
    verbatim, ``analyze_paths`` *finds* the files first, so a
    data-science consumer can point it at a repository root instead of
    writing their own walker (the canonical "analyze my repo" entry
    point).

    ``include`` / ``exclude`` accept a single glob string or a sequence
    of them; globs are matched against each file's path relative to its
    walk seed (so ``include="*.rs"`` matches ``src/lib.rs`` by its
    basename), and a leading ``./`` on a pattern is optional —
    ``dir/**`` and ``./dir/**`` are equivalent. A seed that names a
    *file* directly is always analysed regardless of ``exclude`` (an
    explicit request overrides ignore-style rules); ``include`` still
    narrows it, matched on its basename.
    Pass ``respect_gitignore=False`` to walk ignored files
    too. The remaining kwargs (``exclude_tests`` / ``allow_lossy_path``
    / ``skip_generated`` / ``metrics`` / ``vcs`` / ``vcs_per_function``)
    forward to per-file analysis exactly as on :func:`analyze` /
    :func:`analyze_batch`, including the shared per-repo VCS index (#670).

    Returns the :func:`analyze_batch` result shape with the same
    never-raise semantics: a per-file failure becomes an
    :class:`AnalysisFailure` element rather than a raise, and a generated
    file (under ``skip_generated=True``) yields no element. Under
    ``skip_generated=False`` a discovered file the read gate declines to
    parse (tiny, UTF-16-BOM, or binary) contributes a ``None`` element
    (#1238). There is no caller-supplied input position to pair it
    against here — the result order follows the walk — so it is not
    there for a ``zip``; it keeps a file the walk *found* but could not
    analyse visible in the output instead of dropping it. On a tree with
    many binary assets that is a lot of elements; filter with
    ``[r for r in results if r is not None]`` if you only want records.

    A seed that does not exist (or whose symlink dangles) is surfaced as
    an :class:`AnalysisFailure` element (``error_kind="IoError"``,
    ``error="path does not exist"``) rather than silently dropped (#858),
    keeping parity with the CLI's hard error on a missing ``--paths`` seed
    (#596). These seed-error elements lead the result list, before the
    discovered files' results.

    Raises
    ------
    ValueError
        If ``metrics`` is an empty sequence or names an unknown metric
        (validated before the walk), or if an ``include`` / ``exclude``
        glob is malformed (the offending pattern is named).
    TypeError
        If a positional ``path`` is not ``str`` / ``os.PathLike[str]``.
    """

def language_for_file(
    path: str | os.PathLike[str], /, *, read: bool = True
) -> str | None:
    """Return the language name :func:`analyze` would dispatch for ``path``.

    Resolves through the same ``big_code_analysis::guess_language``
    pipeline :func:`analyze` uses: the path extension wins when
    recognised, otherwise the file's leading window is inspected for
    a ``#!`` shebang (``#!/usr/bin/env python``, ``#!/bin/bash``, …)
    or an emacs ``-*- mode: … -*-`` declaration. Returns ``None``
    only when none of those signals resolve.

    With ``read=True`` (the default) the file is read before inspection
    (parity with :func:`analyze`, #318). Pass ``read=False`` (#682) for
    the cheap, **filesystem-free** path: it resolves by extension alone,
    reads nothing, and **never raises** — so it answers for paths that do
    not exist yet (an archive listing, a git-tree entry, candidate
    filtering). ``read=False`` returns ``None`` for an extension-less path
    or an unknown extension, delegating to the same table
    :func:`language_for_extension` uses.

    Raises
    ------
    OSError
        For any underlying I/O failure, **only when ``read=True``**.
        Dispatches to the canonical subclass (``FileNotFoundError``,
        ``PermissionError``, ``IsADirectoryError``, …) based on
        ``errno``, with ``err.errno`` and ``err.filename`` populated —
        same shape as :func:`analyze`. For the prior "extension only,
        never raises" semantics pass ``read=False`` (the #682 successor
        to wrapping the call in ``try / except OSError``).
    """

def language_for_extension(ext: str, /) -> str | None:
    """Return the language name for a bare file extension (#682).

    Accepts both ``"py"`` and ``".py"`` (the leading dot is normalised
    away); matching is case-insensitive. Returns ``None`` for an unknown
    extension — a pure table lookup that reads no file and never raises,
    the inverse of :func:`language_extensions`. This is the cheap "which
    language is ``.tsx``?" primitive that previously had to be rebuilt by
    inverting the per-language extension table by hand.
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

def language_grammar_version(language: str, /) -> str:
    """Return the pinned tree-sitter grammar crate version backing
    ``language`` (e.g. ``"0.25.1"`` for ``"bash"``).

    For languages backed by an upstream crates.io grammar this is the exact
    upstream version, so a consumer migrating matchers off py-tree-sitter
    can line node-kind vocabularies up against the same pin. For the
    vendored big-code-analysis forks (``mozcpp``, ``mozjs``, ``tcl``,
    ``kotlin``) it is the fork crate's version, not an upstream grammar
    semver.

    Raises
    ------
    UnsupportedLanguageError
        If ``language`` is not a known language name.
    """

def to_sarif(
    result: FuncSpaceDict
    | None
    | Iterable[FuncSpaceDict | AnalysisFailure | None],
    /,
    *,
    thresholds: dict[str, float] | None = None,
) -> str:
    """Render a SARIF 2.1.0 JSON document from analysis results.

    ``result`` accepts either a single ``dict`` returned by
    :func:`analyze` / :func:`analyze_source`, a scalar ``None`` (the
    documented return of :func:`analyze` for generated files; yields
    an empty SARIF run), or any iterable yielding such dicts,
    :class:`AnalysisFailure` instances, and/or ``None`` (the natural
    shape of :func:`analyze_batch`'s return value, or a list
    comprehension over :func:`analyze` which returns ``None`` for
    generated files). ``AnalysisFailure`` and ``None`` entries are
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

    A finding is emitted at every space whose *own* value breaches
    its limit, exactly matching ``bca check --report-format sarif``.
    Emission is scope-gated per metric (``loc.*`` at the file unit,
    ``nom`` / ``wmc`` / ``npm`` / ``npa`` at containers, the rest at
    function spaces), and the four subtree-aggregate metrics
    (``cyclomatic``, ``cyclomatic.modified``, ``cognitive``,
    ``abc``) read the per-space ``value`` field rather than the
    rolled-up aggregate (#958, #969).
    Unit-level findings carry ``logicalLocations: [{"fullyQualifiedName":
    "<file>"}]``; every other space carries its qualified symbol. Within
    that symbol, a closure/lambda (the ``<anonymous>`` name every grammar
    emits) and the ``None``-name parse-failure case both collapse to
    ``<anon@L{start_line}>``, matching the CLI's ``space_segment``.

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
