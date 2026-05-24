"""Drive ``bca.analyze`` from ``asyncio`` via ``to_thread``.

Tied to the book's ``python/async.md`` page. ``bca.analyze`` is
CPU-bound (tree-sitter parse + Rust metric pass) and releases the
GIL across the work, so the canonical async pattern is
``asyncio.to_thread`` — not a custom executor and not a synchronous
call inside a coroutine.
"""

from __future__ import annotations

import asyncio
from collections.abc import Iterable
from pathlib import Path
from typing import Any

import big_code_analysis as bca


async def analyze_async(path: Path) -> dict[str, Any] | None:
    """Run ``bca.analyze(path)`` on the default thread executor."""
    return await asyncio.to_thread(bca.analyze, str(path))


async def analyze_all(
    paths: Iterable[Path],
) -> list[dict[str, Any] | BaseException | None]:
    """Fan ``analyze_async`` out across ``paths`` with ``asyncio.gather``.

    ``return_exceptions=True`` matters here: ``bca.analyze`` runs
    inside ``asyncio.to_thread`` and Python threads cannot be
    cancelled. If one call raises and gather re-raises with
    ``return_exceptions=False``, the surviving threads keep running
    in the default executor, producing results that are silently
    discarded. With ``return_exceptions=True`` every thread's
    result (success OR exception) lands in the returned list so
    the caller can dispatch per-file.
    """
    return await asyncio.gather(
        *(analyze_async(p) for p in paths),
        return_exceptions=True,
    )


def run(paths: Iterable[Path]) -> dict[str, int]:
    """Synchronous entry point for the test suite.

    Wraps ``analyze_all`` in ``asyncio.run`` and returns a small
    summary so the caller can assert on it.
    """
    results = asyncio.run(analyze_all(paths))
    return {
        "count": len(results),
        "analyzed": sum(isinstance(r, dict) for r in results),
        "errors": sum(isinstance(r, BaseException) for r in results),
    }


if __name__ == "__main__":
    import sys

    if len(sys.argv) < 2:
        sys.exit("usage: python async_patterns.py <path> [<path> ...]")
    print(run([Path(p) for p in sys.argv[1:]]))
