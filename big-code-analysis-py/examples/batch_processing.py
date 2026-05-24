"""Batch analysis with the never-raise ``analyze_batch`` entry point.

Shows the ``AnalysisError`` discriminator pattern and a
``ThreadPoolExecutor`` parallelism recipe. Tied to the book's
``python/batch.md`` page.
"""

from __future__ import annotations

from collections.abc import Iterable
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import Any

import big_code_analysis as bca


def run(paths: Iterable[Path]) -> dict[str, int]:
    """Analyse ``paths`` as a batch and bucket successes vs failures.

    Returns a small summary dict (`ok`, `errors`, `total`) so the
    accompanying test can assert on it without re-parsing.
    """
    materialised = [str(p) for p in paths]
    results = bca.analyze_batch(materialised)

    ok = 0
    errors = 0
    for path, result in zip(materialised, results, strict=True):
        if isinstance(result, bca.AnalysisError):
            errors += 1
            print(f"  skip {path}: ({result.error_kind}) {result.error}")
        else:
            ok += 1
            sloc = result["metrics"]["loc"]["sloc"]
            print(f"  ok   {path}: sloc = {sloc:.0f}")

    return {"ok": ok, "errors": errors, "total": len(materialised)}


def run_parallel(paths: Iterable[Path], *, workers: int = 4) -> list[dict[str, Any] | None]:
    """Fan ``analyze`` out across a thread pool.

    PyO3 releases the GIL across each file's read + parse, so a
    thread pool actually parallelises the heavy work. Use this when
    you need per-file exceptions instead of ``AnalysisError`` slots.
    """

    def _analyze(p: Path) -> dict[str, Any] | None:
        return bca.analyze(str(p))

    with ThreadPoolExecutor(max_workers=workers) as pool:
        return list(pool.map(_analyze, paths))


if __name__ == "__main__":
    import sys

    if len(sys.argv) < 2:
        sys.exit("usage: python batch_processing.py <path> [<path> ...]")
    run([Path(p) for p in sys.argv[1:]])
