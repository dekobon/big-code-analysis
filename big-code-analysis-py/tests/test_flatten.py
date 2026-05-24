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

import big_code_analysis as bca
import pytest

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
    # The filter pins ``name == "<anonymous>"`` by construction; assert
    # the paired kind to catch a future Rust-side change that introduces
    # a ``SpaceKind::Closure`` variant.
    for record in anon:
        assert record["kind"] == "function"


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
                    f"non-scalar value at {fixture}::{key} → {type(value).__name__}"
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


def test_iterator_is_actually_lazy_not_pre_materialized() -> None:
    """Build a very wide synthetic tree and consume just the first
    record. A pre-materialising implementation would walk every
    child before yielding — a genuinely lazy one stops at depth 0.

    We detect the difference by counting how many ``.get`` lookups
    the walker performs on a mutation-tracking dict subclass: the
    root record requires ~7 ``get`` calls (one per forwarded field
    plus metrics + spaces), while pre-materialising the whole tree
    would invoke ``get`` on every one of the 5_000 child nodes
    too. The threshold is set well above the per-record cost and
    well below the cost of full materialisation."""
    width = 5_000

    class CountingDict(dict[str, Any]):
        gets: int = 0

        def get(self, key: str, default: Any = None) -> Any:
            CountingDict.gets += 1
            return super().get(key, default)

    children = [
        CountingDict(
            name=f"c{i}",
            kind="function",
            start_line=1,
            end_line=1,
            spaces=[],
            metrics={},
        )
        for i in range(width)
    ]
    root = CountingDict(
        name=None,
        kind="unit",
        start_line=1,
        end_line=1,
        spaces=children,
        metrics={},
    )

    CountingDict.gets = 0
    gen = bca.flatten_spaces(root)
    first = next(gen)
    after_first = CountingDict.gets

    assert first["kind"] == "unit"
    # Lazy: only the root's fields are visited before the first
    # yield. A pre-materialising walker would have visited every
    # child by now (~ width x 7 lookups).
    assert after_first < 100, (
        f"first yield touched {after_first} dict lookups — generator "
        "appears to pre-materialize children"
    )


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


@pytest.mark.parametrize("fixture", ["hello.py", "hello.rs", "Hello.java", "hello.cpp"])
def test_record_count_matches_total_descendants(fixture: str) -> None:
    """Every node in the input tree is yielded exactly once."""
    result = bca.analyze(FIXTURES / fixture)
    assert result is not None
    records = list(bca.flatten_spaces(result))
    assert len(records) == _count_descendants(result)


@pytest.mark.parametrize("fixture", ["hello.py", "hello.rs", "Hello.java", "hello.cpp"])
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
    node: dict[str, Any] = leaf
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


def test_deep_metric_nesting_does_not_blow_recursion_limit() -> None:
    """The metrics flattener uses an explicit stack too, so a future
    metric author who introduces a deep nested metric subtree cannot
    regress the recursion-limit guarantee. The Rust serializer caps
    nesting at 2 levels today (``cyclomatic.modified.X``); this test
    synthesizes a 300-deep metric subtree and walks it under a tight
    recursion limit to pin the contract."""
    metric_depth = 300
    metrics: dict[str, Any] = {"leaf": 1.0}
    for i in range(metric_depth):
        metrics = {f"m{i}": metrics}
    root: dict[str, Any] = {
        "name": None,
        "kind": "unit",
        "start_line": 1,
        "end_line": 1,
        "spaces": [],
        "metrics": metrics,
    }

    original_limit = sys.getrecursionlimit()
    sys.setrecursionlimit(100)
    try:
        record = next(iter(bca.flatten_spaces(root)))
    finally:
        sys.setrecursionlimit(original_limit)

    # The single deep-nested leaf flattens to one dotted key.
    expected_key = ".".join([f"m{i}" for i in range(metric_depth - 1, -1, -1)] + ["leaf"])
    assert record[expected_key] == 1.0


def test_empty_string_metric_keys_are_skipped() -> None:
    """Empty-string keys in a metric subtree would otherwise produce
    nonsense column names (``""`` at the root, ``"halstead."`` when
    nested). Skip them — the Rust serializer never emits empty
    field names, but ``flatten_spaces`` accepts arbitrary mappings."""
    root: dict[str, Any] = {
        "name": None,
        "kind": "unit",
        "start_line": 1,
        "end_line": 1,
        "spaces": [],
        "metrics": {"": 1.0, "halstead": {"": 2.0, "volume": 3.0}},
    }
    record = next(iter(bca.flatten_spaces(root)))

    assert "" not in record
    assert "halstead." not in record
    assert record["halstead.volume"] == 3.0


def test_metric_keys_emitted_in_source_order() -> None:
    """``_flatten_metrics`` must preserve the JSON output's encounter
    order — matters for ``pandas.DataFrame.from_records`` and
    similar consumers that infer column order from the first
    record's keys. The Rust serializer emits ``nargs`` first and
    ``wmc`` (when present) last; assert relative ordering of three
    representative families to pin the contract.

    Regression guard for the iterative-refactor LIFO order asymmetry
    that the initial /code-review fix introduced (subtrees popped in
    reverse encounter order)."""
    result = bca.analyze_source("def f(x): return x + 1\n", "python")
    record = next(iter(bca.flatten_spaces(result)))
    keys = list(record)

    nargs_idx = keys.index("nargs.total")
    cognitive_idx = keys.index("cognitive.sum")
    cyclomatic_idx = keys.index("cyclomatic.sum")
    # JSON emits nargs → nexits → cognitive → cyclomatic → halstead …
    assert nargs_idx < cognitive_idx < cyclomatic_idx, (
        f"metric keys out of JSON encounter order: nargs@{nargs_idx}, "
        f"cognitive@{cognitive_idx}, cyclomatic@{cyclomatic_idx}"
    )

    # Doubly-nested keys (cyclomatic.modified.*) must follow the
    # parent's direct scalars (cyclomatic.sum / .average / .min / .max)
    # — depth-first source order at each level.
    cyclomatic_max_idx = keys.index("cyclomatic.max")
    cyclomatic_modified_sum_idx = keys.index("cyclomatic.modified.sum")
    assert cyclomatic_max_idx < cyclomatic_modified_sum_idx


def test_non_mapping_metrics_value_is_silently_skipped() -> None:
    """The defensive ``isinstance(metrics, Mapping)`` guard in
    ``_walk`` lets a hand-built dict whose ``metrics`` is ``None``,
    a scalar, or a list flow through without crashing or emitting
    spurious keys."""
    for bad_metrics in (None, 1.0, "not a dict", [1, 2, 3]):
        root: dict[str, Any] = {
            "name": None,
            "kind": "unit",
            "start_line": 1,
            "end_line": 1,
            "spaces": [],
            "metrics": bad_metrics,
        }
        record = next(iter(bca.flatten_spaces(root)))

        # Identity columns still populate; no metric keys present.
        assert record["kind"] == "unit"
        metric_keys = [k for k in record if "." in k]
        assert metric_keys == [], (
            f"expected no metric keys for metrics={bad_metrics!r}, got {metric_keys}"
        )


def test_non_mapping_child_in_spaces_is_silently_skipped() -> None:
    """Non-Mapping items in ``space["spaces"]`` (None, scalars,
    strings) are filtered out by the ``isinstance(child, Mapping)``
    guard in ``_walk``. Real ``analyze()`` output never produces
    these, but the function accepts arbitrary mappings."""
    real_child: dict[str, Any] = {
        "name": "good",
        "kind": "function",
        "start_line": 1,
        "end_line": 1,
        "spaces": [],
        "metrics": {},
    }
    root: dict[str, Any] = {
        "name": None,
        "kind": "unit",
        "start_line": 1,
        "end_line": 1,
        "spaces": [None, "not a dict", 42, real_child],
        "metrics": {},
    }
    records = list(bca.flatten_spaces(root))

    # Only the unit root + the one real Mapping child should be yielded.
    assert len(records) == 2
    assert [r["name"] for r in records] == [None, "good"]


def test_tokens_metric_keys_follow_json_shape_not_csv_header() -> None:
    """Regression-pins the README/docstring's documented divergence:
    the JSON output emits ``tokens.tokens``/``tokens_average``/… ,
    not the CLI's CSV_HEADER ``tokens.sum``/``tokens.average``.

    If the Rust serializer is ever fixed to emit ``sum``/``average``,
    this test fails and the README + CHANGELOG callouts must be
    updated (or deleted)."""
    result = bca.analyze_source("def f(x): return x + 1\n", "python")
    record = next(iter(bca.flatten_spaces(result)))

    # JSON-shape keys present.
    assert "tokens.tokens" in record
    assert "tokens.tokens_average" in record
    # CSV_HEADER-shape keys absent (still a known divergence today).
    assert "tokens.sum" not in record
    assert "tokens.average" not in record


@pytest.mark.parametrize("bad", [None, [], 42, "a", (1, 2)])
def test_rejects_non_mapping(bad: Any) -> None:
    """TypeError fires eagerly at call time, not lazily on first
    ``next()`` — so caller bugs surface at the call site."""
    with pytest.raises(TypeError, match="flatten_spaces requires"):
        bca.flatten_spaces(bad)
