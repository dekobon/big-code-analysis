"""Flat-record iterator over the nested ``FuncSpace`` tree.

The compiled ``_native`` extension returns analysis results as a
nested ``dict`` that mirrors the CLI's JSON output byte-for-byte.
Most data-science and pipeline consumers want one flat row per
function / class / namespace instead — a shape ready for
``sqlite3.executemany`` or ``pandas.DataFrame.from_records``.

``flatten_spaces`` walks the tree in pre-order and yields one flat,
scalar-only ``dict`` per ``FuncSpace`` node. Metric keys use the
same dotted convention as the CLI's CSV writer (see
``src/output/csv.rs::CSV_HEADER``) — ``cyclomatic.modified.sum``,
``halstead.volume``, ``loc.lloc_average``, … Metric *columns* line
up with CSV_HEADER's metric columns; the identity columns are
**not** identical (CSV uses ``space_name`` / ``space_kind`` and
omits ``parent_name`` / ``depth``; flat records use ``name`` /
``kind`` and add the parent / depth pair). One known metric also
diverges: the ``tokens`` family flattens to ``tokens.tokens``,
``tokens.tokens_average``, ``tokens.tokens_min``, ``tokens.tokens_max``
(the JSON shape), while CSV_HEADER lists ``tokens.sum`` etc. —
the CLI's CSV writer renames those columns. Rename in the consumer
if you need an exact CSV match.

Both walkers (the space tree and each space's metrics subtree) use
explicit stacks rather than recursion, so pathological inputs —
hundreds of nested closures, or a future metric author who adds a
deep metric subtree — cannot exhaust CPython's recursion limit.
The returned iterator is a genuine generator: lazy and single-use.
"""

from __future__ import annotations

from collections.abc import Generator, Mapping
from typing import Any, cast

__all__ = ["flatten_spaces"]

# Top-level FuncSpace fields the walker propagates onto each flat
# record. Any other key (e.g. ``suppressed``, future scalar
# additions) is intentionally **not** copied — phase 3 scopes the
# flat-record contract to function/class identity + metrics. If you
# add a top-level FuncSpace field and want it on the flat record,
# extend this set and the ``_record_for_space`` builder below.
_FORWARDED_FIELDS = ("name", "kind", "start_line", "end_line")


def flatten_spaces(
    result: Mapping[str, Any],
) -> Generator[dict[str, Any], None, None]:
    """Yield one flat ``dict`` per ``FuncSpace`` in *result*.

    *result* is the nested ``dict`` returned by :func:`analyze` or
    :func:`analyze_source`. Each yielded record carries:

    * ``path`` — the analyzed file path (``None`` for
      ``analyze_source``); copied from the root space's ``name``.
      If *result* is a nested space rather than a unit root, this
      field is whatever the passed-in space's ``name`` happens to
      be — pass the full ``analyze()`` result if you want the file
      path semantics.
    * ``name`` — the space's own ``name`` (the literal
      ``"<anonymous>"`` for unnamed closures / function
      expressions; the file path on the root for ``analyze``).
    * ``kind`` — the ``SpaceKind`` string
      (``"function"``, ``"class"``, ``"unit"``, ``"namespace"``, …).
    * ``start_line`` / ``end_line`` — copied from the space.
    * ``parent_name`` — the enclosing space's name (``None`` for
      the root). Note this alone cannot disambiguate same-named
      siblings nested under different parents (e.g. two classes
      ``Inner`` under different outer classes both surface as
      ``parent_name == 'Inner'`` for their own children); pair with
      ``depth`` plus source-order position if you need to reconstruct
      a fully-qualified path.
    * ``depth`` — 0 for the root, +1 per nested level.
    * One flat key per metric leaf, joined by ``"."`` —
      ``cyclomatic.sum``, ``cyclomatic.modified.sum``,
      ``halstead.volume``, ``loc.lloc_average``, etc. Names mirror
      the JSON output's nested-dict shape; see the module docstring
      for the (small) divergence vs the CLI CSV writer's
      ``tokens.*`` column names.

    Records are emitted in pre-order: each parent before its
    children, children in source order. Missing metric subtrees
    (e.g. ``wmc`` on a function-level space, or a metric the caller
    excluded) elide their keys — they are absent, not ``None``.

    The iterator holds references into *result*; do **not** mutate
    *result* while iterating (mutations to not-yet-yielded subtrees
    will be observed).

    Example::

        import sqlite3
        import big_code_analysis as bca

        result = bca.analyze("src/lib.rs")
        if result is None:  # generated/skipped file
            raise SystemExit("nothing to analyze")
        records = list(bca.flatten_spaces(result))
        cols = sorted({k for r in records for k in r})
        # ``flatten_spaces`` keys come from a bounded alphabet
        # (``.``, ``_``, ASCII alnum) so f-string quoting is safe
        # here — sanitize if you ever build records by hand.
        quoted = ", ".join(f'"{c}"' for c in cols)
        placeholders = ", ".join("?" for _ in cols)
        with sqlite3.connect(":memory:") as db:
            db.execute(f"CREATE TABLE m ({quoted})")
            db.executemany(
                f"INSERT INTO m ({quoted}) VALUES ({placeholders})",
                [tuple(r.get(c) for c in cols) for r in records],
            )

    Raises:
        TypeError: if *result* is not a mapping. ``analyze`` can
            return ``None`` for filtered-out inputs (e.g.
            ``skip_generated=True`` matched a generated file);
            callers must filter ``None`` before flattening.
    """
    # Annotation says ``Mapping[str, Any]``; the runtime check is
    # defensive (a caller can pass ``analyze()``'s ``None`` return
    # when ``skip_generated=True`` filters a file, and the error then
    # surfaces here rather than as an attribute error mid-walk).
    if not isinstance(result, Mapping):  # pyright: ignore[reportUnnecessaryIsInstance]
        raise TypeError(
            "flatten_spaces requires the dict returned by analyze() / "
            f"analyze_source(); got {type(result).__name__}"
        )
    return _walk(result)


def _walk(result: Mapping[str, Any]) -> Generator[dict[str, Any], None, None]:
    path = result.get("name")

    # (space, parent_name, depth). Stack is LIFO, so we push children
    # in reverse source order to surface them in source order.
    stack: list[tuple[Mapping[str, Any], str | None, int]] = [(result, None, 0)]
    while stack:
        space, parent_name, depth = stack.pop()
        record: dict[str, Any] = {"path": path}
        for field in _FORWARDED_FIELDS:
            record[field] = space.get(field)
        record["parent_name"] = parent_name
        record["depth"] = depth

        metrics = space.get("metrics")
        if isinstance(metrics, Mapping):
            _flatten_metrics(cast("Mapping[str, Any]", metrics), record)
        yield record

        children = space.get("spaces")
        if not children:
            continue
        own_name = space.get("name")
        # ``children[::-1]`` instead of ``reversed(children)`` so a
        # hand-built dict that supplied an iterable but not a
        # sequence (set, dict_values) fails at the slicing line with
        # a clearer error rather than mid-iteration.
        stack.extend(
            (cast("Mapping[str, Any]", child), own_name, depth + 1)
            for child in children[::-1]
            if isinstance(child, Mapping)
        )


def _flatten_metrics(metrics: Mapping[str, Any], out: dict[str, Any]) -> None:
    """Flatten *metrics* into *out* using dotted keys, iteratively.

    The Rust serializer caps nesting at two levels today
    (``cyclomatic.modified.X``), so a recursive implementation would
    be safe in practice. We use an explicit stack regardless, so a
    future metric author cannot regress the recursion-limit
    contract without also tripping a test (see
    ``test_deep_metric_nesting_does_not_blow_recursion_limit``).

    Keys are emitted in depth-first source-encounter order — the
    same order the original recursive walk would produce. Matters
    for downstream consumers that infer column order from the
    first record's keys (e.g. ``pandas.DataFrame.from_records``).
    Each stack frame is a tagged operation: ``("walk", subtree,
    prefix)`` expands a subtree, ``("set", key, value)`` writes a
    scalar. Subtree expansion pushes its child operations in
    **reverse** source order so ``.pop()`` yields them back in
    source order — the same convention ``_walk`` uses for child
    spaces. Empty-string keys are dropped to avoid nonsense column
    names like ``"halstead."``; the Rust serializer never emits
    them, so this only guards the hand-built-dict surface area.
    """
    stack: list[tuple[Any, ...]] = [("walk", metrics, "")]
    while stack:
        op = stack.pop()
        if op[0] == "set":
            _, key, value = op
            out[key] = value
            continue
        _, subtree, prefix = op
        children: list[tuple[Any, ...]] = []
        for key, value in subtree.items():
            if not key:
                continue
            full = f"{prefix}.{key}" if prefix else key
            if isinstance(value, Mapping):
                children.append(("walk", cast("Mapping[str, Any]", value), full))
            else:
                children.append(("set", full, value))
        stack.extend(reversed(children))
