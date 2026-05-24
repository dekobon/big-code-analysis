"""Render an ``analyze_batch`` result into SARIF 2.1.0 JSON.

Tied to the book's ``python/sarif.md`` page. The CLI's threshold
table is the source of truth — ``cyclomatic``, ``loc.lloc``,
``halstead.volume``, etc. — and the bindings reject unknown names
with a clear ``ValueError``.
"""

from __future__ import annotations

from collections.abc import Iterable, Mapping
from pathlib import Path

import big_code_analysis as bca


def run(
    paths: Iterable[Path],
    sarif_path: Path,
    thresholds: Mapping[str, float],
) -> str:
    """Analyse ``paths`` and write a SARIF document to ``sarif_path``.

    Returns the rendered SARIF JSON so the caller (or the test) can
    inspect it without re-reading the file.
    """
    batch = bca.analyze_batch([str(p) for p in paths])
    sarif = bca.to_sarif(batch, thresholds=dict(thresholds))

    sarif_path.parent.mkdir(parents=True, exist_ok=True)
    sarif_path.write_text(sarif, encoding="utf-8")
    print(f"wrote {sarif_path} ({len(sarif)} bytes)")
    return sarif


if __name__ == "__main__":
    import sys

    if len(sys.argv) < 3:
        sys.exit("usage: python sarif_output.py <out.sarif> <path> [<path> ...]")
    run(
        [Path(p) for p in sys.argv[2:]],
        Path(sys.argv[1]),
        {"cyclomatic": 15, "loc.lloc": 200},
    )
