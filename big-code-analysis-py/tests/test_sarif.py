"""Tests for :func:`bca.to_sarif` (phase 5/9 of the Python-bindings
epic #103, issue #269).

The contract under test:

* ``to_sarif(result, *, thresholds=None)`` returns SARIF 2.1.0 JSON
  as a ``str``.
* ``thresholds=None`` is equivalent to ``{}`` — the CLI ships no
  built-in defaults; both surfaces produce a well-formed empty run.
* Function-level findings round-trip against ``bca check -O sarif``
  for the same metric/threshold pair: same ``ruleId``, ``level``,
  ``message.text``, ``physicalLocation`` and ``logicalLocation``.
* Iterable input (the natural shape of :func:`analyze_batch`'s
  return value) is consumed lazily; :class:`AnalysisFailure` entries
  are filtered, not raised.
* Unknown threshold names raise :class:`ValueError` listing the
  accepted set, so a typo fails fast.
"""

from __future__ import annotations

import json
import subprocess
from pathlib import Path
from typing import Any, cast

import big_code_analysis as bca
import pytest
from big_code_analysis import FuncSpaceDict

FIXTURES = Path(__file__).parent / "fixtures"

SARIF_SCHEMA_URL = "https://json.schemastore.org/sarif-2.1.0.json"
SARIF_VERSION = "2.1.0"
TOOL_NAME = "big-code-analysis"

# The `bca_binary` fixture used below is defined once in
# `tests/conftest.py` and shared across every parity-aware test
# file. Removing the local copy here (and from `test_smoke.py`)
# means cargo build runs once per pytest session, not three times —
# the actual deduplication the conftest hoist was meant to deliver.


def _cli_check_sarif(bca_path: str, path: Path, *, threshold: str) -> dict[str, Any]:
    """Run ``bca check --threshold X -O sarif --paths <path>``.

    The CLI writes a one-line offender summary to stderr and the
    SARIF document to stdout; we want the JSON, so parse stdout.
    Exit code 2 is the normal "found violations" path (lib.rs line
    1057, reserving 1 for tool errors so CI can distinguish "metric
    regression" from "tool crashed") — `check=False` keeps
    subprocess from raising on it.
    """
    argv = [
        bca_path,
        "check",
        "--threshold",
        threshold,
        "-O",
        "sarif",
        "--paths",
        str(path),
    ]
    result = subprocess.run(argv, capture_output=True, text=True, check=False)
    if result.returncode not in (0, 2):
        pytest.fail(f"bca check exited {result.returncode}; stderr:\n{result.stderr}")
    parsed: dict[str, Any] = json.loads(result.stdout)
    return parsed


# ─────────────────────────────────────────────────────────────────
# Shape / schema
# ─────────────────────────────────────────────────────────────────


def _expected_sarif_uri(path: Path) -> str:
    """Mirror ``path_to_uri_reference`` in ``src/output/sarif.rs``.

    SARIF ``artifactLocation.uri`` is an RFC 3986 URI reference, so
    the writer percent-encodes characters outside the URI unreserved
    set, normalises backslashes to ``/``, and wraps absolute Windows
    drive paths in ``file:///``. The bindings hand their offender
    paths to the same writer, so the test-side expectation needs the
    matching transformation.
    """
    raw = str(path)
    # Detect a Windows-style drive prefix (`C:\...` or `C:/...`).
    drive_abs = (
        len(raw) >= 2
        and raw[0].isascii()
        and raw[0].isalpha()
        and raw[1] == ":"
        and (len(raw) == 2 or raw[2] in ("\\", "/"))
    )
    out: list[str] = []
    if drive_abs:
        out.append("file:///")
    unreserved = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~/:@"
    for ch in raw:
        if ch == "\\":
            out.append("/")
        elif ch in unreserved:
            out.append(ch)
        else:
            out.extend(f"%{byte:02X}" for byte in ch.encode("utf-8"))
    return "".join(out)


def _parse(doc: str) -> dict[str, Any]:
    """Parse a SARIF string and pin the top-level invariants."""
    assert isinstance(doc, str), f"to_sarif must return str, got {type(doc).__name__}"
    parsed: dict[str, Any] = json.loads(doc)
    assert parsed["$schema"] == SARIF_SCHEMA_URL
    assert parsed["version"] == SARIF_VERSION
    assert parsed["runs"][0]["tool"]["driver"]["name"] == TOOL_NAME
    return parsed


def test_to_sarif_returns_str_not_bytes() -> None:
    """The contract says ``str``; a regression to ``bytes`` would
    silently break ``json.loads(...)`` callers on the wrong type."""
    result = bca.analyze_source("def f(): pass\n", "python")
    out = bca.to_sarif(result)
    assert isinstance(out, str)
    # Bytes-encoded SARIF would still decode but the type contract
    # matters — pin it.
    assert not isinstance(out, (bytes, bytearray))


def test_to_sarif_empty_thresholds_emits_well_formed_empty_run() -> None:
    """``thresholds=None`` (and ``thresholds={}``) must produce a SARIF
    document with empty ``results`` and ``rules`` arrays — the
    documented "no-findings" baseline."""
    result = bca.analyze_source("def f(): pass\n", "python")
    thresholds_cases: list[dict[str, float] | None] = [None, {}]
    for thresholds in thresholds_cases:
        kwargs: dict[str, Any] = {}
        if thresholds is not None:
            kwargs["thresholds"] = thresholds
        parsed = _parse(bca.to_sarif(result, **kwargs))
        assert parsed["runs"][0]["results"] == []
        assert parsed["runs"][0]["tool"]["driver"]["rules"] == []


def test_to_sarif_empty_iterable_produces_empty_run() -> None:
    """``to_sarif([])`` must produce a valid empty SARIF document."""
    parsed = _parse(bca.to_sarif([]))
    assert parsed["runs"][0]["results"] == []
    assert parsed["runs"][0]["tool"]["driver"]["rules"] == []


def test_to_sarif_consumes_generator_exactly_once() -> None:
    """Generator inputs must be consumed exactly once (not twice via a
    materialise-then-iterate path, and not zero times by accident).

    Note: this does NOT verify true streaming/lazy consumption — that
    would require ordering checks that an eager-materialise refactor
    could still satisfy. The contract pinned here is the weaker but
    still load-bearing one: the generator's elements are visited the
    expected number of times.
    """
    consumed: list[FuncSpaceDict] = []

    def gen() -> Any:
        for code in ("def a(): pass\n", "def b(): pass\n"):
            r = bca.analyze_source(code, "python")
            consumed.append(r)
            yield r

    parsed = _parse(bca.to_sarif(gen()))
    assert len(consumed) == 2, "generator must be fully consumed exactly once"
    # No thresholds → no results, but the generator was still walked.
    assert parsed["runs"][0]["results"] == []


# ─────────────────────────────────────────────────────────────────
# Threshold finding emission
# ─────────────────────────────────────────────────────────────────


def test_to_sarif_single_function_exceeding_threshold_produces_one_result() -> None:
    """A file with one above-threshold function produces exactly one
    SARIF ``result`` whose ``ruleId``, ``level``, ``startLine``,
    ``endLine`` and ``logicalLocations`` line up with the source."""
    code = (
        "def branchy(x):\n"
        "    if x > 0:\n"
        "        return 1\n"
        "    if x < 0:\n"
        "        return -1\n"
        "    return 0\n"
    )
    result = bca.analyze_source(code, "python")
    parsed = _parse(bca.to_sarif(result, thresholds={"cyclomatic": 2}))

    results = parsed["runs"][0]["results"]
    assert len(results) == 1, f"expected one finding, got {results!r}"
    finding = results[0]
    assert finding["ruleId"] == "cyclomatic"
    assert finding["level"] == "warning"
    # Exact-string check (not substring): the canonical message format
    # is `"<metric> <value> exceeds limit <limit>"`. Substring checks
    # for `"cyclomatic"` or `"exceeds limit 2"` would also pass with
    # the value silently mutated (e.g. `"cyclomatic 999 exceeds limit 2"`).
    assert finding["message"]["text"] == "cyclomatic 3 exceeds limit 2"

    region = finding["locations"][0]["physicalLocation"]["region"]
    assert region["startLine"] == 1
    assert region["endLine"] == 6
    assert finding["locations"][0]["logicalLocations"][0]["fullyQualifiedName"] == "branchy"

    rules = parsed["runs"][0]["tool"]["driver"]["rules"]
    assert [r["id"] for r in rules] == ["cyclomatic"]


def test_to_sarif_function_below_threshold_emits_no_finding() -> None:
    """A function whose cyclomatic stays at-or-below the limit must
    not appear — pinned with a strict-equality boundary check that
    catches a regression from ``>`` to ``>=``."""
    code = "def trivial():\n    return 1\n"  # cyclomatic = 1
    result = bca.analyze_source(code, "python")
    # Equal to limit must NOT fire (CLI semantics: value > limit).
    parsed = _parse(bca.to_sarif(result, thresholds={"cyclomatic": 1}))
    assert parsed["runs"][0]["results"] == []


def test_to_sarif_mi_is_lower_is_worse() -> None:
    """``mi.*`` is lower-is-worse: a value strictly BELOW the limit is the
    violation, unlike every other metric. A healthy (high) MI must fire
    when the limit is set above it, and must NOT fire when the limit is
    at-or-below it — mirroring the CLI's #698 direction gate (the pre-fix
    binding used ``value > limit`` for every metric, flagging healthy MI
    and ignoring unhealthy MI)."""
    code = "def trivial():\n    return 1\n"
    result = bca.analyze_source(code, "python")
    mi_original = result["metrics"]["mi"]["original"]
    assert mi_original is not None, "mi.original must be present for trivial code"
    mi = float(mi_original)

    # Limit above the value -> value falls below -> flagged.
    above = _parse(bca.to_sarif(result, thresholds={"mi.original": mi + 50.0}))
    assert above["runs"][0]["results"], "MI below the limit must fire"

    # Limit at/below the value -> healthy -> no finding.
    below = _parse(bca.to_sarif(result, thresholds={"mi.original": max(mi - 50.0, 0.0)}))
    assert below["runs"][0]["results"] == [], "MI at/above the limit must not fire"


def test_to_sarif_metric_absent_from_dict_emits_no_finding() -> None:
    """When ``metrics=`` was used to skip a family, ``to_sarif`` must
    not synthesise a finding — the dict simply has no value to
    compare.

    Caveat on coverage: this assertion catches the regression where
    ``extract_metric`` would return some *positive* default (e.g.
    ``Some(1.0)``) for a missing key. It does NOT catch a regression
    to ``Some(0.0)`` (e.g. via ``unwrap_or_default``), because the
    threshold contract rejects negative limits and ``0.0 > 0.0`` is
    false — every legal threshold value would still suppress the
    finding in that scenario. Capturing the ``Some(0.0)`` case would
    require either a negative-threshold path (rejected upstream) or
    distinguishing ``None`` from ``Some(0.0)`` at an observable
    boundary, which the contract does not expose.
    """
    code = "def f(x):\n    return x + 1\n"
    # Compute only LoC — the result dict has no `cyclomatic` family.
    result = bca.analyze_source(code, "python", metrics=["loc"])
    parsed = _parse(bca.to_sarif(result, thresholds={"cyclomatic": 0}))
    assert parsed["runs"][0]["results"] == [], (
        "missing cyclomatic in dict must not synthesise a finding"
    )


# ─────────────────────────────────────────────────────────────────
# Iterable input + AnalysisFailure filtering
# ─────────────────────────────────────────────────────────────────


def test_to_sarif_filters_analysis_errors_silently(tmp_path: Path) -> None:
    """``AnalysisFailure`` entries in an iterable must be skipped, not
    raised — they represent files we couldn't analyse.

    Uses ``cyclomatic=0`` so the ok.py findings are positively
    asserted alongside the silent-skip behaviour. A regression that
    dropped successful dicts together with the errors would emit
    zero findings and slip past a bare "no errors raised" check.
    """
    ok = tmp_path / "ok.py"
    ok.write_text("def f(x):\n    return x + 1\n")
    missing = tmp_path / "nope.py"
    bogus = tmp_path / "noise.unknownext"
    bogus.write_text("noise")

    results = bca.analyze_batch([ok, missing, bogus])
    assert any(isinstance(r, bca.AnalysisFailure) for r in results), (
        "fixture expected to produce at least one AnalysisFailure"
    )
    parsed = _parse(bca.to_sarif(results, thresholds={"cyclomatic": 0}))
    # cyclomatic is Function-scoped (#969), so only ok.py's function `f`
    # (cyclomatic 1 > 0) emits — not the `<file>` unit. Pins that
    # AnalysisFailure entries are dropped while the successful dict is
    # still walked and its function gated.
    findings = parsed["runs"][0]["results"]
    fq_names = sorted(
        f["locations"][0]["logicalLocations"][0]["fullyQualifiedName"] for f in findings
    )
    assert fq_names == ["f"], (
        f"expected findings from ok.py only (errors skipped, dict kept), got {findings!r}"
    )
    for finding in findings:
        assert finding["ruleId"] == "cyclomatic"
        assert finding["locations"][0]["physicalLocation"]["artifactLocation"][
            "uri"
        ] == _expected_sarif_uri(ok)


def test_to_sarif_does_not_raise_on_pure_analysis_error_input(
    tmp_path: Path,
) -> None:
    """An iterable containing **only** AnalysisFailure instances still
    yields a well-formed empty SARIF run."""
    missing = tmp_path / "nope.py"
    results = bca.analyze_batch([missing])
    assert isinstance(results[0], bca.AnalysisFailure)
    parsed = _parse(bca.to_sarif(results, thresholds={"cyclomatic": 1}))
    assert parsed["runs"][0]["results"] == []


def test_to_sarif_filters_none_entries_silently(tmp_path: Path) -> None:
    """``None`` entries in an iterable must be skipped, not raised
    (issue #341).

    ``analyze()`` documents that it returns ``None`` for files
    classified as generated, so the natural pattern
    ``bca.to_sarif([bca.analyze(p) for p in paths])`` MUST tolerate
    ``None`` siblings alongside successful dicts. The skip mirrors
    the ``AnalysisFailure`` contract — both represent "no record
    emitted for this file".

    Uses ``cyclomatic=0`` so the ok.py findings are positively
    asserted alongside the silent-skip behaviour, mirroring
    ``test_to_sarif_filters_analysis_errors_silently``: a regression
    that dropped successful dicts together with ``None`` would emit
    zero findings and slip past a bare "no errors raised" check.
    """
    ok = tmp_path / "ok.py"
    ok.write_text("def f(x):\n    return x + 1\n")
    good = bca.analyze(ok)
    assert isinstance(good, dict), "fixture must produce an analysed dict"

    parsed = _parse(bca.to_sarif([good, None], thresholds={"cyclomatic": 0}))
    findings = parsed["runs"][0]["results"]
    fq_names = sorted(
        f["locations"][0]["logicalLocations"][0]["fullyQualifiedName"] for f in findings
    )
    # cyclomatic is Function-scoped (#969), so only ok.py's function `f`
    # (cyclomatic 1 > 0) is gated — the `<file>` unit is not.
    assert fq_names == ["f"], (
        f"expected findings from ok.py only (None skipped, dict kept), got {findings!r}"
    )
    for finding in findings:
        assert finding["ruleId"] == "cyclomatic"
        assert finding["locations"][0]["physicalLocation"]["artifactLocation"][
            "uri"
        ] == _expected_sarif_uri(ok)


def test_to_sarif_does_not_raise_on_pure_none_input() -> None:
    """An iterable containing **only** ``None`` still yields a
    well-formed empty SARIF run (issue #341 — generated-file lists
    that happen to be entirely generated)."""
    parsed = _parse(bca.to_sarif([None, None], thresholds={"cyclomatic": 1}))
    assert parsed["runs"][0]["results"] == []


def test_to_sarif_accepts_scalar_none() -> None:
    """A scalar ``None`` (the documented return of :func:`analyze`
    for a single generated file) must produce a well-formed empty
    SARIF run, mirroring the iterable arm's silent-skip contract.

    Without this, the natural per-file pattern
    ``bca.to_sarif(bca.analyze(generated_file))`` raised
    ``TypeError: 'NoneType' is not iterable`` — asymmetric with the
    list-comprehension form ``bca.to_sarif([bca.analyze(p) for p in
    paths])`` that the #341 fix already accepted. Closes the
    follow-up gap surfaced by code review on branch
    fix/batch-2026-05-25.
    """
    parsed = _parse(bca.to_sarif(None, thresholds={"cyclomatic": 1}))
    assert parsed["runs"][0]["results"] == []
    # The empty run is still SARIF-shaped — tool driver and schema
    # URL must be present so SARIF consumers don't trip on missing
    # metadata.
    assert parsed["runs"][0]["tool"]["driver"]["name"]
    assert parsed["$schema"]


# ─────────────────────────────────────────────────────────────────
# Input validation
# ─────────────────────────────────────────────────────────────────


def test_to_sarif_rejects_unknown_threshold_metric() -> None:
    """A typo must surface as ``ValueError`` listing the accepted set,
    not silently as zero findings."""
    result = bca.analyze_source("def f(): pass\n", "python")
    with pytest.raises(ValueError, match=r"unknown threshold metric"):
        bca.to_sarif(result, thresholds={"made.up.metric": 1.0})


def test_to_sarif_rejects_negative_threshold() -> None:
    """The CLI's ``parse_cli_threshold`` rejects negative limits; the
    bindings adopt the same contract."""
    result = bca.analyze_source("def f(): pass\n", "python")
    with pytest.raises(ValueError, match=r"finite non-negative number"):
        bca.to_sarif(result, thresholds={"cyclomatic": -1.0})


def test_to_sarif_rejects_nan_threshold() -> None:
    """NaN limits are not finite — same rejection path as negative."""
    result = bca.analyze_source("def f(): pass\n", "python")
    with pytest.raises(ValueError, match=r"finite non-negative number"):
        bca.to_sarif(result, thresholds={"cyclomatic": float("nan")})


def test_to_sarif_rejects_non_dict_non_iterable_input() -> None:
    """An int / float / random object is neither a result dict nor an
    iterable of them — surface the bad type rather than dropping it.

    The expected error path is the `try_iter()?` call after the
    single-dict branch falls through, which raises PyO3's standard
    "'int' object is not iterable" message. The regex pin guards
    against a regression where some unrelated internal TypeError
    (e.g., a kwarg dispatch bug) would pass a bare ``raises(TypeError)``
    silently.
    """
    with pytest.raises(TypeError, match=r"not iterable"):
        bca.to_sarif(12345)  # type: ignore[arg-type]


def test_to_sarif_rejects_str_input() -> None:
    """``str`` is iterable in Python — yielding characters that would
    fail the dict downcast with a confusing error. Reject explicitly
    so the caller sees a clear message."""
    with pytest.raises(TypeError, match=r"not str"):
        bca.to_sarif("definitely not a result dict")  # type: ignore[arg-type]


# ─────────────────────────────────────────────────────────────────
# CLI parity
# ─────────────────────────────────────────────────────────────────


def test_to_sarif_matches_cli_check_for_single_function(bca_binary: str, tmp_path: Path) -> None:
    """``to_sarif(bca.analyze(p), thresholds={...})`` matches
    ``bca check --threshold X -O sarif --paths p`` modulo documented
    differences (tool.driver.version is identical because both come
    from ``CARGO_PKG_VERSION``).

    Uses a Python fixture with a single function: the function's own
    cyclomatic breaches while the file unit's own cyclomatic (base 1)
    does not, so both front-ends emit exactly one finding.
    """
    src = tmp_path / "branchy.py"
    src.write_text(
        "def branchy(x):\n"
        "    if x > 0:\n"
        "        return 1\n"
        "    if x < 0:\n"
        "        return -1\n"
        "    return 0\n"
    )

    analyzed = bca.analyze(src)
    assert analyzed is not None, "fixture must not be skipped"
    py_doc = json.loads(bca.to_sarif(analyzed, thresholds={"cyclomatic": 2}))
    cli_doc = _cli_check_sarif(bca_binary, src, threshold="cyclomatic=2")

    # Tool descriptor parity: same name, version, rule set.
    assert (
        py_doc["runs"][0]["tool"]["driver"]["name"] == cli_doc["runs"][0]["tool"]["driver"]["name"]
    )
    assert (
        py_doc["runs"][0]["tool"]["driver"]["version"]
        == cli_doc["runs"][0]["tool"]["driver"]["version"]
    )
    assert (
        py_doc["runs"][0]["tool"]["driver"]["rules"]
        == cli_doc["runs"][0]["tool"]["driver"]["rules"]
    )

    # Findings parity: same count, same ruleId / level / message /
    # logical location. ``artifactLocation.uri`` may differ in path
    # normalisation; both sides see the same tmpfile here so we can
    # compare directly.
    assert len(py_doc["runs"][0]["results"]) == len(cli_doc["runs"][0]["results"])
    for py_r, cli_r in zip(
        py_doc["runs"][0]["results"],
        cli_doc["runs"][0]["results"],
        strict=True,
    ):
        assert py_r["ruleId"] == cli_r["ruleId"]
        assert py_r["level"] == cli_r["level"]
        assert py_r["message"]["text"] == cli_r["message"]["text"]
        py_loc = py_r["locations"][0]
        cli_loc = cli_r["locations"][0]
        assert py_loc["logicalLocations"] == cli_loc["logicalLocations"]
        assert py_loc["physicalLocation"]["region"] == cli_loc["physicalLocation"]["region"]
        assert (
            py_loc["physicalLocation"]["artifactLocation"]["uri"]
            == cli_loc["physicalLocation"]["artifactLocation"]["uri"]
        )


def _sarif_sort_key(r: dict[str, Any]) -> tuple[int, str]:
    """Sort SARIF results by (startLine, fullyQualifiedName) so a walk-order
    difference across the two sides doesn't masquerade as a real divergence —
    semantic equality is what's under test."""
    loc = r["locations"][0]
    line = int(loc["physicalLocation"]["region"]["startLine"])
    fq = loc.get("logicalLocations", [{}])[0].get("fullyQualifiedName", "")
    return (line, fq)


def _assert_sarif_results_match(
    py_results: list[dict[str, Any]], cli_results: list[dict[str, Any]]
) -> None:
    """Assert two SARIF result lists are field-for-field equal after sorting."""
    py_results.sort(key=_sarif_sort_key)
    cli_results.sort(key=_sarif_sort_key)
    for py_r, cli_r in zip(py_results, cli_results, strict=True):
        assert py_r["ruleId"] == cli_r["ruleId"]
        assert py_r["level"] == cli_r["level"]
        assert py_r["message"]["text"] == cli_r["message"]["text"]
        assert py_r["locations"][0]["logicalLocations"] == cli_r["locations"][0]["logicalLocations"]
        assert (
            py_r["locations"][0]["physicalLocation"]["region"]
            == cli_r["locations"][0]["physicalLocation"]["region"]
        )
        assert (
            py_r["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
            == cli_r["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
        )


def test_to_sarif_matches_cli_check_for_wmc_container_scope(
    bca_binary: str, tmp_path: Path
) -> None:
    """CLI parity for a Container-scoped metric (#969).

    `wmc` (weighted methods per class) gates container spaces, not the
    file unit: its file-level total is an aggregate across every class,
    not a per-class limit. A multi-class Python file therefore produces
    exactly one finding per class and **no** `<file>` finding. Both sides
    must agree — this catches a regression where one front-end forgets to
    apply the per-metric scope gate and emits the file aggregate.
    """
    src = tmp_path / "classes.py"
    src.write_text(
        "class A:\n"
        "    def m1(self): pass\n"
        "    def m2(self): pass\n"
        "\n"
        "class B:\n"
        "    def n1(self): pass\n"
    )

    analyzed = bca.analyze(src)
    assert analyzed is not None, "fixture must not be skipped"
    py_doc = json.loads(bca.to_sarif(analyzed, thresholds={"wmc": 0}))
    cli_doc = _cli_check_sarif(bca_binary, src, threshold="wmc=0")

    py_results = py_doc["runs"][0]["results"]
    cli_results = cli_doc["runs"][0]["results"]

    assert len(py_results) == len(cli_results) == 2, (
        f"expected 2 findings (the 2 classes, no file unit), "
        f"got py={len(py_results)} cli={len(cli_results)}"
    )
    for results in (py_results, cli_results):
        names = {r["locations"][0]["logicalLocations"][0]["fullyQualifiedName"] for r in results}
        assert names == {"A", "B"}, f"wmc must gate the classes, not the file unit: {names}"
        assert all(r["ruleId"] == "wmc" for r in results)
    _assert_sarif_results_match(py_results, cli_results)


def test_to_sarif_matches_cli_check_for_loc_unit_emission(bca_binary: str, tmp_path: Path) -> None:
    """CLI parity for a File-scoped metric (#969).

    `loc.sloc` gates the whole-file `unit` root only, never a class or
    function. A multi-class file therefore produces exactly one finding,
    the `<file>` unit. Complements the Container-scope case above and
    guards the unit-level `<file>` placeholder and walking the file unit
    at all.
    """
    src = tmp_path / "classes.py"
    src.write_text(
        "class A:\n"
        "    def m1(self): pass\n"
        "    def m2(self): pass\n"
        "\n"
        "class B:\n"
        "    def n1(self): pass\n"
    )

    analyzed = bca.analyze(src)
    assert analyzed is not None, "fixture must not be skipped"
    py_doc = json.loads(bca.to_sarif(analyzed, thresholds={"loc.sloc": 0}))
    cli_doc = _cli_check_sarif(bca_binary, src, threshold="loc.sloc=0")

    py_results = py_doc["runs"][0]["results"]
    cli_results = cli_doc["runs"][0]["results"]

    assert len(py_results) == len(cli_results) == 1, (
        f"expected 1 finding (the file unit only), got py={len(py_results)} cli={len(cli_results)}"
    )
    for results in (py_results, cli_results):
        assert results[0]["locations"][0]["logicalLocations"][0]["fullyQualifiedName"] == "<file>"
        assert results[0]["ruleId"] == "loc.sloc"
    _assert_sarif_results_match(py_results, cli_results)


def test_to_sarif_qualified_symbol_matches_cli_for_nested_method(
    bca_binary: str, tmp_path: Path
) -> None:
    """A method nested inside a named container emits the CLI's
    *qualified* ``Container::method`` symbol in ``logicalLocations``
    (issue #706), not the bare method name. Before the fix the binding
    emitted ``branchy`` where the CLI emits ``A::branchy`` — a silent
    divergence the single-/top-level-function parity tests above could
    not catch (a top-level function's qualified name equals its bare
    name).

    The binding keys every metric off its per-space own value, so the
    enclosing class ``A`` (own cyclomatic = base 1, below the limit) is
    not reported while the method that genuinely breaches is — the full
    finding *set* matches the CLI exactly. This test asserts set equality,
    not just membership of the method symbol.
    """
    src = tmp_path / "nested.py"
    src.write_text(
        "class A:\n"
        "    def branchy(self, x):\n"
        "        if x > 0:\n"
        "            return 1\n"
        "        if x < 0:\n"
        "            return -1\n"
        "        return 0\n"
    )

    analyzed = bca.analyze(src)
    assert analyzed is not None, "fixture must not be skipped"
    py_doc = _parse(bca.to_sarif(analyzed, thresholds={"cyclomatic": 2}))
    cli_doc = _cli_check_sarif(bca_binary, src, threshold="cyclomatic=2")

    def _qualified_names(doc: dict[str, Any]) -> set[str]:
        return {
            r["locations"][0]["logicalLocations"][0]["fullyQualifiedName"]
            for r in doc["runs"][0]["results"]
        }

    py_names = _qualified_names(py_doc)
    cli_names = _qualified_names(cli_doc)
    # The method-level offender must carry the container-joined symbol
    # on both sides — the core #706 divergence.
    assert "A::branchy" in cli_names, f"CLI reference must qualify the method; got {cli_names!r}"
    # The binding's finding set matches the CLI exactly: `cyclomatic` is
    # compared per-space via its own value, so the enclosing class `A`
    # (own value below the limit) is not emitted — only the method is.
    assert py_names == cli_names, (
        f"binding finding set must match the CLI; got py={py_names!r} cli={cli_names!r}"
    )
    # No finding may carry the bare, un-qualified method name.
    assert "branchy" not in py_names, (
        f"binding must not emit the un-qualified method name; got {py_names!r}"
    )


def test_to_sarif_anonymous_space_collapses_to_anon_line() -> None:
    """A space whose name is the literal ``<anonymous>`` (every grammar's
    closure/lambda sentinel) collapses to ``<anon@L{start_line}>``,
    matching the CLI's ``space_segment`` (issue #706). The prior binding
    passed ``<anonymous>`` through verbatim, which the CLI never emits.
    """
    fake = _fake_function_dict(name="<anonymous>", start_line=42, cyclomatic_value=5.0)
    parsed = _parse(bca.to_sarif(fake, thresholds={"cyclomatic": 1}))
    findings = parsed["runs"][0]["results"]
    assert len(findings) == 1
    assert findings[0]["locations"][0]["logicalLocations"] == [{"fullyQualifiedName": "<anon@L42>"}]


# ─────────────────────────────────────────────────────────────────
# Robustness fixes from /code-review (recall-mode review)
# ─────────────────────────────────────────────────────────────────


def _fake_function_dict(
    *,
    name: str | None = "f",
    kind: str = "function",
    start_line: int = 1,
    end_line: int = 5,
    cyclomatic_value: Any = 5.0,
) -> FuncSpaceDict:
    """Hand-construct a FuncSpace-shaped dict for adversarial input
    tests that cannot be reached through ``analyze_source``.

    The deliberately-malformed metric values (e.g. ``bool`` / ``float``
    where the wire shape is ``int``) are the point of these tests, so the
    builder is ``cast`` to :class:`FuncSpaceDict` — the static shape
    :func:`to_sarif` now expects (#623) — rather than re-deriving a
    looser parameter type that would weaken every other call site.

    ``cyclomatic_value`` drives the per-space ``cyclomatic.value`` the
    binding thresholds against since #958. The dict is a leaf
    (``spaces: []``), so ``sum`` mirrors ``value`` to stay shape-realistic.
    """
    fake: dict[str, Any] = {
        "name": name,
        "kind": kind,
        "start_line": start_line,
        "end_line": end_line,
        "spaces": [],
        "metrics": {
            "cyclomatic": {
                "sum": cyclomatic_value,
                "value": cyclomatic_value,
                "average": 1.0,
                "min": 1.0,
                "max": 1.0,
                "modified": {
                    "sum": 1.0,
                    "value": 1.0,
                    "average": 1.0,
                    "min": 1.0,
                    "max": 1.0,
                },
            },
        },
    }
    return cast("FuncSpaceDict", fake)


@pytest.mark.parametrize(
    ("metric_name", "json_path"),
    [
        ("cyclomatic", ("cyclomatic", "value")),
        ("cyclomatic.modified", ("cyclomatic", "modified", "value")),
        ("cognitive", ("cognitive", "value")),
        ("loc.lloc", ("loc", "lloc")),
    ],
)
def test_to_sarif_rejects_bool_metric_value(metric_name: str, json_path: tuple[str, ...]) -> None:
    """Python ``True`` extracts as ``1.0`` via PyO3's ``f64`` extractor
    because ``bool`` inherits from ``int``. Without an explicit guard,
    a user-crafted dict with any metric value set to ``True`` would
    silently emit a finding at value 1.0. The path under test is the one
    the binding actually thresholds against, so the rejection — not an
    incidentally-absent field — is what suppresses the finding. Cover
    every shape the extractor walks: top-level (``loc.lloc``), one-level
    nested own-value (``cyclomatic``, ``cognitive``), two-level nested
    (``cyclomatic.modified``).
    """
    fake = _fake_function_dict()
    # Drop the canonical metrics block from the helper and replace
    # only the path under test with a bool, so the rejection isn't
    # masked by some other metric incidentally firing.
    metrics: dict[str, Any] = {}
    cursor: dict[str, Any] = metrics
    for key in json_path[:-1]:
        cursor[key] = {}
        cursor = cursor[key]
    cursor[json_path[-1]] = True
    # `fake` is typed FuncSpaceDict (#623); the adversarial metrics block
    # is deliberately the wrong shape, so write it through a plain-dict
    # view rather than against the CodeMetricsDict key type.
    cast("dict[str, Any]", fake)["metrics"] = metrics

    parsed = _parse(bca.to_sarif(fake, thresholds={metric_name: 0}))
    assert parsed["runs"][0]["results"] == [], (
        f"bool metric value at {json_path!r} must be rejected by extract_metric"
    )


def test_to_sarif_rejects_bool_line_number() -> None:
    """Mirror of the bool rejection in ``extract_line_number``. Without
    the guard, ``start_line: True`` would extract as 1 (Python bool
    inherits from int) and emit a finding at line 1. The
    SARIF-writer's ``max(1)`` clamp would mask the issue."""
    fake = _fake_function_dict(start_line=10, end_line=15, cyclomatic_value=5.0)
    fake["start_line"] = True
    fake["end_line"] = True
    parsed = _parse(bca.to_sarif(fake, thresholds={"cyclomatic": 1}))
    region = parsed["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"]
    # bool rejected → falls back to 0 → writer clamps to 1.
    assert region["startLine"] == 1
    assert region["endLine"] == 1


def test_to_sarif_treats_unit_kind_case_insensitively() -> None:
    """Upstream serialises ``SpaceKind`` via ``rename_all = \"lowercase\"``,
    but defending against a future upstream rename (or a hand-crafted
    dict using ``Unit``) is cheap: the kind comparison normalises to
    ASCII-lowercase. The capitalised ``Unit`` must still resolve to the
    file-level ``<file>`` symbol on the offender it emits. A File-scoped
    ``loc.sloc`` breaches at the unit, so it emits; the point under test
    is the kind-name normalisation driving both the scope match and the
    ``<file>`` symbol.
    """
    # loc.sloc is File-scoped (#969), so it gates the unit root; the
    # capitalised "Unit" must normalise to unit for both the scope match
    # and the <file> symbol.
    fake: dict[str, Any] = {
        "name": "mod.py",
        "kind": "Unit",  # capitalised on purpose
        "start_line": 1,
        "end_line": 5,
        "spaces": [],
        "metrics": {"loc": {"sloc": 999}},
    }
    parsed = _parse(bca.to_sarif(cast("FuncSpaceDict", fake), thresholds={"loc.sloc": 1}))
    findings = parsed["runs"][0]["results"]
    fq_names = [f["locations"][0]["logicalLocations"][0]["fullyQualifiedName"] for f in findings]
    assert fq_names == ["<file>"], (
        f"Unit (capital) must normalise to unit and carry the <file> symbol, got {fq_names!r}"
    )


def test_to_sarif_rejects_mappingproxytype_with_clear_error() -> None:
    """``types.MappingProxyType`` is a Mapping but not a dict, so the
    inner ``cast_into::<PyDict>()`` fails. Without the explicit
    Mapping rejection the value would fall through to the iterable
    path, iterate the dict's keys as strings, and surface a confusing
    'got str' error. Pin the dedicated error message.
    """
    import types as _types

    result = bca.analyze_source("def f(): pass\n", "python")
    proxy = _types.MappingProxyType(result)
    with pytest.raises(TypeError, match=r"plain dict"):
        bca.to_sarif(proxy, thresholds={"cyclomatic": 0})  # type: ignore[arg-type]


def test_to_sarif_emits_container_level_finding_for_oo_metrics() -> None:
    """Container-scoped metrics (`nom`, `wmc`, `npm`, `npa`) emit one
    finding per container (#969), never at the file unit — their file-level
    total is an aggregate across every class, not a per-class limit. Pinned
    with ``wmc`` because the multi-class fixture cleanly distinguishes the
    per-class values from the file total.
    """
    code = (
        "class A:\n"
        "    def m1(self): pass\n"
        "    def m2(self): pass\n"
        "\n"
        "class B:\n"
        "    def n1(self): pass\n"
    )
    result = bca.analyze_source(code, "python")
    parsed = _parse(bca.to_sarif(result, thresholds={"wmc": 0}))
    findings = parsed["runs"][0]["results"]
    # Expect exactly one finding per class (A, B) and no file-level finding.
    fully_qualified = sorted(
        f["locations"][0]["logicalLocations"][0]["fullyQualifiedName"] for f in findings
    )
    assert fully_qualified == ["A", "B"], (
        f"wmc must gate the classes, not the file unit: {fully_qualified!r}"
    )


def _own_value_block(metric_name: str, own: float) -> dict[str, Any]:
    """Build a minimal ``metrics`` sub-dict carrying ``own`` at the path
    the binding thresholds ``metric_name`` against since #958 (the
    per-space ``value`` field, or ``modified.value`` for the modified
    variant). Only the walked path needs to be present — ``extract_metric``
    ignores the sibling aggregate/min/max keys — so the fixtures stay
    readable.
    """
    if metric_name == "cyclomatic.modified":
        return {"cyclomatic": {"modified": {"value": own}}}
    if metric_name == "cyclomatic":
        return {"cyclomatic": {"value": own}}
    if metric_name in ("cognitive", "abc"):
        return {metric_name: {"value": own}}
    raise AssertionError(f"unhandled metric {metric_name!r}")


@pytest.mark.parametrize(
    "metric_name",
    ["cyclomatic", "cyclomatic.modified", "cognitive", "abc"],
)
def test_to_sarif_emits_interior_space_when_own_value_breaches(metric_name: str) -> None:
    """#958: an interior space (here a function owning a nested closure)
    whose *own* value breaches the limit is now reported — exactly as the
    CLI's per-space accessor does. For these four metrics the JSON exposes
    a subtree aggregate (``sum``/``magnitude``) *and*, since #958, the
    per-space ``value``; the binding reads ``value``, so it no longer has
    to skip interior spaces. Before #958 it could read only the aggregate,
    so it skipped every interior space and silently under-emitted this
    breach (the residual gap #855's leaf-only fix left open).
    """
    # outer.value (5) breaches the limit (3); the closure it owns and the
    # file unit stay below it, so only `outer` may be reported.
    unit: dict[str, Any] = {
        "name": "mod.py",
        "kind": "unit",
        "start_line": 1,
        "end_line": 12,
        "metrics": _own_value_block(metric_name, 1.0),
        "spaces": [
            {
                "name": "outer",
                "kind": "function",
                "start_line": 2,
                "end_line": 11,
                "metrics": _own_value_block(metric_name, 5.0),
                "spaces": [
                    {
                        "name": "<anonymous>",
                        "kind": "function",
                        "start_line": 4,
                        "end_line": 6,
                        "metrics": _own_value_block(metric_name, 1.0),
                        "spaces": [],
                    }
                ],
            }
        ],
    }
    parsed = _parse(bca.to_sarif(cast("FuncSpaceDict", unit), thresholds={metric_name: 3}))
    findings = parsed["runs"][0]["results"]
    fq_names = [f["locations"][0]["logicalLocations"][0]["fullyQualifiedName"] for f in findings]
    assert fq_names == ["outer"], (
        f"only the interior function whose own {metric_name} breaches may emit, got {fq_names!r}"
    )


def test_to_sarif_nameless_space_emits_anon_line_placeholder() -> None:
    """A non-unit space with ``name: None`` (rare parse-failure case)
    collapses to ``<anon@L{start_line}>``, matching the CLI's
    ``space_segment`` (issue #706). The CLI bakes the start line into
    the segment so the anonymous space keeps a stable-within-a-snapshot
    identity; the prior binding emitted a bare ``<unnamed>`` that the
    CLI never produces.
    """
    fake = _fake_function_dict(name=None, start_line=1, cyclomatic_value=5.0)
    # The outer dict's name doubles as `path`; set it explicitly so
    # the test isolates the `function` field behaviour.
    fake["name"] = None
    parsed = _parse(bca.to_sarif(fake, thresholds={"cyclomatic": 1}))
    findings = parsed["runs"][0]["results"]
    assert len(findings) == 1
    assert findings[0]["locations"][0]["logicalLocations"] == [{"fullyQualifiedName": "<anon@L1>"}]


def test_to_sarif_unit_space_emits_file_placeholder() -> None:
    """A unit-level finding (for a File-scoped metric, #969) emits
    ``logicalLocations: [{fullyQualifiedName: '<file>'}]`` rather than
    duplicating the path that already appears in
    ``artifactLocation.uri``. Matches the CLI's ``qualified_symbol``.
    """
    code = "class A:\n    def m(self): pass\n"
    result = bca.analyze_source(code, "python")
    parsed = _parse(bca.to_sarif(result, thresholds={"loc.sloc": 0}))
    findings = parsed["runs"][0]["results"]
    fq_names = [f["locations"][0]["logicalLocations"][0]["fullyQualifiedName"] for f in findings]
    assert fq_names == ["<file>"], (
        f"unit-level finding must carry '<file>' placeholder, got {fq_names!r}"
    )


def test_to_sarif_rejects_empty_metric_name() -> None:
    """Mirrors the CLI's ``parse_cli_threshold`` rejection of an
    empty metric name (``"empty metric name"`` is the canonical
    message). The previous binding fell through to the generic
    'unknown metric ""' path."""
    result = bca.analyze_source("def f(): pass\n", "python")
    with pytest.raises(ValueError, match=r"empty metric name"):
        bca.to_sarif(result, thresholds={"": 1.0})


def test_to_sarif_clamps_oversized_line_numbers_to_u32_max() -> None:
    """Adversarial / future-proofed input with line numbers exceeding
    ``u32::MAX`` must clamp to ``u32::MAX`` (matching the CLI's
    ``u32::try_from(usize).unwrap_or(u32::MAX)`` fallback in
    ``violation_to_offender``). Previously the binding silently
    collapsed any non-u32 value to ``0``.
    """
    fake = _fake_function_dict(
        start_line=2**32 + 5,
        end_line=2**32 + 10,
        cyclomatic_value=5.0,
    )
    parsed = _parse(bca.to_sarif(fake, thresholds={"cyclomatic": 1}))
    region = parsed["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"]
    # u32::MAX = 4_294_967_295
    assert region["startLine"] == 4_294_967_295
    assert region["endLine"] == 4_294_967_295


def test_to_sarif_negative_line_numbers_fall_back_to_zero() -> None:
    """Negative line numbers cannot be represented in SARIF's u32
    region; they fall back to 0, which the upstream writer clamps to
    1 (its documented invariant). Pin both ends of that contract.
    """
    fake = _fake_function_dict(start_line=-5, end_line=-3, cyclomatic_value=5.0)
    parsed = _parse(bca.to_sarif(fake, thresholds={"cyclomatic": 1}))
    region = parsed["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"]
    # Binding: start_line = 0 (i64 was negative, fell back).
    # Writer:  startLine = max(0, 1) = 1.
    assert region["startLine"] == 1
    assert region["endLine"] == 1


def test_to_sarif_unit_not_emitted_when_only_aggregate_exceeds() -> None:
    """A unit whose subtree aggregate (``cognitive.sum``) exceeds the limit
    but whose *own* value (``cognitive.value``) does not must NOT produce a
    unit finding — the binding reads the own value, so it is no longer
    fooled by the rolled-up aggregate (#958). The nested function whose own
    value genuinely exceeds the limit is still reported.
    """
    unit: dict[str, Any] = {
        "name": "mod.rs",
        "kind": "unit",
        "start_line": 1,
        "end_line": 20,
        # Aggregate (99) >> limit, but the unit's own cognitive (2) is below
        # it: the binding must key off `value`, not `sum`.
        "metrics": {"cognitive": {"sum": 99.0, "value": 2.0}},
        "spaces": [
            {
                "name": "f",
                "kind": "function",
                "start_line": 3,
                "end_line": 18,
                "metrics": {"cognitive": {"sum": 42.0, "value": 42.0}},
                "spaces": [],
            }
        ],
    }
    parsed = _parse(bca.to_sarif(cast("FuncSpaceDict", unit), thresholds={"cognitive": 10}))
    findings = parsed["runs"][0]["results"]
    fq_names = [f["locations"][0]["logicalLocations"][0]["fullyQualifiedName"] for f in findings]
    assert fq_names == ["f"], (
        f"unit (own value below limit) must not emit; only the nested fn, got {fq_names!r}"
    )


def test_to_sarif_reports_deeply_nested_space_offender() -> None:
    """A method nested two levels deep (unit -> class -> method) that
    exceeds a threshold must be discovered by the stack walk and carry
    its *qualified* ``Container::method`` name (issue #706) and line
    span. Guards the nested-space traversal feeding ``collect_offenders``'
    qualified-prefix threading.

    cyclomatic is Function-scoped (#969), so only the leaf method ``m``
    (a function space) is gated — the enclosing class ``C`` and the file
    unit are not, regardless of their own ``cyclomatic.value``. The
    qualified ``C::m`` name still exercises the nested-prefix threading.
    """
    unit: dict[str, Any] = {
        "name": "mod.rs",
        "kind": "unit",
        "start_line": 1,
        "end_line": 30,
        # Unit's own cyclomatic stays below the limit -> no unit finding.
        "metrics": {"cyclomatic": {"sum": 13.0, "value": 1.0}},
        "spaces": [
            {
                "name": "C",
                "kind": "class",
                "start_line": 2,
                "end_line": 29,
                # The class body's own cyclomatic (5) would breach the
                # limit, but cyclomatic is Function-scoped (#969), so a
                # class space is never gated — only the nested method is.
                "metrics": {"cyclomatic": {"sum": 12.0, "value": 5.0}},
                "spaces": [
                    {
                        "name": "m",
                        "kind": "function",
                        "start_line": 5,
                        "end_line": 12,
                        "metrics": {"cyclomatic": {"sum": 7.0, "value": 7.0}},
                        "spaces": [],
                    }
                ],
            }
        ],
    }
    parsed = _parse(bca.to_sarif(cast("FuncSpaceDict", unit), thresholds={"cyclomatic": 3}))
    findings = parsed["runs"][0]["results"]
    fq_names = sorted(
        f["locations"][0]["logicalLocations"][0]["fullyQualifiedName"] for f in findings
    )
    # Only the leaf method `C::m` (a function space) is gated; the
    # enclosing class `C` and the file unit are not, as cyclomatic is
    # Function-scoped (#969).
    assert fq_names == ["C::m"], (
        f"only the leaf method must emit for a Function-scoped metric, got {fq_names!r}"
    )
    method_finding = next(
        f
        for f in findings
        if f["locations"][0]["logicalLocations"][0]["fullyQualifiedName"] == "C::m"
    )
    assert method_finding["locations"][0]["physicalLocation"]["region"]["startLine"] == 5
