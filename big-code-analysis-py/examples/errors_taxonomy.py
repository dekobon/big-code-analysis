"""Walk through three representative error paths the bindings expose.

Tied to the book's ``python/errors.md`` page. Demonstrates:

* ``bca.UnsupportedLanguageError`` (subclass of ``ValueError``) —
  raised from ``analyze`` for an unknown extension.
* ``FileNotFoundError`` (subclass of ``OSError``) — typed
  dispatch from ``analyze`` for a missing path.
* ``bca.AnalysisError`` — returned, never raised, by
  ``analyze_batch``.

Other paths in the closed taxonomy — ``bca.ParseError``, raw
``ValueError`` for non-UTF-8 paths, and the rest of the
``OSError`` subclasses (``PermissionError`` /
``IsADirectoryError`` / …) — are documented in the chapter but
not exercised here. ``ParseError`` in particular needs grammar-
specific malformed input to fire deterministically (tree-sitter
emits ``ERROR`` nodes rather than raising); the example sticks
to portable inputs.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

import big_code_analysis as bca


def run(
    fixtures: Path,
    *,
    missing_path: Path,
) -> dict[str, Any]:
    """Trigger each error path and return a small report.

    ``fixtures`` is a directory containing at least ``hello.rs``;
    ``missing_path`` must NOT exist on disk.
    """
    report: dict[str, Any] = {
        "file_not_found": False,
        "unsupported": False,
        "batch_errors": 0,
    }

    # 1. analyze() on a missing path raises a typed OSError subclass.
    try:
        bca.analyze(str(missing_path))
    except FileNotFoundError as err:
        report["file_not_found"] = True
        print(f"file_not_found: errno={err.errno} filename={err.filename}")

    # 2. analyze() on an unknown extension raises
    #    UnsupportedLanguageError (itself a ValueError subclass).
    #    The write is inside the try/finally so a future second
    #    mutation before the analyse call still gets cleaned up.
    unknown = fixtures / "hello.unknown_extension"
    try:
        unknown.write_text("noop", encoding="utf-8")
        bca.analyze(str(unknown))
    except bca.UnsupportedLanguageError as err:
        report["unsupported"] = True
        print(f"unsupported_language: {err}")
    finally:
        unknown.unlink(missing_ok=True)

    # 3. analyze_batch() returns AnalysisError, never raises per-file.
    paths = [str(fixtures / "hello.rs"), str(missing_path)]
    for slot in bca.analyze_batch(paths):
        if isinstance(slot, bca.AnalysisError):
            report["batch_errors"] += 1
            print(f"batch_error: ({slot.error_kind}) {slot.error}")

    return report


if __name__ == "__main__":
    import sys

    if len(sys.argv) != 2:
        sys.exit("usage: python errors_taxonomy.py <fixtures-dir>")
    run(Path(sys.argv[1]), missing_path=Path(sys.argv[1]) / "does_not_exist.rs")
