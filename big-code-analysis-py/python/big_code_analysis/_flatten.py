"""Flat-record iterator over the nested ``FuncSpace`` tree.

The compiled ``_native`` extension returns analysis results as a
nested ``dict`` that mirrors the CLI's JSON output byte-for-byte.
Most data-science and pipeline consumers want one flat row per
function / class / namespace instead — a shape ready for
``sqlite3.executemany`` or ``pandas.DataFrame.from_records``.

``flatten_spaces`` walks the tree in pre-order and yields one flat,
scalar-only ``dict`` per ``FuncSpace`` node. Metric keys use the
same dotted convention as the CLI's CSV writer (see
``src/output/csv.rs::CSV_HEADER``), so flat records correlate
column-for-column against CSV output produced by ``bca metrics
--output-format csv``.

The walker keeps its own explicit stack rather than recursing, so
pathological inputs (hundreds of nested closures, deeply nested
class hierarchies) cannot exhaust CPython's recursion limit. The
returned iterator is a genuine generator — lazy and single-use.
"""

from __future__ import annotations

from collections.abc import Iterator, Mapping
from typing import Any

__all__ = ["flatten_spaces"]


def flatten_spaces(result: Mapping[str, Any]) -> Iterator[dict[str, Any]]:
    """Yield one flat ``dict`` per ``FuncSpace`` in *result*.

    *result* is the nested ``dict`` returned by :func:`analyze` or
    :func:`analyze_source`. Each yielded record carries:

    * ``path`` — the analyzed file path (``None`` for
      ``analyze_source``); copied from the root space's ``name``.
    * ``name`` — the space's own ``name`` (the literal
      ``"<anonymous>"`` for unnamed closures / function
      expressions; the file path on the root for ``analyze``).
    * ``kind`` — the ``SpaceKind`` string
      (``"function"``, ``"class"``, ``"unit"``, ``"namespace"``, …).
    * ``start_line`` / ``end_line`` — copied from the space.
    * ``parent_name`` — the enclosing space's name (``None`` for
      the root).
    * ``depth`` — 0 for the root, +1 per nested level.
    * One flat key per metric leaf, joined by ``"."`` —
      ``cyclomatic.sum``, ``cyclomatic.modified.sum``,
      ``halstead.volume``, ``loc.lloc_average``, etc. Keys match the
      CLI's CSV_HEADER column names verbatim.

    Records are emitted in pre-order: each parent before its
    children, children in source order. Missing metric subtrees
    (e.g. ``wmc`` on a function-level space, or a metric the caller
    excluded) elide their keys — they are absent, not ``None``.

    Example::

        import sqlite3
        import big_code_analysis as bca

        records = list(bca.flatten_spaces(bca.analyze("src/lib.rs")))
        cols = sorted({k for r in records for k in r})
        with sqlite3.connect(":memory:") as db:
            db.execute(
                f"CREATE TABLE m ({', '.join(f'\"{c}\"' for c in cols)})"
            )
            db.executemany(
                f"INSERT INTO m ({', '.join(f'\"{c}\"' for c in cols)}) "
                f"VALUES ({', '.join('?' for _ in cols)})",
                [tuple(r.get(c) for c in cols) for r in records],
            )

    Raises:
        TypeError: if *result* is not a mapping. ``analyze`` can
            return ``None`` for filtered-out inputs (e.g.
            ``skip_generated=True`` matched a generated file);
            callers must filter ``None`` before flattening.
    """
    if not isinstance(result, Mapping):
        raise TypeError(
            "flatten_spaces requires the dict returned by analyze() / "
            f"analyze_source(); got {type(result).__name__}"
        )
    return _walk(result)


def _walk(result: Mapping[str, Any]) -> Iterator[dict[str, Any]]:
    path = result.get("name")

    # (space, parent_name, depth). Stack is LIFO, so we push children
    # in reverse source order to surface them in source order.
    stack: list[tuple[Mapping[str, Any], str | None, int]] = [(result, None, 0)]
    while stack:
        space, parent_name, depth = stack.pop()
        record: dict[str, Any] = {
            "path": path,
            "name": space.get("name"),
            "kind": space.get("kind"),
            "start_line": space.get("start_line"),
            "end_line": space.get("end_line"),
            "parent_name": parent_name,
            "depth": depth,
        }
        metrics = space.get("metrics")
        if isinstance(metrics, Mapping):
            _flatten_metrics(metrics, "", record)
        yield record

        children = space.get("spaces")
        if not children:
            continue
        own_name = space.get("name")
        for child in reversed(children):
            if isinstance(child, Mapping):
                stack.append((child, own_name, depth + 1))


def _flatten_metrics(
    subtree: Mapping[str, Any], prefix: str, out: dict[str, Any]
) -> None:
    """Recursively flatten *subtree* into *out* using dotted keys."""
    for key, value in subtree.items():
        full = f"{prefix}.{key}" if prefix else key
        if isinstance(value, Mapping):
            _flatten_metrics(value, full, out)
        else:
            out[full] = value
