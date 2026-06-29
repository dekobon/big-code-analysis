#!/usr/bin/env python3
"""Library-wheel import + analyse smoke for ``big_code_analysis``.

Extracted from the inline heredoc in ``.github/workflows/python-wheels.yml``
so the load-bearing assertions are visible to reviewers, lintable, and
runnable per-PR / locally rather than only on a ``v*`` tag push (#995).
Three of these checks rotted into a release blocker exactly because the
script was buried in workflow YAML: the #614 ``AnalysisError`` ->
``AnalysisFailure`` rename was mirrored in ``tests/test_batch.py`` but not
here, so the smoke only failed when the ``v2.0.0`` tag forced it to run.

Run it against any importable build of the bindings::

    maturin develop -m big-code-analysis-py/Cargo.toml
    python scripts/smoke/lib_wheel_smoke.py

The workflow runs the identical file against the freshly built wheel, and
``make smoke-lib`` / the ``smoke-dryrun.yml`` PR job run it against a
``maturin develop`` build — a single source of truth for all three.

Assertions are deliberately VALUE-bearing, not just shape-bearing: a
binding regression that returned ``{"spaces": []}`` would satisfy a bare
``isinstance(result, dict)`` check but fail the ``records[*]["name"] ==
"add"`` check below. Each ``_check_*`` helper pins one public API surface
that #103 enumerates.
"""

import json
import tempfile
from pathlib import Path

import big_code_analysis as bca
from big_code_analysis import FuncSpaceDict

# Shared fixture: one function spanning the file plus one `if` branch.
_ADD_SRC = "def add(a, b):\n    if a > b:\n        return a\n    return b\n"


def _check_package_loaded() -> None:
    """Package metadata + the language registry survived packaging."""
    assert bca.__version__, repr(bca.__version__)
    assert "python" in bca.supported_languages()
    # Extensions are bare names (no leading dot) — match the
    # `language_extensions` contract in the stub. The CLI used to expose
    # dotted forms; the binding does not.
    assert "py" in bca.language_extensions("python"), bca.language_extensions("python")


def _check_language_resolution() -> None:
    """`language_for_file` reads the file (parity with `analyze`, #318)."""
    # A stub path that does not exist raises FileNotFoundError, so
    # materialise an empty fixture in a tempdir to exercise the extension
    # table path rather than crashing on I/O.
    with tempfile.TemporaryDirectory() as td:
        fixture = Path(td) / "foo.py"
        fixture.write_bytes(b"")
        resolved = bca.language_for_file(fixture)
        assert resolved == "python", resolved


def _check_analyze_source() -> FuncSpaceDict:
    """`analyze_source` returns a unit dict with non-zero metrics."""
    result = bca.analyze_source(_ADD_SRC, "python")
    assert isinstance(result, dict), type(result)
    assert result.get("kind") == "unit", result.get("kind")
    # The unit-level cyclomatic is 3 for this fixture. A binding regression
    # that produced null / zero metrics would slip past `isinstance(result,
    # dict)` but trip this assertion. The binding returns Python floats (the
    # integer-as-JSON serialization of #530 applies to the CLI's text
    # output, asserted in cli_wheel_smoke.sh), so 3.0 is correct.
    cyclomatic_sum = result["metrics"]["cyclomatic"]["sum"]
    assert cyclomatic_sum == 3.0, f"expected unit cyclomatic.sum == 3.0, got {cyclomatic_sum}"
    return result


def _check_flatten_spaces(result: FuncSpaceDict) -> None:
    """`flatten_spaces` yields one record per space (unit + `add`)."""
    records = list(bca.flatten_spaces(result))
    assert records, "flatten_spaces returned no records"
    add_records = [r for r in records if r.get("name") == "add"]
    assert add_records, f"no 'add' function in records: {[r.get('name') for r in records]}"
    assert add_records[0].get("kind") == "function", add_records[0].get("kind")
    # The `add` function record's cyclomatic.sum is 2 (1 entry + 1 if).
    assert add_records[0].get("cyclomatic.sum") == 2.0, add_records[0].get("cyclomatic.sum")


def _check_sarif(result: FuncSpaceDict) -> None:
    """`to_sarif` emits a well-formed 2.1.0 document with one tool run."""
    # A stub `{"version": "2.1.0"}` (the most plausible regression of an
    # empty-run SARIF writer) would fail the runs check.
    sarif_obj = json.loads(bca.to_sarif(result))
    assert sarif_obj.get("version") == "2.1.0", sarif_obj.get("version")
    runs = sarif_obj.get("runs") or []
    assert len(runs) == 1, f"expected 1 SARIF run, got {len(runs)}"
    driver = runs[0].get("tool", {}).get("driver", {})
    assert driver.get("name"), f"SARIF run missing tool.driver.name: {driver}"


def _check_batch_never_raises() -> None:
    """`analyze_batch` returns an AnalysisFailure for a bad path, never raises."""
    # A binding regression that switched batch back to raising would crash
    # here; one that dropped error records entirely would shrink `len(batch)`
    # below the input count and trip the length assertion.
    with tempfile.TemporaryDirectory() as td:
        good = Path(td) / "good.py"
        good.write_text("def f():\n    pass\n")
        missing = Path(td) / "does-not-exist.py"
        batch = bca.analyze_batch([good, missing])
        assert len(batch) == 2, f"expected 2 batch results, got {len(batch)}"
        # Good path → dict.
        assert isinstance(batch[0], dict), type(batch[0])
        # Missing path → AnalysisFailure with `error_kind == "IoError"` (the
        # type was renamed from AnalysisError at 2.0, #614).
        err = batch[1]
        assert isinstance(err, bca.AnalysisFailure), type(err)
        assert err.error_kind == "IoError", err.error_kind
        assert str(missing) in err.path, (err.path, str(missing))


def main() -> None:
    _check_package_loaded()
    _check_language_resolution()
    result = _check_analyze_source()
    _check_flatten_spaces(result)
    _check_sarif(result)
    _check_batch_never_raises()
    print("smoke OK:", bca.__version__)


if __name__ == "__main__":
    main()
