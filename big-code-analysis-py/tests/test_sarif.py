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
  return value) is consumed lazily; :class:`AnalysisError` entries
  are filtered, not raised.
* Unknown threshold names raise :class:`ValueError` listing the
  accepted set, so a typo fails fast.
"""

from __future__ import annotations

import json
import subprocess
from pathlib import Path
from typing import Any

import big_code_analysis as bca
import pytest

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
    unreserved = (
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~/:@"
    )
    for ch in raw:
        if ch == "\\":
            out.append("/")
        elif ch in unreserved:
            out.append(ch)
        else:
            for byte in ch.encode("utf-8"):
                out.append(f"%{byte:02X}")
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
    consumed: list[dict[str, Any]] = []

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
# Iterable input + AnalysisError filtering
# ─────────────────────────────────────────────────────────────────


def test_to_sarif_filters_analysis_errors_silently(tmp_path: Path) -> None:
    """``AnalysisError`` entries in an iterable must be skipped, not
    raised — they represent files we couldn't analyse.

    Uses ``cyclomatic=0`` so the ok.py finding is positively
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
    assert any(isinstance(r, bca.AnalysisError) for r in results), (
        "fixture expected to produce at least one AnalysisError"
    )
    parsed = _parse(bca.to_sarif(results, thresholds={"cyclomatic": 0}))
    # ok.py's `f` has cyclomatic = 1 > 0 → exactly one finding.
    # Pins that AnalysisError entries are dropped while the
    # successful dict is still walked.
    findings = parsed["runs"][0]["results"]
    assert len(findings) == 1, (
        f"expected one finding from ok.py (errors skipped, dict kept), got {findings!r}"
    )
    assert findings[0]["ruleId"] == "cyclomatic"
    assert findings[0]["locations"][0]["physicalLocation"]["artifactLocation"][
        "uri"
    ] == _expected_sarif_uri(ok)


def test_to_sarif_does_not_raise_on_pure_analysis_error_input(
    tmp_path: Path,
) -> None:
    """An iterable containing **only** AnalysisError instances still
    yields a well-formed empty SARIF run."""
    missing = tmp_path / "nope.py"
    results = bca.analyze_batch([missing])
    assert isinstance(results[0], bca.AnalysisError)
    parsed = _parse(bca.to_sarif(results, thresholds={"cyclomatic": 1}))
    assert parsed["runs"][0]["results"] == []


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

    Uses a Python fixture with a single function so all per-space
    findings are at leaf-function level — no unit-vs-function
    aggregate divergence for sum-shaped metrics.
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


def test_to_sarif_matches_cli_check_for_wmc_with_unit_emission(
    bca_binary: str, tmp_path: Path
) -> None:
    """CLI parity for a metric that emits at the unit level.

    Complements ``test_to_sarif_matches_cli_check_for_single_function``
    (which uses ``cyclomatic`` — a skip-at-unit metric) by exercising
    a metric (`wmc`) where the binding now emits unit findings as
    well. A multi-class Python file produces one finding per class
    plus one file-level finding; both sides must agree on count,
    rule, level, message, physical region, logical location, and
    artifact URI. Catches any regression in the unit-level
    `<file>` placeholder or in the `skip_at_unit=false` flag for
    a metric whose CLI accessor matches the JSON headline.
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

    # Sort by (startLine, fullyQualifiedName) so any difference in
    # walk order across sides doesn't masquerade as a real divergence
    # — semantic equality is what's under test.
    def _sort_key(r: dict[str, Any]) -> tuple[int, str]:
        loc = r["locations"][0]
        line = int(loc["physicalLocation"]["region"]["startLine"])
        fq = loc.get("logicalLocations", [{}])[0].get("fullyQualifiedName", "")
        return (line, fq)

    py_results.sort(key=_sort_key)
    cli_results.sort(key=_sort_key)

    assert len(py_results) == len(cli_results) == 3, (
        f"expected 3 findings (file + 2 classes), got py={len(py_results)} cli={len(cli_results)}"
    )
    for py_r, cli_r in zip(py_results, cli_results, strict=True):
        assert py_r["ruleId"] == cli_r["ruleId"] == "wmc"
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


# ─────────────────────────────────────────────────────────────────
# Robustness fixes from /code-review (recall-mode review)
# ─────────────────────────────────────────────────────────────────


def _fake_function_dict(
    *,
    name: str | None = "f",
    kind: str = "function",
    start_line: int = 1,
    end_line: int = 5,
    cyclomatic_sum: Any = 5.0,
) -> dict[str, Any]:
    """Hand-construct a FuncSpace-shaped dict for adversarial input
    tests that cannot be reached through ``analyze_source``."""
    return {
        "name": name,
        "kind": kind,
        "start_line": start_line,
        "end_line": end_line,
        "spaces": [],
        "metrics": {
            "cyclomatic": {
                "sum": cyclomatic_sum,
                "average": 1.0,
                "min": 1.0,
                "max": 1.0,
                "modified": {"sum": 1.0, "average": 1.0, "min": 1.0, "max": 1.0},
            },
        },
    }


@pytest.mark.parametrize(
    ("metric_name", "json_path"),
    [
        ("cyclomatic", ("cyclomatic", "sum")),
        ("cyclomatic.modified", ("cyclomatic", "modified", "sum")),
        ("cognitive", ("cognitive", "sum")),
        ("loc.lloc", ("loc", "lloc")),
    ],
)
def test_to_sarif_rejects_bool_metric_value(metric_name: str, json_path: tuple[str, ...]) -> None:
    """Python ``True`` extracts as ``1.0`` via PyO3's ``f64`` extractor
    because ``bool`` inherits from ``int``. Without an explicit guard,
    a user-crafted dict with any metric headline set to ``True`` would
    silently emit a finding at value 1.0. Cover every shape of JSON
    path the extractor walks: top-level (``loc.lloc``), one-level
    nested-with-sum (``cyclomatic``, ``cognitive``), two-level nested
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
    fake["metrics"] = metrics

    parsed = _parse(bca.to_sarif(fake, thresholds={metric_name: 0}))
    assert parsed["runs"][0]["results"] == [], (
        f"bool metric value at {json_path!r} must be rejected by extract_metric"
    )


def test_to_sarif_rejects_bool_line_number() -> None:
    """Mirror of the bool rejection in ``extract_line_number``. Without
    the guard, ``start_line: True`` would extract as 1 (Python bool
    inherits from int) and emit a finding at line 1. The
    SARIF-writer's ``max(1)`` clamp would mask the issue."""
    fake = _fake_function_dict(start_line=10, end_line=15, cyclomatic_sum=5.0)
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
    ASCII-lowercase. A regression that re-introduced a case-sensitive
    check would emit findings for the skip-listed metrics at the
    unit level.
    """
    fake = _fake_function_dict(kind="Unit", cyclomatic_sum=999)
    parsed = _parse(bca.to_sarif(fake, thresholds={"cyclomatic": 1}))
    assert parsed["runs"][0]["results"] == [], (
        "Unit (capital) must normalise to unit and trigger skip_at_unit"
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


def test_to_sarif_emits_unit_level_finding_for_non_sum_metrics() -> None:
    """Unit-level findings are now emitted for metrics whose JSON
    headline matches the CLI's per-space accessor (loc, halstead,
    wmc, mi, nom, abc, tokens, nargs, nexits, npa, npm). Pinned with
    ``wmc`` because the multi-class fixture cleanly distinguishes
    unit-aggregate from per-class.
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
    # Expect: 1 finding per class (A, B) + 1 file-level (unit) finding.
    fully_qualified = [
        f["locations"][0]["logicalLocations"][0]["fullyQualifiedName"] for f in findings
    ]
    assert "<file>" in fully_qualified, (
        f"unit finding missing; logicalLocations seen: {fully_qualified!r}"
    )
    assert "A" in fully_qualified
    assert "B" in fully_qualified


@pytest.mark.parametrize(
    "metric_name",
    ["cyclomatic", "cyclomatic.modified", "cognitive"],
)
def test_to_sarif_still_skips_unit_for_cyclomatic_family(metric_name: str) -> None:
    """The three metrics whose CLI accessor returns the per-space
    scalar (cyclomatic / cyclomatic.modified / cognitive) MUST still
    be skipped at the unit level, because the JSON exposes only the
    aggregate ``sum``. A regression that emitted unit findings for
    any of these would diverge from the CLI in a way that's hard to
    spot. Parameterised so the contract is pinned for each member of
    the family, not just `cyclomatic`.
    """
    code = (
        "def branchy(x):\n"
        "    if x > 0:\n"
        "        return 1\n"
        "    if x < 0:\n"
        "        return -1\n"
        "    return 0\n"
    )
    result = bca.analyze_source(code, "python")
    # Use threshold 0 so any value > 0 fires (the unit's aggregate
    # value is positive for all three metrics on this fixture). The
    # unit must still be skipped; only `branchy` should emit.
    parsed = _parse(bca.to_sarif(result, thresholds={metric_name: 0}))
    findings = parsed["runs"][0]["results"]
    assert len(findings) == 1, (
        f"expected single per-function finding for {metric_name!r}, got {findings!r}"
    )
    fq = findings[0]["locations"][0]["logicalLocations"][0]["fullyQualifiedName"]
    assert fq == "branchy", f"expected per-function finding for {metric_name!r}, got fq={fq!r}"


def test_to_sarif_nameless_space_emits_unnamed_placeholder() -> None:
    """A non-unit space with ``name: None`` (rare parse-failure case)
    must emit ``logicalLocations: [{fullyQualifiedName: '<unnamed>'}]``,
    matching the CLI's ``function_token`` fallback. The previous
    behaviour dropped the field entirely, diverging from the CLI's
    SARIF shape.
    """
    fake = _fake_function_dict(name=None, cyclomatic_sum=5.0)
    # The outer dict's name doubles as `path`; set it explicitly so
    # the test isolates the `function` field behaviour.
    fake["name"] = None
    parsed = _parse(bca.to_sarif(fake, thresholds={"cyclomatic": 1}))
    findings = parsed["runs"][0]["results"]
    assert len(findings) == 1
    assert findings[0]["locations"][0]["logicalLocations"] == [{"fullyQualifiedName": "<unnamed>"}]


def test_to_sarif_unit_space_emits_file_placeholder() -> None:
    """A unit-level finding (for a non-skip metric) emits
    ``logicalLocations: [{fullyQualifiedName: '<file>'}]`` rather than
    duplicating the path that already appears in
    ``artifactLocation.uri``. Matches the CLI's ``function_token``.
    """
    code = "class A:\n    def m(self): pass\n"
    result = bca.analyze_source(code, "python")
    parsed = _parse(bca.to_sarif(result, thresholds={"wmc": 0}))
    findings = parsed["runs"][0]["results"]
    fq_names = [f["locations"][0]["logicalLocations"][0]["fullyQualifiedName"] for f in findings]
    assert "<file>" in fq_names, (
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
        cyclomatic_sum=5.0,
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
    fake = _fake_function_dict(start_line=-5, end_line=-3, cyclomatic_sum=5.0)
    parsed = _parse(bca.to_sarif(fake, thresholds={"cyclomatic": 1}))
    region = parsed["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"]
    # Binding: start_line = 0 (i64 was negative, fell back).
    # Writer:  startLine = max(0, 1) = 1.
    assert region["startLine"] == 1
    assert region["endLine"] == 1
