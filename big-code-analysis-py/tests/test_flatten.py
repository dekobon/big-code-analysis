"""Tests for ``bca.flatten_spaces``.

Covers the issue-#267 contract: pre-order walk, scalar-only flat
records, dotted metric keys matching the CLI's CSV_HEADER,
explicit-stack traversal (no recursion-limit blow-up), and the
edge cases enumerated in the issue's "Edge cases" block.

Run via::

    maturin develop
    python -m pytest big-code-analysis-py/tests/test_flatten.py
"""

from __future__ import annotations

import inspect
import sys
from collections.abc import Mapping
from pathlib import Path
from typing import Any

import pytest

import big_code_analysis as bca

FIXTURES = Path(__file__).parent / "fixtures"


def _count_descendants(space: Mapping[str, Any]) -> int:
    """Total node count rooted at *space* (the space itself + every
    descendant) — used to verify ``flatten_spaces`` yields every
    node exactly once."""
    total = 1
    for child in space.get("spaces") or ():
        total += _count_descendants(child)
    return total


# ── Single function / no nesting ───────────────────────────────


def test_single_function_yields_two_records() -> None:
    result = bca.analyze_source("def f(): pass\n", "python")
    records = list(bca.flatten_spaces(result))

    assert len(records) == 2
    assert [r["kind"] for r in records] == ["unit", "function"]
    assert [r["depth"] for r in records] == [0, 1]
    # analyze_source has no path, so parent_name at depth-1 is also None
    assert [r["parent_name"] for r in records] == [None, None]
    assert [r["path"] for r in records] == [None, None]
    assert records[1]["name"] == "f"


def test_empty_file_yields_one_record() -> None:
    result = bca.analyze_source("", "python")
    records = list(bca.flatten_spaces(result))

    assert len(records) == 1
    assert records[0]["kind"] == "unit"
    assert records[0]["depth"] == 0
    assert records[0]["parent_name"] is None


# ── Nested classes / methods (Java) ────────────────────────────


def test_nested_classes_and_methods() -> None:
    source = "class A { void f() {} class B { void g() {} } }"
    result = bca.analyze_source(source, "java")
    records = list(bca.flatten_spaces(result))

    # unit → A → f, A → B → g.  Pre-order, source order.
    tuples = [(r["kind"], r["name"], r["depth"], r["parent_name"]) for r in records]
    assert tuples == [
        ("unit", None, 0, None),
        ("class", "A", 1, None),
        ("function", "f", 2, "A"),
        ("class", "B", 2, "A"),
        ("function", "g", 3, "B"),
    ]


# ── Anonymous functions (closures) ─────────────────────────────


def test_anonymous_function_preserves_marker() -> None:
    """Anonymous spaces surface as ``kind="function"`` with the
    literal ``name="<anonymous>"`` — ``flatten_spaces`` is a
    pass-through, it does not normalize to ``None``.

    Rust closures generate the marker; Python lambdas don't emit
    their own space at all (they just bump ``nom.closures``).
    """
    source = "fn outer() { let f = |x| x + 1; let _ = f(2); }"
    result = bca.analyze_source(source, "rust")
    records = list(bca.flatten_spaces(result))

    anon = [r for r in records if r["name"] == "<anonymous>"]
    assert anon, "expected at least one anonymous space in Rust closure source"
    for record in anon:
        assert record["kind"] == "function"
        assert record["name"] == "<anonymous>"


# ── Metric flattening: dotted keys, no nested-dict leaks ───────


def test_halstead_leaves_flattened_to_dotted_keys() -> None:
    result = bca.analyze_source("def f(x): return x + 1\n", "python")
    record = next(iter(bca.flatten_spaces(result)))

    for key in (
        "halstead.volume",
        "halstead.difficulty",
        "halstead.n1",
        "halstead.N1",
    ):
        assert key in record, f"missing dotted key {key!r} in record"
        assert isinstance(record[key], (int, float))

    # No bare nested-dict leak.
    assert "halstead" not in record


def test_cyclomatic_modified_double_nesting() -> None:
    """``cyclomatic.modified`` is the only doubly-nested metric in
    the tree; its leaves must surface as ``cyclomatic.modified.X``
    with no intermediate bare-subtree key."""
    result = bca.analyze_source("def f(x): return x + 1\n", "python")
    record = next(iter(bca.flatten_spaces(result)))

    assert "cyclomatic.modified.sum" in record
    assert "cyclomatic.modified.average" in record
    # The intermediate subtree must not leak as a key.
    assert "cyclomatic.modified" not in record


def test_record_values_are_scalars() -> None:
    """No dict / list values may leak into a flat record — sweep a
    multi-language fixture set to guard against future metric
    additions that introduce a fresh nested subtree."""
    scalar_types = (type(None), bool, int, float, str)
    for fixture in ("hello.py", "hello.rs", "Hello.java", "hello.cpp"):
        result = bca.analyze(FIXTURES / fixture)
        assert result is not None
        for record in bca.flatten_spaces(result):
            for key, value in record.items():
                assert isinstance(value, scalar_types), (
                    f"non-scalar value at {fixture}::{key} → "
                    f"{type(value).__name__}"
                )


# ── Iterator semantics ─────────────────────────────────────────


def test_iterator_is_lazy_and_single_use() -> None:
    result = bca.analyze_source("def f(): pass\n", "python")
    gen = bca.flatten_spaces(result)

    assert inspect.isgenerator(gen)
    first_pass = list(gen)
    assert first_pass, "generator must yield at least the unit record"
    # Second consumption is empty (single-use semantics).
    assert list(gen) == []


# ── Round-trip against hand-curated fixtures ───────────────────


def test_round_trip_kind_name_lines_python() -> None:
    result = bca.analyze(FIXTURES / "hello.py")
    assert result is not None
    records = list(bca.flatten_spaces(result))

    expected = [
        ("unit", str(FIXTURES / "hello.py"), 1, 4),
        ("function", "hello", 1, 4),
    ]
    actual = [(r["kind"], r["name"], r["start_line"], r["end_line"]) for r in records]
    assert actual == expected


def test_round_trip_kind_name_lines_java() -> None:
    result = bca.analyze(FIXTURES / "Hello.java")
    assert result is not None
    records = list(bca.flatten_spaces(result))

    expected = [
        ("unit", str(FIXTURES / "Hello.java"), 1, 8),
        ("class", "Hello", 1, 8),
        ("function", "greet", 2, 7),
    ]
    actual = [(r["kind"], r["name"], r["start_line"], r["end_line"]) for r in records]
    assert actual == expected


@pytest.mark.parametrize(
    "fixture", ["hello.py", "hello.rs", "Hello.java", "hello.cpp"]
)
def test_record_count_matches_total_descendants(fixture: str) -> None:
    """Every node in the input tree is yielded exactly once."""
    result = bca.analyze(FIXTURES / fixture)
    assert result is not None
    records = list(bca.flatten_spaces(result))
    assert len(records) == _count_descendants(result)


@pytest.mark.parametrize(
    "fixture", ["hello.py", "hello.rs", "Hello.java", "hello.cpp"]
)
def test_path_field_propagated_to_every_record(fixture: str) -> None:
    """For file-based ``analyze``, every flat record carries the
    same ``path`` (= the analyzed file)."""
    path = FIXTURES / fixture
    result = bca.analyze(path)
    assert result is not None
    for record in bca.flatten_spaces(result):
        assert record["path"] == str(path)


# ── Synthetic edge cases ───────────────────────────────────────


def test_deep_nesting_does_not_blow_recursion_limit() -> None:
    """Build a 300-deep synthetic tree and walk it under a tight
    recursion limit. Documents the explicit-stack contract: a
    recursive walker would die at this depth."""
    depth = 300
    leaf: dict[str, Any] = {
        "name": "leaf",
        "kind": "function",
        "start_line": 1,
        "end_line": 1,
        "spaces": [],
        "metrics": {},
    }
    node = leaf
    for i in range(depth - 1):
        node = {
            "name": f"n{i}",
            "kind": "function",
            "start_line": 1,
            "end_line": 1,
            "spaces": [node],
            "metrics": {},
        }
    root: dict[str, Any] = {
        "name": None,
        "kind": "unit",
        "start_line": 1,
        "end_line": 1,
        "spaces": [node],
        "metrics": {},
    }

    original_limit = sys.getrecursionlimit()
    sys.setrecursionlimit(100)
    try:
        records = list(bca.flatten_spaces(root))
    finally:
        sys.setrecursionlimit(original_limit)

    # 1 unit + 300 nested function nodes
    assert len(records) == 1 + depth
    assert [r["depth"] for r in records] == list(range(1 + depth))


def test_missing_metric_subtree_is_omitted_not_none() -> None:
    """A metric subtree absent from input produces no key — not a
    ``None`` value. Covers the "Halstead disabled via metrics="
    edge case from the issue."""
    root: dict[str, Any] = {
        "name": None,
        "kind": "unit",
        "start_line": 1,
        "end_line": 1,
        "spaces": [],
        "metrics": {"cyclomatic": {"sum": 1.0}},
    }
    record = next(iter(bca.flatten_spaces(root)))

    assert record["cyclomatic.sum"] == 1.0
    assert "halstead.volume" not in record
    assert "halstead" not in record


@pytest.mark.parametrize("bad", [None, [], 42, "a", (1, 2)])
def test_rejects_non_mapping(bad: Any) -> None:
    """TypeError fires eagerly at call time, not lazily on first
    ``next()`` — so caller bugs surface at the call site."""
    with pytest.raises(TypeError, match="flatten_spaces requires"):
        bca.flatten_spaces(bad)
