"""Batch analysis with the never-raise ``analyze_batch`` entry point.

Shows the ``AnalysisFailure`` discriminator pattern and a
``ThreadPoolExecutor`` parallelism recipe. Tied to the book's
``python/batch.md`` page.
"""

from __future__ import annotations

from collections.abc import Iterable
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import big_code_analysis as bca
from big_code_analysis import FuncSpaceDict


def run(paths: Iterable[Path]) -> dict[str, int]:
    """Analyse ``paths`` as a batch and bucket successes vs failures.

    Returns a small summary dict (`ok`, `errors`, `skipped`, `total`) so
    the accompanying test can assert on it without re-parsing.
    """
    materialised = list(paths)
    # `skip_generated=False` guarantees one result element per input
    # (generated files are analysed, not dropped), so the `strict=True`
    # zip against `materialised` cannot raise `ValueError`. Under the
    # 2.0 default (`skip_generated=True`) a generated input yields no
    # slot, the lengths diverge, and the strict zip blows up — the same
    # bug #660 fixed in `pipeline_db.py`.
    results = bca.analyze_batch(materialised, skip_generated=False)

    ok = 0
    errors = 0
    skipped = 0
    for path, result in zip(materialised, results, strict=True):
        if isinstance(result, bca.AnalysisFailure):
            errors += 1
            print(f"  skip {path}: ({result.error_kind}) {result.error}")
        elif result is None:
            # The read gate declined this file; the slot is held open
            # so the strict zip above stays aligned (#1238).
            skipped += 1
            print(f"  skip {path}: nothing to parse (empty or binary)")
        else:
            ok += 1
            sloc = result["metrics"]["loc"]["sloc"]
            print(f"  ok   {path}: sloc = {sloc:.0f}")

    return {
        "ok": ok,
        "errors": errors,
        "skipped": skipped,
        "total": len(materialised),
    }


def run_parallel(paths: Iterable[Path], *, workers: int = 4) -> list[FuncSpaceDict | None]:
    """Fan ``analyze`` out across a thread pool.

    PyO3 releases the GIL across each file's read + parse, so a
    thread pool actually parallelises the heavy work. Use this when
    you need per-file exceptions instead of ``AnalysisFailure`` slots.
    """

    def _analyze(p: Path) -> FuncSpaceDict | None:
        return bca.analyze(p)

    with ThreadPoolExecutor(max_workers=workers) as pool:
        return list(pool.map(_analyze, paths))


if __name__ == "__main__":
    import sys

    if len(sys.argv) < 2:
        sys.exit("usage: python batch_processing.py <path> [<path> ...]")
    run([Path(p) for p in sys.argv[1:]])
