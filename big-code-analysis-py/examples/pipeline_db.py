"""Minimal pipeline-style example: directory walk → batch → sqlite.

Issue #273 (phase 9/9) asks for an end-to-end example that walks
a directory, calls :func:`big_code_analysis.analyze_batch`,
flattens each result into scalar rows via
:func:`big_code_analysis.flatten_spaces`, persists those rows in a
sqlite database, and queries the top-N most-complex functions
back out. The script also threads a deliberately-broken file
through the batch to demonstrate the never-raise semantics:
:func:`analyze_batch` returns an :class:`AnalysisError` for the
bad input while the rest of the batch lands normally.

This example complements ``flat_records.py`` — that one shows the
single-file flattening pipeline; this one shows the directory-walk
+ batch + error-bucketing pipeline that real ETL workloads actually
look like.

The sqlite schema is built dynamically from the keys present in the
flattened rows so it survives the addition of new metrics without an
editing pass. Halstead's ``N1`` / ``N2`` aggregates collide with
their ``n1`` / ``n2`` siblings under sqlite's case-insensitive
column matching — the same explicit rename ``flat_records.py``
uses is applied here, kept in lockstep deliberately.
"""

from __future__ import annotations

import argparse
import sqlite3
import sys
from collections.abc import Iterable
from contextlib import closing
from pathlib import Path

import big_code_analysis as bca

# Halstead exposes both `n1` (distinct operators) and `N1`
# (total operators); sqlite identifiers are case-insensitive, so
# both columns collapse onto the same name. Rewrite the uppercase
# totals to a distinct column so neither side gets clobbered.
# Kept in lockstep with `flat_records.py` — the workaround is
# load-bearing under sqlite, not a stylistic choice.
_RENAME_FOR_SQLITE: dict[str, str] = {
    "halstead.N1": "halstead.total_1",
    "halstead.N2": "halstead.total_2",
}


def _safe_column(key: str) -> str:
    return _RENAME_FOR_SQLITE.get(key, key)


def discover_sources(root: Path) -> list[Path]:
    """Yield every file under ``root`` a language can claim.

    Resolves each candidate via :func:`bca.language_for_file`, which
    matches the path extension and (for extension-less scripts) the
    leading ``#!`` shebang / emacs ``-*- mode -*-`` declaration —
    so extension-less Python scripts like the bindings'
    ``tests/fixtures/install`` shebang fixture are included. The
    candidate set is filtered upfront by the registered extension
    table so the per-file ``language_for_file`` reads stay scoped to
    files that can plausibly be sources.
    """
    suffixes: set[str] = set()
    for lang in bca.supported_languages():
        for ext in bca.language_extensions(lang):
            # `language_extensions` returns the bare suffix (no dot);
            # `Path.suffix` includes the leading dot. Normalise here
            # so the membership check below stays a single string op.
            suffixes.add(f".{ext.lower()}")

    def _is_source(p: Path) -> bool:
        if not p.is_file():
            return False
        if p.suffix and p.suffix.lower() in suffixes:
            return True
        # Extension-less candidate — pay the file-read cost in
        # `language_for_file` to catch shebang / emacs-mode scripts.
        # Wrap in try/except: missing-file races and permission
        # errors here would otherwise abort the whole walk.
        try:
            return bca.language_for_file(p) is not None
        except OSError:
            return False

    return sorted(p for p in root.rglob("*") if _is_source(p))


def run(
    root: Path,
    db_path: Path,
    *,
    extra_paths: Iterable[Path] = (),
    top_n: int = 5,
    skip_generated: bool = True,
) -> dict[str, int]:
    """Walk ``root``, batch-analyse, persist to ``db_path``, return summary.

    ``extra_paths`` lets the caller (or the test) splice in extra
    inputs that the directory walk would not naturally pick up —
    typically a deliberately-broken file used to demonstrate the
    :class:`AnalysisError` discriminator path.

    ``skip_generated`` (default ``True``) routes through per-file
    :func:`bca.analyze` instead of :func:`bca.analyze_batch` so the
    CLI's ``@generated`` / ``DO NOT EDIT`` walker filter applies —
    ``bca.analyze_batch`` hardcodes ``skip_generated=False`` and
    would otherwise pull machine-generated code (``*.pb.rs``,
    OpenAPI stubs, the workspace's own ``generated.rs`` fixture)
    into the dashboard. Pass ``False`` to opt back into the batch
    entry point if you want every file, generated or not.

    Returns a small summary dict (``analyzed``, ``errors``,
    ``skipped``, ``rows``, ``top_n``) so the caller can assert
    without re-querying.
    """
    discovered = discover_sources(root)
    inputs: list[Path] = [*discovered, *extra_paths]
    if not inputs:
        msg = f"no source files discovered under {root}"
        raise SystemExit(msg)

    analyzed = 0
    errors = 0
    skipped = 0
    flat_rows: list[dict[str, object]] = []
    if skip_generated:
        # Per-file `analyze` honours the walker's `is_generated`
        # filter (returns None) and raises typed OSError /
        # UnsupportedLanguageError / ParseError on failure. Catch
        # those and bucket alongside the AnalysisError taxonomy so
        # the never-raise contract still holds for the caller.
        for path in inputs:
            try:
                result = bca.analyze(path)
            except (OSError, ValueError) as exc:
                errors += 1
                print(f"  skip {path}: ({type(exc).__name__}) {exc}")
                continue
            if result is None:
                skipped += 1
                print(f"  skip {path}: looks generated")
                continue
            analyzed += 1
            flat_rows.extend(
                {_safe_column(k): v for k, v in record.items()}
                for record in bca.flatten_spaces(result)
            )
    else:
        batch = bca.analyze_batch([str(p) for p in inputs])
        for path, batch_result in zip(inputs, batch, strict=True):
            if isinstance(batch_result, bca.AnalysisError):
                errors += 1
                print(f"  skip {path}: ({batch_result.error_kind}) {batch_result.error}")
                continue
            analyzed += 1
            flat_rows.extend(
                {_safe_column(k): v for k, v in record.items()}
                for record in bca.flatten_spaces(batch_result)
            )

    inserted = _persist(db_path, flat_rows)
    top = _top_n_cyclomatic(db_path, top_n)
    print(
        f"persisted {inserted} rows from {analyzed} files "
        f"({errors} errors, {skipped} generated skipped)"
    )
    # ASCII '-' separator (not U+2014 EM DASH) — non-UTF Windows
    # console code pages (cp1252 on default Windows) raise
    # UnicodeEncodeError on the em-dash and crash this happy-path
    # loop. The whole script is otherwise pure ASCII for that
    # reason; keep it that way.
    for rank, row in enumerate(top, start=1):
        print(
            f"  #{rank}: {row['name']!r} in {row['path']!r} - "
            f"cyclomatic.sum = {row['cyclomatic_sum']}"
        )

    return {
        "analyzed": analyzed,
        "errors": errors,
        "skipped": skipped,
        "rows": inserted,
        "top_n": len(top),
    }


# Minimum schema the helper queries against; surfaces even when
# every input errored and no flattened rows landed. Without this
# floor, an all-AnalysisError batch leaves `metrics` un-created,
# `_top_n_cyclomatic` then raises `sqlite3.OperationalError: no
# such table: metrics` mid-pipeline — directly contradicting the
# never-raise demo this example exists to showcase. The columns
# listed here are the subset `_top_n_cyclomatic` SELECTs; the
# dynamic schema below ADDs any additional metric columns the
# current batch happens to carry.
_BASE_COLUMNS: tuple[str, ...] = (
    "path",
    "name",
    "kind",
    "start_line",
    "end_line",
    "cyclomatic.sum",
)


def _persist(db_path: Path, rows: list[dict[str, object]]) -> int:
    """Write ``rows`` to ``db_path`` under a dynamically-derived schema.

    The metrics table is created unconditionally (even when ``rows``
    is empty) so :func:`_top_n_cyclomatic` cannot raise
    ``OperationalError: no such table`` on a pathological all-error
    batch.
    """
    derived = sorted({k for r in rows for k in r})
    # Preserve `_BASE_COLUMNS` order at the front, then append any
    # other columns alphabetically. Using a list + a seen-set keeps
    # the order deterministic across runs (a `set(...) | ...` would
    # not).
    columns: list[str] = []
    seen: set[str] = set()
    for c in (*_BASE_COLUMNS, *derived):
        if c not in seen:
            columns.append(c)
            seen.add(c)
    cols_sql = ", ".join(f'"{c}"' for c in columns)
    placeholders = ", ".join("?" for _ in columns)
    payload = [tuple(r.get(c) for c in columns) for r in rows]

    # `closing(...)` plus the inner `with db` is the documented sqlite3
    # idiom: the outer `closing` releases the file descriptor, the
    # inner `with` commits / rolls back the transaction. A bare
    # `with sqlite3.connect(...)` would leak the connection (commits
    # but does NOT close) — and on Windows that holds an exclusive
    # write lock on the .db until GC fires.
    with closing(sqlite3.connect(db_path)) as db, db:
        db.execute(f"CREATE TABLE IF NOT EXISTS metrics ({cols_sql})")
        if payload:
            db.executemany(
                f"INSERT INTO metrics ({cols_sql}) VALUES ({placeholders})",
                payload,
            )
    return len(payload)


def _top_n_cyclomatic(db_path: Path, top_n: int) -> list[dict[str, object]]:
    """Return the ``top_n`` rows by ``cyclomatic.sum`` (highest first).

    The column name carries a literal dot (``"cyclomatic.sum"``) —
    sqlite accepts it as a quoted identifier, no rewrite needed.
    Function-kind rows are the natural unit of comparison; unit-kind
    rows aggregate across the whole file and would dominate the
    ranking unfairly.

    ``path`` carries the source-file path that
    :func:`bca.flatten_spaces` writes into every record (the
    top-level ``FuncSpace`` name), so callers see *which file* the
    most-complex functions live in — distinct from ``name``, which
    is the function identifier.

    ``WHERE "cyclomatic.sum" IS NOT NULL`` guards against the
    SQLite ``ORDER BY ... DESC`` quirk that places NULL values
    *first*, which would otherwise let a row missing the metric
    (e.g., a future caller using ``analyze(..., metrics=['cognitive'])``
    on the same db) dominate the top-N.
    """
    with closing(sqlite3.connect(db_path)) as db:
        db.row_factory = sqlite3.Row
        cur = db.execute(
            'SELECT "path", name, "kind", "cyclomatic.sum" AS cyclomatic_sum, '
            '"start_line", "end_line" '
            "FROM metrics WHERE kind = 'function' "
            'AND "cyclomatic.sum" IS NOT NULL '
            'ORDER BY "cyclomatic.sum" DESC LIMIT ?',
            (top_n,),
        )
        rows = cur.fetchall()
    return [
        {
            "name": row["name"],
            "kind": row["kind"],
            "cyclomatic_sum": row["cyclomatic_sum"],
            "start_line": row["start_line"],
            "end_line": row["end_line"],
            "path": row["path"],
        }
        for row in rows
    ]


def _parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "root",
        type=Path,
        help="Directory to walk for source files.",
    )
    parser.add_argument(
        "db",
        type=Path,
        help="Sqlite database to populate.",
    )
    parser.add_argument(
        "--top-n",
        type=int,
        default=5,
        help="How many top-complexity functions to query back. Default 5.",
    )
    return parser.parse_args(argv)


if __name__ == "__main__":
    args = _parse_args(sys.argv[1:])
    run(args.root, args.db, top_n=args.top_n)
