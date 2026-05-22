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

def analyze(path: str | os.PathLike[str], /) -> dict[str, Any]:
    """Compute metrics for the file at ``path``.

    The returned ``dict`` is byte-for-byte equivalent to the JSON
    emitted by ``bca metrics --output-format json`` for the same
    file: identical field order (``name``, ``start_line``,
    ``end_line``, ``kind``, ``spaces``, ``metrics``), identical
    numeric formatting, identical shape. Both sides serialise the
    same ``FuncSpace`` through ``serde_json::to_string``; the
    bindings then parse that JSON with ``json.loads``, which
    preserves insertion order on CPython 3.7+.

    Raises
    ------
    UnsupportedLanguageError
        If ``path`` has no extension or its extension is not recognised.
    ParseError
        If the tree-sitter parser fails on the source.
    OSError
        For any underlying I/O failure (e.g. file not found).
    """

def analyze_source(
    code: str | bytes | bytearray,
    language: str,
    /,
) -> dict[str, Any]:
    """Compute metrics for an in-memory source buffer.

    ``language`` is a name returned by :func:`supported_languages`
    (case-insensitive). ``code`` may be ``str`` (encoded as UTF-8),
    ``bytes``, or ``bytearray``.

    Raises
    ------
    UnsupportedLanguageError
        If ``language`` is not a known language name.
    ParseError
        If the tree-sitter parser fails on the source.
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
