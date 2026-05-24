"""Python bindings for the big-code-analysis Rust library.

All entry points live in the compiled extension ``_native``; this
facade exists so the public API is reachable via the package name
(``big_code_analysis.analyze``) and so static analysers can resolve
the symbols through the bundled type stubs in ``_native.pyi``.

See ``big-code-analysis-py/README.md`` for usage examples and the
project book for the per-language metric semantics.
"""

from __future__ import annotations

from ._flatten import flatten_spaces
from ._native import (
    METRIC_NAMES,
    AnalysisError,
    ParseError,
    UnsupportedLanguageError,
    __version__,
    analyze,
    analyze_batch,
    analyze_source,
    language_extensions,
    language_for_file,
    supported_languages,
)

__all__ = [
    "METRIC_NAMES",
    "AnalysisError",
    "ParseError",
    "UnsupportedLanguageError",
    "__version__",
    "analyze",
    "analyze_batch",
    "analyze_source",
    "flatten_spaces",
    "language_extensions",
    "language_for_file",
    "supported_languages",
]
