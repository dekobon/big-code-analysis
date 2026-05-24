"""Compute a subset of the metric suite with the ``metrics=`` kwarg.

Tied to the book's ``python/metrics.md`` page. Demonstrates the
canonical names, the auto-pulled dependencies for derived metrics
(``mi`` → ``loc`` + ``cyclomatic`` + ``halstead``), and the
``METRIC_NAMES`` enumeration.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

import big_code_analysis as bca


def run(path: Path) -> dict[str, Any]:
    """Compute only LoC + cyclomatic for ``path`` and return the result.

    ``bca.METRIC_NAMES`` is a ``tuple[str, ...]`` of every canonical
    name accepted by ``metrics=``. The string ``"halstead"`` is one
    of them; ``in`` membership tests the selection client-side
    before any I/O is paid for.
    """
    if "halstead" not in bca.METRIC_NAMES:
        msg = "halstead is missing from METRIC_NAMES — bindings ABI drift"
        raise RuntimeError(msg)
    selected = bca.analyze(path, metrics=["loc", "cyclomatic"])
    if selected is None:
        msg = f"{path} was skipped (looks generated)"
        raise SystemExit(msg)

    metric_keys = sorted(selected["metrics"])
    print(f"computed only: {metric_keys}")
    return selected


def run_derived(path: Path) -> dict[str, Any]:
    """Selecting ``mi`` auto-pulls in its three dependencies."""
    selected = bca.analyze(path, metrics=["mi"])
    if selected is None:
        msg = f"{path} was skipped (looks generated)"
        raise SystemExit(msg)

    pulled = sorted(selected["metrics"])
    print(f"mi pulled in: {pulled}")
    return selected


if __name__ == "__main__":
    import sys

    if len(sys.argv) != 2:
        sys.exit("usage: python metric_selection.py <path>")
    run(Path(sys.argv[1]))
    run_derived(Path(sys.argv[1]))
