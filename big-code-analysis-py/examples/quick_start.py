"""Quick-start: analyse one file and print the headline cyclomatic count.

Mirrors the worked example shown on the book's
``python/quick-start.md`` page. The book embeds this file verbatim,
so the snippet is the test fixture — if the API drifts, the
``test_book_examples.py`` test fails and the docs are forced back
into sync.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

import big_code_analysis as bca


def run(path: Path) -> dict[str, Any]:
    """Analyse ``path`` and return its metric dict."""
    result = bca.analyze(path)
    if result is None:
        msg = f"{path} was skipped (looks generated)"
        raise SystemExit(msg)

    cyclomatic = result["metrics"]["cyclomatic"]
    print(f"{result['name']}: cyclomatic sum = {cyclomatic['sum']:.0f}")
    return result


if __name__ == "__main__":
    import sys

    if len(sys.argv) != 2:
        sys.exit("usage: python quick_start.py <path>")
    run(Path(sys.argv[1]))
