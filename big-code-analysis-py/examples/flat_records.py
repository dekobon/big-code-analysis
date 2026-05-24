"""Flatten a FuncSpace tree into scalar rows for sqlite / pandas.

Demonstrates ``bca.flatten_spaces`` + ``sqlite3.executemany``. The
pandas equivalent is shown in the book as a non-executed snippet so
this example stays dependency-free (sqlite ships with the stdlib).

Tied to the book's ``python/flat-records.md`` page.
"""

from __future__ import annotations

import sqlite3
from contextlib import closing
from pathlib import Path

import big_code_analysis as bca

# SQLite identifier names are case-insensitive, so the Halstead
# pair `N1` / `n1` (and `N2` / `n2`) collide on one column. Rewrite
# the uppercase totals to a distinct name before insertion. The
# explicit map (not a `.replace(".N", "...")` substring rewrite)
# means a hypothetical future `halstead.NN_metric` would not be
# silently mangled.
_RENAME_FOR_SQLITE: dict[str, str] = {
    "halstead.N1": "halstead.total_1",
    "halstead.N2": "halstead.total_2",
}


def _safe_column(key: str) -> str:
    return _RENAME_FOR_SQLITE.get(key, key)


def run(path: Path, db_path: Path) -> int:
    """Analyse ``path`` and insert one row per FuncSpace into ``db_path``.

    Returns the number of rows inserted so the test can assert on it.
    """
    result = bca.analyze(path)
    if result is None:
        msg = f"{path} was skipped (looks generated)"
        raise SystemExit(msg)

    records = [{_safe_column(k): v for k, v in r.items()} for r in bca.flatten_spaces(result)]
    if not records:
        return 0

    columns = sorted({k for r in records for k in r})
    cols_sql = ", ".join(f'"{c}"' for c in columns)
    placeholders = ", ".join("?" for _ in columns)
    rows = [tuple(r.get(c) for c in columns) for r in records]

    # `closing(sqlite3.connect(...))` is the documented idiom — the
    # bare ``with sqlite3.connect(...)`` context manager only commits
    # / rolls back the transaction; it does NOT close the connection,
    # so a long-running consumer leaks file descriptors (and on
    # Windows holds an exclusive write lock on the db file).
    with closing(sqlite3.connect(db_path)) as db, db:
        db.execute(f"CREATE TABLE IF NOT EXISTS metrics ({cols_sql})")
        db.executemany(
            f"INSERT INTO metrics ({cols_sql}) VALUES ({placeholders})",
            rows,
        )

    return len(rows)


if __name__ == "__main__":
    import sys

    if len(sys.argv) != 3:
        sys.exit("usage: python flat_records.py <source-file> <out.db>")
    inserted = run(Path(sys.argv[1]), Path(sys.argv[2]))
    print(f"inserted {inserted} rows into {sys.argv[2]}")
