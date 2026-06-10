"""Python bindings for the big-code-analysis Rust library.

All entry points live in the compiled extension ``_native``; this
facade exists so the public API is reachable via the package name
(``big_code_analysis.analyze``) and so static analysers can resolve
the symbols through the bundled type stubs in ``_native.pyi``.

The ``Lang`` and ``MetricName`` string enums (``big_code_analysis._enums``)
are generated from the same upstream tables the CLI and JSON output
use, so ``Lang.CPP == "cpp"`` and every member round-trips with
``analyze_source`` / ``language_extensions`` / the ``metrics=``
selector. :func:`supported_languages` returns ``list[Lang]``,
:func:`language_for_file` returns ``Lang | None``, and
:data:`METRIC_NAMES` is a ``tuple[MetricName, ...]``; because
``StrEnum`` members *are* ``str`` instances, existing string-based
call sites keep working unchanged.

See ``big-code-analysis-py/README.md`` for usage examples and the
project book for the per-language metric semantics.
"""

from __future__ import annotations

import os

from . import _native
from ._enums import Lang, MetricName
from ._flatten import flatten_spaces
from ._native import (
    AnalysisError,
    InvalidDiffError,
    InvalidRevisionError,
    NotARepositoryError,
    ParseError,
    UnsupportedLanguageError,
    VcsEnvironmentError,
    VcsError,
    __version__,
    analyze,
    analyze_batch,
    analyze_source,
    to_sarif,
    vcs_jit,
    vcs_metrics,
    vcs_trend,
)

#: Canonical metric names, as :class:`MetricName` members. The values
#: are ``str``-compatible, so ``"cognitive" in METRIC_NAMES`` and
#: ``metrics=["cognitive"]`` keep working; the typed members add IDE
#: discoverability. Sourced verbatim from the native ``METRIC_NAMES``
#: tuple (the upstream ``Metric::NAMES`` vocabulary).
METRIC_NAMES: tuple[MetricName, ...] = tuple(MetricName(name) for name in _native.METRIC_NAMES)


def supported_languages() -> list[Lang]:
    """Return the supported languages, in declaration order.

    Each element is a :class:`Lang` member; because ``Lang`` is a
    ``StrEnum`` the values compare equal to their canonical slug
    (``Lang.RUST == "rust"``) and round-trip through
    :func:`analyze_source` and :func:`language_extensions`.
    """
    return [Lang(name) for name in _native.supported_languages()]


def language_for_file(path: str | os.PathLike[str], /) -> Lang | None:
    """Return the :class:`Lang` :func:`analyze` would dispatch for ``path``.

    Mirrors :func:`supported_languages`: the native call returns the
    canonical slug, which this facade lifts into the :class:`Lang`
    enum so the language vocabulary is consistently typed. Because
    ``Lang`` is a ``StrEnum`` the result still compares equal to its
    slug (``language_for_file("foo.py") == "python"``) and remains
    ``in supported_languages()`` — existing string-based call sites
    keep working.

    Resolves through the same ``big_code_analysis::guess_language``
    pipeline :func:`analyze` uses: the path extension wins when
    recognised, otherwise the file's leading window is inspected for a
    ``#!`` shebang or an emacs ``-*- mode: … -*-`` declaration. Returns
    ``None`` only when none of those signals resolve.

    Reads the file before inspection (parity with :func:`analyze`,
    #318); see the native ``language_for_file`` stub for the ``OSError``
    contract on I/O failure.
    """
    name = _native.language_for_file(path)
    return Lang(name) if name is not None else None


def language_extensions(language: str | Lang, /) -> list[str]:
    """Return the file extensions registered for ``language``.

    Accepts either a canonical slug or a :class:`Lang` member; the
    latter is a ``StrEnum`` instance, so it passes straight through to
    the native lookup.

    Raises
    ------
    UnsupportedLanguageError
        If ``language`` is not a known language name.
    """
    return _native.language_extensions(language)


__all__ = [
    "METRIC_NAMES",
    "AnalysisError",
    "InvalidDiffError",
    "InvalidRevisionError",
    "Lang",
    "MetricName",
    "NotARepositoryError",
    "ParseError",
    "UnsupportedLanguageError",
    "VcsEnvironmentError",
    "VcsError",
    "__version__",
    "analyze",
    "analyze_batch",
    "analyze_source",
    "flatten_spaces",
    "language_extensions",
    "language_for_file",
    "supported_languages",
    "to_sarif",
    "vcs_jit",
    "vcs_metrics",
    "vcs_trend",
]
