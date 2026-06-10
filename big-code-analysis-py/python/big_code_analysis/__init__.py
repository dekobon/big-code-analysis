"""Python bindings for the big-code-analysis Rust library.

All entry points live in the compiled extension ``_native``; this
facade exists so the public API is reachable via the package name
(``big_code_analysis.analyze``) and so static analysers can resolve
the symbols through the bundled type stubs in ``_native.pyi``.

The ``Lang`` and ``MetricName`` string enums (``big_code_analysis._enums``)
are generated from the same upstream tables the CLI and JSON output
use, so ``Lang.CPP == "cpp"`` and every member round-trips with
``analyze_source`` / ``language_extensions`` / the ``metrics=``
selector. :func:`supported_languages` returns ``list[Lang]`` and
:data:`METRIC_NAMES` is a ``tuple[MetricName, ...]``; because
``StrEnum`` members *are* ``str`` instances, existing string-based
call sites keep working unchanged.

See ``big-code-analysis-py/README.md`` for usage examples and the
project book for the per-language metric semantics.
"""

from __future__ import annotations

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
    language_extensions,
    language_for_file,
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
