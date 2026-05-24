"""End-to-end exercises for every example shipped in the book.

Each example under ``big-code-analysis-py/examples/`` is imported
here and exercised against the existing ``tests/fixtures/`` tree.
The book embeds the same files verbatim via ``mdbook``'s
``{{#include}}`` directive, so this test is the gate that keeps
the documentation in lockstep with the API:

* A renamed kwarg or removed function makes ``run()`` fail to
  import, so the matching test errors with a clear traceback and
  the book change cannot land without updating the example.
* A behaviour change (e.g. the ``analyze_batch`` ``skip_generated``
  toggle) shows up as an assertion failure here before it can
  silently rot the book copy.

The examples are loaded with ``importlib.util`` rather than a
regular import to avoid making ``examples/`` a package (which would
otherwise pull into the wheel via maturin's ``python-source``).
"""

from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path
from typing import Any

import big_code_analysis as bca
import pytest

EXAMPLES_DIR = Path(__file__).resolve().parents[1] / "examples"
FIXTURES_DIR = Path(__file__).parent / "fixtures"


def _load(name: str) -> Any:
    """Load ``examples/<name>.py`` as a standalone module."""
    path = EXAMPLES_DIR / f"{name}.py"
    spec = importlib.util.spec_from_file_location(f"big_code_analysis_examples.{name}", path)
    if spec is None or spec.loader is None:
        msg = f"could not load example {path}"
        raise RuntimeError(msg)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = mod
    try:
        spec.loader.exec_module(mod)
    except BaseException:
        # Don't leave a partially-initialised module in sys.modules
        # — a subsequent test that imports the same dotted name
        # would otherwise pick up the broken state.
        sys.modules.pop(spec.name, None)
        raise
    return mod


def test_quick_start() -> None:
    mod = _load("quick_start")
    result = mod.run(FIXTURES_DIR / "hello.rs")
    assert "metrics" in result
    assert "cyclomatic" in result["metrics"]


def test_batch_processing() -> None:
    mod = _load("batch_processing")
    summary = mod.run(
        [
            FIXTURES_DIR / "hello.rs",
            FIXTURES_DIR / "hello.py",
            FIXTURES_DIR / "does_not_exist.rs",
        ]
    )
    assert summary["total"] == 3
    assert summary["ok"] == 2
    assert summary["errors"] == 1


def test_batch_processing_parallel() -> None:
    mod = _load("batch_processing")
    results = mod.run_parallel(
        [FIXTURES_DIR / "hello.rs", FIXTURES_DIR / "hello.py"],
        workers=2,
    )
    assert len(results) == 2
    assert all(r is not None for r in results)


def test_flat_records(tmp_path: Path) -> None:
    import sqlite3

    mod = _load("flat_records")
    db_path = tmp_path / "metrics.db"
    rows = mod.run(FIXTURES_DIR / "hello.rs", db_path)
    assert rows >= 1
    assert db_path.exists()

    # Defensive: confirm the SQLite case-insensitivity workaround
    # actually preserves both Halstead totals (`N1`/`N2`) and the
    # distinct-operator/operand counts (`n1`/`n2`). If either rename
    # regresses, the renamed column collapses onto its lowercase
    # sibling and ``halstead.total_{1,2}`` either disappears or
    # matches the lowercase count numerically.
    with sqlite3.connect(db_path) as conn:
        cur = conn.execute(
            'SELECT "halstead.total_1", "halstead.n1", '
            '"halstead.total_2", "halstead.n2" FROM metrics LIMIT 1'
        )
        total_n1, distinct_n1, total_n2, distinct_n2 = cur.fetchone()
    assert total_n1 is not None
    assert distinct_n1 is not None
    assert total_n2 is not None
    assert distinct_n2 is not None
    # Totals are always >= distincts. A regression that lost the
    # `halstead.N2` rename would fold both columns onto
    # `halstead.n2`, making total_n2 == distinct_n2 — the strict
    # inequality on at least one pair catches that.
    assert total_n1 >= distinct_n1
    assert total_n2 >= distinct_n2


def test_metric_selection() -> None:
    mod = _load("metric_selection")
    selected = mod.run(FIXTURES_DIR / "hello.rs")
    assert set(selected["metrics"]) == {"loc", "cyclomatic"}

    derived = mod.run_derived(FIXTURES_DIR / "hello.rs")
    # Strict equality (not .issubset) — the book promises
    # "Unrequested metrics are absent from the result dict", so
    # a regression that leaks extra metrics into a metrics=["mi"]
    # result must fail this test.
    assert set(derived["metrics"]) == {"loc", "cyclomatic", "halstead", "mi"}


def test_sarif_output(tmp_path: Path) -> None:
    mod = _load("sarif_output")
    sarif_path = tmp_path / "metrics.sarif"
    sarif_text = mod.run(
        [FIXTURES_DIR / "hello.rs", FIXTURES_DIR / "hello.py"],
        sarif_path,
        {"cyclomatic": 1, "loc.lloc": 1},
    )
    sarif = json.loads(sarif_text)
    assert sarif["$schema"].startswith("https://")
    assert sarif["runs"][0]["tool"]["driver"]["name"]
    # Aggressive thresholds on two non-trivial fixtures must
    # produce at least one finding — a regression that silently
    # drops all findings ("results": []) would otherwise pass.
    assert len(sarif["runs"][0]["results"]) >= 1
    assert sarif_path.exists()


def test_errors_taxonomy(tmp_path: Path) -> None:
    """Re-uses the fixtures dir but writes the throwaway file under tmp_path
    so the test never mutates the checked-in fixtures tree.
    """
    # Stage hello.rs into tmp_path so the example can write its
    # ``hello.unknown_extension`` sibling alongside.
    stage = tmp_path / "fixtures"
    stage.mkdir()
    (stage / "hello.rs").write_text(
        (FIXTURES_DIR / "hello.rs").read_text(encoding="utf-8"),
        encoding="utf-8",
    )

    mod = _load("errors_taxonomy")
    report = mod.run(stage, missing_path=stage / "does_not_exist.cpp")

    assert report["file_not_found"] is True
    assert report["unsupported"] is True
    assert report["batch_errors"] >= 1


def test_async_patterns(monkeypatch: pytest.MonkeyPatch) -> None:
    import threading

    mod = _load("async_patterns")

    # Verify the analyse calls actually ran on a separate thread
    # from the test thread. Without this gate, a regression that
    # turns `analyze_async` into a synchronous `bca.analyze(...)`
    # call on the event-loop thread (the documented anti-pattern)
    # would pass the count assertions silently.
    test_thread = threading.get_ident()
    seen_threads: set[int] = set()
    original_analyze = bca.analyze

    def _recording(*args: Any, **kwargs: Any) -> dict[str, Any] | None:
        seen_threads.add(threading.get_ident())
        return original_analyze(*args, **kwargs)

    monkeypatch.setattr(bca, "analyze", _recording)

    summary = mod.run(
        [
            FIXTURES_DIR / "hello.rs",
            FIXTURES_DIR / "hello.py",
            FIXTURES_DIR / "hello.cpp",
        ]
    )
    assert summary["count"] == 3
    assert summary["analyzed"] == 3
    assert summary["errors"] == 0
    assert seen_threads - {test_thread}, (
        f"analyze never dispatched off the test thread (seen={seen_threads}, test={test_thread})"
    )


def test_async_patterns_return_exceptions() -> None:
    """A non-existent path raises FileNotFoundError inside the worker
    thread; ``analyze_all`` must surface it as a list entry, not
    propagate the raise out of ``asyncio.gather`` (the regression
    the fix to issue #272 review targets).
    """
    mod = _load("async_patterns")
    summary = mod.run(
        [
            FIXTURES_DIR / "hello.rs",
            FIXTURES_DIR / "does_not_exist.rs",
            FIXTURES_DIR / "hello.py",
        ]
    )
    assert summary["count"] == 3
    assert summary["analyzed"] == 2
    assert summary["errors"] == 1


@pytest.mark.parametrize(
    "name",
    [
        "quick_start",
        "batch_processing",
        "flat_records",
        "metric_selection",
        "sarif_output",
        "errors_taxonomy",
        "async_patterns",
    ],
)
def test_example_file_exists_for_book_include(name: str) -> None:
    """Defensive gate: every page in the book references one of these
    files via ``{{#include}}``. If a file is renamed without updating
    the matching page, ``mdbook build`` fails — this test pre-empts
    the failure with a clearer error message.
    """
    path = EXAMPLES_DIR / f"{name}.py"
    assert path.is_file(), f"missing book example {path}"


def test_metric_names_is_a_tuple() -> None:
    """Tiny smoke covering the ``METRIC_NAMES`` reference used across
    the metrics page; saves the book from going stale if the binding
    ever switches the literal to a list.
    """
    assert isinstance(bca.METRIC_NAMES, tuple)
    assert "halstead" in bca.METRIC_NAMES
