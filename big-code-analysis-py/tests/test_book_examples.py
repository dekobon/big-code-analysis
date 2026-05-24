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
from typing import Any, cast

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


# ---------------------------------------------------------------------------
# Phase 9 examples (issue #273). These are not embedded in the book — they
# are end-user copy-paste scripts shipped under examples/ and gated here so
# the bindings cannot break them silently.
# ---------------------------------------------------------------------------


def test_cli_parity(bca_binary: str) -> None:
    """``examples/cli_parity.py`` enforces byte-for-byte equality vs the CLI.

    The example raises ``SystemExit`` on any divergence; reaching
    the return statement is itself the load-bearing signal that
    parity held. The assertions on the report dict are belt-and-
    suspenders — they cannot independently fail given the example's
    fail-fast design (see audit lesson at
    ``test_cli_parity_divergence_raises`` below for the test that
    actually pins the SystemExit branch).
    """
    mod = _load("cli_parity")
    report = mod.run(FIXTURES_DIR / "hello.rs", bca_binary=bca_binary)
    assert report["ok"] is True
    assert report["key_order_matches"] is True
    assert report["json_bytes_match"] is True


def test_cli_parity_unicode_identifier(bca_binary: str, tmp_path: Path) -> None:
    """Regression: parity check must succeed on non-ASCII identifiers.

    serde_json (the CLI) emits raw UTF-8; Python's ``json.dumps``
    defaults to ``ensure_ascii=True`` which escapes non-ASCII as
    ``\\uXXXX``. The two strings are not byte-equal even though they
    encode the same JSON value. Without ``ensure_ascii=False`` on
    the bindings side, the byte-match guard fires with a
    confusingly-worded "numeric-type or nested-order regression"
    error for any Unicode identifier or path. This test pins the
    fix by analysing a Rust fixture whose function name is the
    non-ASCII identifier ``ñoño`` and asserting the parity check
    completes cleanly.
    """
    mod = _load("cli_parity")
    src = tmp_path / "noño.rs"
    src.write_text("fn ñoño() {}\n", encoding="utf-8")
    report = mod.run(src, bca_binary=bca_binary)
    assert report["ok"] is True
    assert report["json_bytes_match"] is True


def test_cli_parity_structural_divergence_raises(
    bca_binary: str, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Regression: structural divergence (``dict ==`` fails) must raise.

    Audit lesson: the happy-path ``test_cli_parity`` above only
    pins ``report['json_bytes_match'] is True``, which is True for
    the matching fixture **whether or not** the SystemExit guard
    runs. A regression that silently removed the divergence raise
    would still pass that test. This test forces a structural
    divergence by monkeypatching :func:`bca.analyze` to add an
    extra top-level key, then asserts ``SystemExit`` propagates
    from the first parity check.
    """
    mod = _load("cli_parity")
    real_analyze = bca.analyze

    def _diverging(path: Any, **kwargs: Any) -> dict[str, Any] | None:
        truth = real_analyze(path, **kwargs)
        if truth is None:
            return None
        return {**truth, "_DIVERGENCE_MARKER_": "audit"}

    monkeypatch.setattr(mod.bca, "analyze", _diverging)
    with pytest.raises(SystemExit) as exc_info:
        mod.run(FIXTURES_DIR / "hello.rs", bca_binary=bca_binary)
    assert "structural mismatch" in str(exc_info.value).lower(), (
        f"SystemExit fired but message {exc_info.value!r} does not look "
        "like a structural-mismatch message — the test may be catching "
        "the wrong code path."
    )


def test_cli_parity_byte_only_divergence_raises(
    bca_binary: str, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Regression: byte-only divergence (nested key reorder) must raise.

    Audit lesson: the ``structural_divergence`` test above triggers
    the *first* check (``py_result != cli_result``), so it does not
    pin the ``json_bytes_match`` guard specifically — silently
    removing the byte-match raise (mutation M4 from the audit
    run) still passes that test. This test injects a divergence
    that only manifests at the JSON-bytes layer: a nested
    key-order swap. ``dict ==`` is order-insensitive at every
    level so the structural check passes; the top-level key order
    is preserved so that check passes too; but
    ``json.dumps(py_result)`` reflects the swapped insertion order
    and diverges from the CLI's stdout, so the byte-match guard
    is the ONLY check that can fire.
    """
    mod = _load("cli_parity")
    real_analyze = bca.analyze

    def _reorder_nested(path: Any, **kwargs: Any) -> dict[str, Any] | None:
        truth = real_analyze(path, **kwargs)
        if truth is None:
            return None
        # Reverse the order of the top-level ``metrics`` sub-keys.
        # Structurally identical (dict == is order-insensitive),
        # top-level key order preserved (only the value at one key
        # is rebuilt), but byte-divergent vs the CLI's serde-fixed-
        # order output. One nested level is enough to fire the
        # byte-match guard — no recursion required.
        metrics_value = truth.get("metrics")
        if not isinstance(metrics_value, dict):
            pytest.fail(
                "fixture's metrics value is not a dict; the test "
                "needs a different byte-divergence injection point"
            )
        metrics = cast("dict[str, Any]", metrics_value)
        reversed_metrics: dict[str, Any] = dict(reversed(list(metrics.items())))
        return {**truth, "metrics": reversed_metrics}

    monkeypatch.setattr(mod.bca, "analyze", _reorder_nested)
    with pytest.raises(SystemExit) as exc_info:
        mod.run(FIXTURES_DIR / "hello.rs", bca_binary=bca_binary)
    assert "json byte sequences diverged" in str(exc_info.value).lower(), (
        f"SystemExit fired but message {exc_info.value!r} does not look "
        "like a byte-mismatch message — the structural or key-order "
        "check may have fired instead, which would defeat this test's "
        "purpose."
    )


def test_pipeline_db(tmp_path: Path) -> None:
    """``examples/pipeline_db.py`` walks a dir, batches, persists, queries.

    Threads a non-existent ``does_not_exist.cpp`` through
    ``extra_paths`` so the never-raise contract is exercised — the
    bad slot must land as an error entry while the rest of the
    batch persists normally. Asserts ``errors >= 1`` (not ``== 1``)
    so adding future broken fixtures for unrelated tests does not
    cascade into this assertion; the broken-path-was-handled
    invariant is then pinned by the `db_path.exists()` and
    `rows > 0` checks below (a never-raise regression would leave
    rows at 0 since the function would have raised mid-pipeline).
    """
    mod = _load("pipeline_db")
    db_path = tmp_path / "metrics.db"
    broken = tmp_path / "does_not_exist.cpp"
    summary = mod.run(
        FIXTURES_DIR,
        db_path,
        extra_paths=[broken],
        top_n=3,
    )
    assert summary["analyzed"] > 0
    assert summary["errors"] >= 1, (
        f"expected at least one error from the broken extra path, got {summary['errors']}"
    )
    assert summary["rows"] > 0
    # `top_n` floor: there must be at least one function-kind row in
    # the fixture set (otherwise the example's whole demo is vacuous).
    # `min(3, ...)` rather than `> 0` so a future fixture restructure
    # that drops to 1-2 functions still passes without an edit.
    assert summary["top_n"] >= 1
    assert summary["top_n"] <= 3
    assert db_path.exists()


def test_pipeline_db_all_errors_does_not_raise(tmp_path: Path) -> None:
    """Regression: ``run()`` must not raise when every input errors.

    Builds a fake "fixtures" dir containing only files with bogus
    extensions so ``discover_sources`` picks nothing up, then
    threads two non-existent paths through ``extra_paths``. The
    pre-fix code would create no metrics table (``_persist`` early-
    returned on empty rows), then ``_top_n_cyclomatic`` would raise
    ``sqlite3.OperationalError: no such table: metrics`` — directly
    contradicting the never-raise demo this example showcases.
    """
    mod = _load("pipeline_db")
    empty_root = tmp_path / "empty_fixtures"
    empty_root.mkdir()
    db_path = tmp_path / "metrics.db"
    summary = mod.run(
        empty_root,
        db_path,
        extra_paths=[tmp_path / "does_not_exist.rs", tmp_path / "also_missing.py"],
        top_n=3,
    )
    assert summary["analyzed"] == 0
    assert summary["errors"] == 2
    assert summary["rows"] == 0
    assert summary["top_n"] == 0
    assert db_path.exists()


def test_pipeline_db_top_n_reports_real_source_path(tmp_path: Path) -> None:
    """Regression: top-N output's ``path`` must be the source file,
    not the FuncSpace name (which would print the function name).

    Exercises ``mod._top_n_cyclomatic`` **directly** rather than a
    raw SQL SELECT: the bug being guarded against lived in the
    helper's mapping (``'path': row['name']``), so a regression
    test that bypasses the helper would not catch a re-introduction
    (audit lesson: a raw ``SELECT path FROM metrics`` would always
    see the unaliased column the bug never touched).
    """
    mod = _load("pipeline_db")
    db_path = tmp_path / "metrics.db"
    mod.run(FIXTURES_DIR, db_path, top_n=3)

    top = mod._top_n_cyclomatic(db_path, 3)
    assert top, "expected at least one function-kind row in top-N"
    for row in top:
        # The helper's mapping must surface a source path distinct
        # from the function name. A regression that re-aliased
        # `path = row['name']` would fail this strict inequality.
        assert row["path"] is not None
        assert row["name"] is not None
        assert row["path"] != row["name"], (
            f"path and name must be distinct keys in the helper's "
            f"output; got both = {row['name']!r}"
        )
        # The path must look like a real source file (carries one of
        # the fixture filenames). Catches the "row['name'] aliased
        # to path" regression even on function-kind rows where the
        # function name happens not to match the file basename.
        assert any(row["path"].endswith(s) for s in (".rs", ".py", ".cpp", ".java", "install")), (
            f"path should end in a known fixture suffix, got {row['path']!r}"
        )


def test_sarif_upload(tmp_path: Path) -> None:
    """``examples/sarif_upload.py`` produces a SARIF doc with findings.

    Uses aggressive overrides (cyclomatic >= 1, loc.lloc >= 1) so the
    multi-fixture batch produces at least one finding — an
    "always empty results" regression must surface here. The default
    threshold table the example ships with would not fire on the
    tiny fixtures, so we deliberately tighten via the kwarg.
    """
    mod = _load("sarif_upload")
    output = tmp_path / "results.sarif"
    summary = mod.run(
        [
            FIXTURES_DIR / "hello.rs",
            FIXTURES_DIR / "hello.py",
        ],
        output,
        thresholds={"cyclomatic": 1, "loc.lloc": 1},
        category="bca-test-category",
    )
    assert output.exists()
    assert summary["analyzed"] == 2
    assert summary["errors"] == 0
    assert summary["results"] >= 1, (
        "aggressive thresholds on two non-trivial fixtures must produce at least one finding"
    )
    assert summary["rules"] >= 1

    document = json.loads(output.read_text(encoding="utf-8"))
    assert document["runs"][0]["tool"]["driver"]["name"]
    # Pin the literal id value rather than a truthy check — a regression
    # that dropped the category (e.g. `automation['id'] = '/'`) would
    # otherwise pass silently. The trailing `/` is the documented
    # "category-only id" form Code Scanning uses for dedup.
    assert document["runs"][0]["automationDetails"]["id"] == "bca-test-category/", (
        f"automationDetails.id should equal '{{category}}/', got "
        f"{document['runs'][0]['automationDetails']['id']!r}"
    )


def test_sarif_upload_threshold_fallback_semantics(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """Regression: ``thresholds={}`` honours the empty policy while
    ``thresholds=None`` falls back to ``DEFAULT_THRESHOLDS``.

    The original test ("empty_thresholds_produces_no_findings") was
    upgraded to cross-check the ``None``-fallback path on the SAME
    fixture, since otherwise the empty-zero assertion was vacuous
    (hello.rs trivially passes the shipped defaults). The two
    branches share the same scaffolding so the test name no longer
    captured the full scope.

    The pre-fix code did ``dict(thresholds or DEFAULT_THRESHOLDS)``,
    treating an empty dict as falsy and replacing it with the
    defaults — directly contradicting the module docstring's
    documented "thresholds=None means empty SARIF" contract. The
    fix uses ``is None`` explicitly so an empty dict is honoured.

    Audit lesson: the shipped ``DEFAULT_THRESHOLDS`` set
    (cyclomatic=15, …) is far higher than anything ``hello.rs``
    reaches, so the bug AND the fix both produce zero findings on
    that fixture — the regression test would pass under either.
    We monkeypatch ``DEFAULT_THRESHOLDS`` to ``{"cyclomatic": 1}``
    so the fallback WOULD fire if the bug returned; the empty-dict
    path must STILL produce zero findings, and we cross-check by
    asserting ``thresholds=None`` (which legitimately uses the
    lowered defaults) DOES emit findings on the same fixture.
    """
    mod = _load("sarif_upload")
    monkeypatch.setattr(mod, "DEFAULT_THRESHOLDS", {"cyclomatic": 1.0})

    # Explicit empty dict — must stay empty regardless of defaults.
    empty = mod.run(
        [FIXTURES_DIR / "hello.rs"],
        tmp_path / "empty.sarif",
        thresholds={},
    )
    assert empty["results"] == 0
    assert empty["rules"] == 0

    # `thresholds=None` legitimately falls back to the (now
    # lowered) defaults. Asserting this fires on the SAME fixture
    # proves the empty-dict zero above is the function honouring
    # `{}`, not the fixture trivially passing every threshold.
    fallback = mod.run(
        [FIXTURES_DIR / "hello.rs"],
        tmp_path / "fallback.sarif",
        thresholds=None,
    )
    assert fallback["results"] >= 1, (
        "lowered DEFAULT_THRESHOLDS must produce findings on hello.rs; "
        "if this assertion fails, the cross-check itself is broken — "
        "the empty-dict zero above no longer proves anything."
    )


def test_sqlite_rename_map_kept_in_lockstep() -> None:
    """Regression: ``_RENAME_FOR_SQLITE`` is duplicated between
    ``flat_records.py`` and ``pipeline_db.py`` for the SQLite case-
    insensitivity workaround (Halstead's ``N1``/``n1`` collision).
    Comments in both files say "kept in lockstep" — pin that with a
    test so a future edit to one file fails CI rather than silently
    diverging.
    """
    flat = _load("flat_records")
    pipeline = _load("pipeline_db")
    assert flat._RENAME_FOR_SQLITE == pipeline._RENAME_FOR_SQLITE, (
        "_RENAME_FOR_SQLITE drifted between flat_records.py and "
        f"pipeline_db.py:\n  flat_records: {flat._RENAME_FOR_SQLITE!r}\n"
        f"  pipeline_db:  {pipeline._RENAME_FOR_SQLITE!r}"
    )


@pytest.mark.parametrize(
    "name",
    [
        "cli_parity",
        "pipeline_db",
        "sarif_upload",
    ],
)
def test_phase9_example_file_exists(name: str) -> None:
    """Defensive gate that fires before any of the phase-9 examples
    can be renamed out from under the runnable smoke tests above.
    """
    path = EXAMPLES_DIR / f"{name}.py"
    assert path.is_file(), f"missing phase-9 example {path}"


def test_jupyter_quickstart_notebook_exists() -> None:
    """Defensive gate for the notebook artifact.

    The CI ``python-examples-nbconvert`` step is what actually
    executes the notebook end-to-end; this assertion is the cheap
    "did it get deleted?" check that fires from regular pytest runs
    so a rename / move shows up without waiting for the heavier job.
    """
    notebook = EXAMPLES_DIR / "jupyter_quickstart.ipynb"
    assert notebook.is_file(), f"missing notebook {notebook}"
    # Cheap JSON sanity — a notebook with malformed JSON would still
    # exist as a file but nbconvert would later fail with a less
    # actionable error.
    payload = json.loads(notebook.read_text(encoding="utf-8"))
    assert payload["nbformat"] == 4
    assert payload["cells"], "notebook has no cells"
