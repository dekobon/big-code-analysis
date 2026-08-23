"""Tests for the generated result-shape TypedDicts (#623).

The TypedDicts mirror the ``big_code_analysis::wire`` shapes the bindings
serialize. The static guarantees (``assert_type``, no-cast nested access)
are checked by ``mypy --strict`` / ``pyright`` over this file; the runtime
assertions confirm the live ``analyze`` result actually carries the keys
the TypedDict promises.
"""

from __future__ import annotations

from pathlib import Path
from typing import assert_type

import big_code_analysis as bca
from big_code_analysis import CodeMetricsDict, FuncSpaceDict


def test_analyze_source_result_is_funcspacedict() -> None:
    # analyze_source is annotated to return FuncSpaceDict (#623). The
    # assert_type is the static contract: mypy/pyright fail here if the
    # stub regresses to dict[str, Any].
    result = bca.analyze_source("fn main() {}\n", "rust")
    assert_type(result, FuncSpaceDict)
    assert isinstance(result, dict)


def test_analyze_result_is_optional_funcspacedict(tmp_path: Path) -> None:
    src = tmp_path / "main.rs"
    src.write_bytes(b"fn main() { if true {} }\n")
    result = bca.analyze(src)
    # analyze returns FuncSpaceDict | None (None for generated files).
    assert_type(result, "FuncSpaceDict | None")
    assert result is not None


def test_nested_field_access_needs_no_cast(tmp_path: Path) -> None:
    # The headline win of #623: a nested metric field is reachable under
    # --strict with zero casts. Before, ``result["metrics"]`` was
    # dict[str, Any] and ``["cyclomatic"]`` erased to Unknown.
    src = tmp_path / "main.rs"
    src.write_bytes(b"fn classify(x: int) { if x > 0 {} }\n".replace(b"int", b"i32"))
    result = bca.analyze(src)
    assert result is not None

    metrics: CodeMetricsDict = result["metrics"]
    assert_type(metrics, CodeMetricsDict)

    # ``cyclomatic`` is NotRequired (elided under metrics= selection), so
    # the typed access is the .get()/None-guard pattern a strict consumer
    # writes — no cast, full completion.
    cyclomatic = metrics.get("cyclomatic")
    assert cyclomatic is not None
    # ``sum`` is a required int field on CyclomaticDict; mypy knows its type.
    total: int = cyclomatic["sum"]
    assert total >= 1

    # The recursive ``spaces`` list is typed list[FuncSpaceDict]; iterate
    # and read a child's required ``kind`` field with no annotation help.
    for child in result["spaces"]:
        assert_type(child, FuncSpaceDict)
        assert isinstance(child["kind"], str)


def test_analyze_batch_yields_funcspacedict_or_error(tmp_path: Path) -> None:
    src = tmp_path / "a.rs"
    src.write_bytes(b"fn main() {}\n")
    results = bca.analyze_batch([src])
    assert len(results) == 1
    entry = results[0]
    # The non-error branch narrows to FuncSpaceDict, so the metric table is
    # typed without a cast.
    assert isinstance(entry, dict)
    assert_type(entry, FuncSpaceDict)
    assert "loc" in entry["metrics"]


def test_metric_field_types_are_concrete(tmp_path: Path) -> None:
    # Spot-check that the int / float distinction the spec encodes holds at
    # runtime: integer counts are int, derived averages are float (or None
    # for the non-finite / not-applicable case, #531).
    src = tmp_path / "main.rs"
    src.write_bytes(b"fn main() { let _ = 1 + 2; }\n")
    result = bca.analyze(src)
    assert result is not None
    loc = result["metrics"].get("loc")
    assert loc is not None
    assert isinstance(loc["sloc"], int)
    # ``sloc_average`` is float | None; this file is populated and finite,
    # so it is concretely a float here (not None). Pinning the float
    # positively — rather than the looser ``is None or isinstance`` — keeps
    # the int/float distinction the test name promises load-bearing: a
    # regression collapsing the average to None would otherwise slip past.
    avg = loc["sloc_average"]
    assert isinstance(avg, float)
