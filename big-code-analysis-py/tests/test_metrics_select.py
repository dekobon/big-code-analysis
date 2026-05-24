"""End-to-end tests for the ``metrics=`` kwarg and ``METRIC_NAMES``
constant added in #268 (phase 4/9 of the Python-bindings epic #103).

The contract under test:

* ``metrics=None`` (default) computes the full metric suite.
* ``metrics=[name, …]`` computes only those metrics; unrequested
  metrics are **absent** from the result dict (not present with
  ``None`` placeholders).
* Empty list and unknown names raise :class:`ValueError` *before*
  any file I/O — programmer errors take precedence over per-file
  failures.
* Derived metrics (``"mi"``, ``"wmc"``) implicitly pull their
  dependencies into the result.
* The selection is keyword-only, strict-lowercase, applied
  uniformly across :func:`analyze`, :func:`analyze_source`, and
  :func:`analyze_batch`.

Several tests below are flagged **load-bearing** per
``.claude/rules/testing.md``: reverting the production change they
guard makes them fail. Treat them as test-via-revert candidates
before deleting or weakening their assertions.
"""

from __future__ import annotations

import concurrent.futures
from pathlib import Path
from typing import Any

import pytest

import big_code_analysis as bca

FIXTURES = Path(__file__).parent / "fixtures"


def _metrics_keys(result: dict[str, Any]) -> set[str]:
    """Return the set of metric-family keys on the top-level FuncSpace."""
    return set(result["metrics"].keys())


# ─────────────────────────────────────────────────────────────────
# METRIC_NAMES module constant
# ─────────────────────────────────────────────────────────────────


def test_metric_names_constant_shape() -> None:
    """``METRIC_NAMES`` is the canonical, alphabetised, immutable list."""
    assert isinstance(bca.METRIC_NAMES, tuple)
    assert all(isinstance(n, str) for n in bca.METRIC_NAMES)
    assert all(n.islower() for n in bca.METRIC_NAMES)
    assert len(bca.METRIC_NAMES) == len(set(bca.METRIC_NAMES)), "no duplicates"
    assert list(bca.METRIC_NAMES) == sorted(bca.METRIC_NAMES), "alphabetised"
    # Pin the exact canonical set — the issue spec lists 12 names
    # (omitting ``tokens``); the bindings expose all 13 since
    # ``Metric::Tokens`` participates in the same MetricSet gating
    # as every other variant.
    assert set(bca.METRIC_NAMES) == {
        "abc",
        "cognitive",
        "cyclomatic",
        "halstead",
        "loc",
        "mi",
        "nargs",
        "nexits",
        "nom",
        "npa",
        "npm",
        "tokens",
        "wmc",
    }


def test_metric_names_round_trip_through_analyze() -> None:
    """Every name in ``METRIC_NAMES`` is accepted by ``metrics=``.

    Class-only metrics (``npa``, ``npm``, ``wmc``) are elided from
    a unit-level space when there is no class to attach them to —
    use the Java fixture (which carries a class) for those so the
    output key actually appears.
    """
    class_only = {"npa", "npm", "wmc"}
    for name in bca.METRIC_NAMES:
        fixture = FIXTURES / ("Hello.java" if name in class_only else "hello.py")
        result = bca.analyze(fixture, metrics=[name])
        assert result is not None, f"analyze returned None for metrics=[{name!r}]"
        keys = _metrics_keys(result)
        # ``mi`` and ``wmc`` are derived; their *direct* output key
        # may sit on a nested space (or, for ``wmc``, be ``"wmc"``
        # at the unit root after the Java class merge). Their
        # dependencies must be present either way.
        if name == "mi":
            # Derived: also asserts `mi` itself was selected (not
            # silently dropped to deps-only) — both `mi` and its
            # closure must appear.
            assert {"loc", "cyclomatic", "halstead", "mi"}.issubset(keys), (
                f"mi or its dependencies missing; got {keys}"
            )
        elif name == "wmc":
            assert {"cyclomatic", "nom", "wmc"}.issubset(keys), (
                f"wmc or its dependencies missing; got {keys}"
            )
        else:
            assert name in keys, f"{name!r} missing from {keys} on {fixture.name}"


# ─────────────────────────────────────────────────────────────────
# Default / None parity
# ─────────────────────────────────────────────────────────────────


def test_metrics_none_is_parity_default() -> None:
    """**Load-bearing.** ``metrics=None`` must match the no-kwarg call.

    Reverting the ``None`` branch of ``resolve_metric_set`` to
    anything other than ``MetricSet::all()`` breaks this — the
    bindings' headline backwards-compatibility guarantee.
    """
    fixture = FIXTURES / "hello.rs"
    default = bca.analyze(fixture)
    explicit = bca.analyze(fixture, metrics=None)
    assert default == explicit


# ─────────────────────────────────────────────────────────────────
# Single-metric selection
# ─────────────────────────────────────────────────────────────────


def test_single_metric_selection_only_emits_that_key() -> None:
    """**Load-bearing.** ``metrics=["cyclomatic"]`` emits exactly ``{"cyclomatic"}``.

    Pins the core contract: unrequested metrics are *absent*,
    not present with ``null`` placeholders. Reverting the
    ``MetricSet`` plumbing to pass ``MetricSet::all()`` from the
    kwarg path breaks this immediately.
    """
    result = bca.analyze(FIXTURES / "hello.py", metrics=["cyclomatic"])
    assert result is not None
    assert _metrics_keys(result) == {"cyclomatic"}


def test_unrequested_metrics_absent_not_null() -> None:
    """``halstead`` must be entirely missing, not present-as-None.

    A naive implementation might emit every metric key with
    ``null`` placeholders for masked-out ones; the contract is
    *absent*, so ``.get("halstead")`` returns ``None`` only
    because the key doesn't exist.
    """
    result = bca.analyze(FIXTURES / "hello.py", metrics=["cyclomatic"])
    assert result is not None
    assert "halstead" not in result["metrics"]
    assert "loc" not in result["metrics"]
    assert "nom" not in result["metrics"]


# ─────────────────────────────────────────────────────────────────
# Validation: empty / unknown / case
# ─────────────────────────────────────────────────────────────────


def test_empty_list_raises_value_error_on_analyze() -> None:
    with pytest.raises(ValueError, match=r"provide at least one metric"):
        bca.analyze(FIXTURES / "hello.py", metrics=[])


def test_empty_list_raises_value_error_on_analyze_source() -> None:
    with pytest.raises(ValueError, match=r"provide at least one metric"):
        bca.analyze_source("def f(): pass\n", "python", metrics=[])


def test_empty_list_raises_value_error_on_analyze_batch() -> None:
    with pytest.raises(ValueError, match=r"provide at least one metric"):
        bca.analyze_batch([FIXTURES / "hello.py"], metrics=[])


def test_unknown_metric_raises_with_valid_list_in_message() -> None:
    """**Load-bearing.** Unknown-name error message advertises the canonical list.

    The error message wording is part of the public contract —
    callers grep it for the valid set when troubleshooting.
    """
    with pytest.raises(ValueError) as exc_info:
        bca.analyze(FIXTURES / "hello.py", metrics=["nope"])
    msg = str(exc_info.value)
    assert "unknown metric: nope" in msg
    # The valid list must be listed in full so users don't need to
    # consult METRIC_NAMES separately when they hit this.
    for name in bca.METRIC_NAMES:
        assert name in msg, f"valid-list message must include {name!r}"


def test_case_sensitivity_strict() -> None:
    """Strict lowercase: uppercase / mixed-case rejected."""
    with pytest.raises(ValueError, match=r"unknown metric: Loc"):
        bca.analyze(FIXTURES / "hello.py", metrics=["Loc"])
    with pytest.raises(ValueError, match=r"unknown metric: CYCLOMATIC"):
        bca.analyze(FIXTURES / "hello.py", metrics=["CYCLOMATIC"])


def test_duplicates_are_silently_accepted() -> None:
    """``["loc", "loc", "loc"]`` is equivalent to ``["loc"]``."""
    single = bca.analyze(FIXTURES / "hello.py", metrics=["loc"])
    dup = bca.analyze(FIXTURES / "hello.py", metrics=["loc", "loc", "loc"])
    assert single == dup


# ─────────────────────────────────────────────────────────────────
# Transitive dependency resolution
# ─────────────────────────────────────────────────────────────────


def test_mi_transitively_pulls_dependencies() -> None:
    """**Load-bearing.** ``metrics=["mi"]`` auto-includes loc+cyclomatic+halstead.

    Reverting ``MetricSet::from_slice_with_deps`` to a single-pass
    walk that only adds direct deps would still pass this for
    today's flat dependency graph; reverting it to *no* dependency
    resolution would fail immediately.
    """
    result = bca.analyze(FIXTURES / "hello.py", metrics=["mi"])
    assert result is not None
    keys = _metrics_keys(result)
    assert {"loc", "cyclomatic", "halstead", "mi"}.issubset(keys)
    # Unrelated metrics stay out.
    assert "abc" not in keys
    assert "nom" not in keys
    assert "tokens" not in keys


def test_wmc_transitively_pulls_dependencies() -> None:
    """``metrics=["wmc"]`` auto-includes cyclomatic+nom on a class-bearing fixture.

    The Java fixture carries a class so ``wmc`` itself is emitted at
    the unit root (not just its deps); asserting all three pins both
    the dependency closure AND the selection-honour contract — a
    regression that silently dropped ``wmc`` from the JSON output
    while keeping deps populated would otherwise pass.
    """
    result = bca.analyze(FIXTURES / "Hello.java", metrics=["wmc"])
    assert result is not None
    keys = _metrics_keys(result)
    assert {"cyclomatic", "nom", "wmc"}.issubset(keys), (
        f"wmc or its dependencies missing; got {keys}"
    )


# ─────────────────────────────────────────────────────────────────
# Cross-entry-point / cross-language coverage
# ─────────────────────────────────────────────────────────────────


@pytest.mark.parametrize(
    ("fixture", "language"),
    [
        ("hello.py", "python"),
        ("hello.rs", "rust"),
        ("Hello.java", "java"),
        ("hello.cpp", "cpp"),
    ],
)
def test_parametrized_across_languages(fixture: str, language: str) -> None:
    """Selection works uniformly across Python, Rust, Java, C++.

    The selection mechanism is language-agnostic (gating lives in
    the shared serializer), so this is a smoke check across every
    parser surface — not a correctness claim about each metric's
    value.
    """
    del language  # only used to label parametrize ids; analyze_path resolves the language itself
    result = bca.analyze(FIXTURES / fixture, metrics=["loc"])
    assert result is not None
    assert _metrics_keys(result) == {"loc"}


def test_analyze_source_supports_metrics_kwarg() -> None:
    """``analyze_source`` honours ``metrics=`` identically to ``analyze``."""
    result = bca.analyze_source("def f(): return 1\n", "python", metrics=["cyclomatic"])
    assert _metrics_keys(result) == {"cyclomatic"}


# ─────────────────────────────────────────────────────────────────
# Batch behaviour
# ─────────────────────────────────────────────────────────────────


def test_batch_validates_metrics_before_iteration() -> None:
    """**Load-bearing.** Bad ``metrics=`` aborts before ``__iter__`` runs.

    A generator's stateful ``__iter__`` (or any side effect of
    materialising the input) must not be invoked when ``metrics=``
    is invalid — the issue spec pins this: "validation runs before
    any file is opened" (and *a fortiori* before iteration starts).
    """
    side_effect_log: list[str] = []

    def tracking_generator() -> Any:
        side_effect_log.append("iter-entered")
        yield FIXTURES / "hello.py"

    with pytest.raises(ValueError, match=r"provide at least one metric"):
        bca.analyze_batch(tracking_generator(), metrics=[])
    assert side_effect_log == [], "metrics=[] must abort before __iter__ runs"

    with pytest.raises(ValueError, match=r"unknown metric"):
        bca.analyze_batch(tracking_generator(), metrics=["bogus"])
    assert side_effect_log == [], "metrics=['bogus'] must abort before __iter__ runs"


def test_batch_threads_selection_to_every_file() -> None:
    """``metrics=`` applies uniformly to every file in the batch."""
    results = bca.analyze_batch(
        [FIXTURES / "hello.py", FIXTURES / "hello.rs"], metrics=["loc"]
    )
    assert len(results) == 2
    for r in results:
        assert isinstance(r, dict), f"expected dict, got {type(r).__name__}: {r}"
        assert _metrics_keys(r) == {"loc"}


def test_batch_none_metrics_is_full_suite() -> None:
    """``analyze_batch(..., metrics=None)`` matches no-kwarg behaviour."""
    a = bca.analyze_batch([FIXTURES / "hello.py"])
    b = bca.analyze_batch([FIXTURES / "hello.py"], metrics=None)
    assert a == b


# ─────────────────────────────────────────────────────────────────
# Compatibility with flatten_spaces
# ─────────────────────────────────────────────────────────────────


def test_flatten_compatible_with_metric_selection() -> None:
    """``flatten_spaces`` honours absent metric keys.

    With ``metrics=["loc"]``, the only metric-family prefix in the
    flat records is ``loc``; every other family in
    :data:`bca.METRIC_NAMES` is absent. Checking the whole set (not
    just ``cyclomatic.*``) means a gating regression that broke any
    single family — halstead, nom, abc, etc — still fails this
    test.
    """
    result = bca.analyze(FIXTURES / "hello.py", metrics=["loc"])
    assert result is not None
    records = list(bca.flatten_spaces(result))
    assert records, "expected at least one flat record"
    # The non-metric flat keys (identity columns: `path`, `name`,
    # `kind`, `start_line`, `end_line`, `parent_name`, `depth`) carry
    # no dot, so split on `.` and inspect prefixes only — anything
    # before a dot is a metric family.
    forbidden = set(bca.METRIC_NAMES) - {"loc"}
    for r in records:
        for k in r:
            family = k.split(".", 1)[0]
            assert family not in forbidden, (
                f"unexpected {family!r} flat column when metrics=['loc']: {k!r}"
            )
    # At least one record should have a ``loc.*`` column (the unit
    # root carries metric values).
    assert any(any(k.startswith("loc.") for k in r) for r in records), (
        "no loc.* columns in any flat record"
    )


# ─────────────────────────────────────────────────────────────────
# exit / nexits alias
# ─────────────────────────────────────────────────────────────────


def test_exit_alias_accepts_both_spellings() -> None:
    """``"exit"`` and ``"nexits"`` both map to ``Metric::Exit``.

    The result's metric key is ``"nexits"`` (the JSON output key)
    regardless of which spelling the caller used.
    """
    via_display = bca.analyze(FIXTURES / "hello.py", metrics=["exit"])
    via_json_key = bca.analyze(FIXTURES / "hello.py", metrics=["nexits"])
    assert via_display == via_json_key
    assert via_display is not None
    assert "nexits" in via_display["metrics"]


# ─────────────────────────────────────────────────────────────────
# Argument shape / keyword-only / non-string elements
# ─────────────────────────────────────────────────────────────────


def test_metrics_kwarg_is_keyword_only() -> None:
    """``analyze(path, metrics)`` (positional) must raise ``TypeError``."""
    with pytest.raises(TypeError):
        # PyO3 enforces the ``/, *`` boundary in the signature.
        bca.analyze(FIXTURES / "hello.py", ["loc"])  # type: ignore[misc]


def test_metrics_tuple_is_accepted() -> None:
    """Any ``Sequence[str]`` works — list and tuple.

    PyO3's argument extractor requires a ``Sequence``; a bare
    generator is rejected with ``TypeError`` (matching the
    ``_native.pyi`` annotation). Materialise via ``list(gen)`` if
    you need to drive ``metrics=`` from a generator.
    """
    result = bca.analyze(FIXTURES / "hello.py", metrics=("loc",))
    assert result is not None
    assert _metrics_keys(result) == {"loc"}


def test_metrics_generator_is_rejected() -> None:
    """A bare generator passed to ``metrics=`` raises ``TypeError``.

    Pins the runtime contract advertised in ``_native.pyi``
    (``Sequence[str]``, not ``Iterable[str]``): downstream type
    checkers and the runtime agree that generators are not accepted
    without materialisation. A regression that silently widened
    extraction to any iterable would break this test, signalling a
    contract change that should be reflected in the stubs.
    """
    with pytest.raises(TypeError, match=r"Sequence"):
        bca.analyze(FIXTURES / "hello.py", metrics=(n for n in ["loc"]))


# ─────────────────────────────────────────────────────────────────
# Concurrent / per-call isolation
# ─────────────────────────────────────────────────────────────────


def test_metrics_kwarg_thread_safety_smoke() -> None:
    """**Load-bearing.** Per-call ``metrics=`` does not leak across threads.

    The issue spec pins "no global mutable state": passing
    ``metrics=["loc"]`` on one thread must not affect a concurrent
    ``metrics=["cyclomatic"]`` call on another. A regression that
    stashed the selection in a static would surface as crossed
    results here.
    """
    fixture = FIXTURES / "hello.py"
    selections = [["loc"], ["cyclomatic"], ["loc"], ["cyclomatic"], ["abc"]]

    def run(sel: list[str]) -> tuple[list[str], set[str]]:
        r = bca.analyze(fixture, metrics=sel)
        assert r is not None
        return sel, _metrics_keys(r)

    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        for sel, keys in pool.map(run, selections):
            assert keys == set(sel), f"thread cross-talk: {sel} -> {keys}"


# ─────────────────────────────────────────────────────────────────
# Interaction with other kwargs
# ─────────────────────────────────────────────────────────────────


def test_metrics_interacts_correctly_with_exclude_tests() -> None:
    """``metrics=["cyclomatic"], exclude_tests=True`` produces the same
    cyclomatic value as the full suite with ``exclude_tests=True``.

    Pins independence: ``exclude_tests`` operates on the AST walk
    (subtree pruning) and ``metrics=`` operates on the per-field
    serialisation gate; they should compose cleanly.
    """
    fixture = FIXTURES / "rust_with_tests.rs"
    full = bca.analyze(fixture, exclude_tests=True)
    selected = bca.analyze(fixture, exclude_tests=True, metrics=["cyclomatic"])
    assert full is not None
    assert selected is not None
    assert selected["metrics"]["cyclomatic"] == full["metrics"]["cyclomatic"]


def test_metrics_does_not_override_skip_generated() -> None:
    """``skip_generated=True`` (default) still wins when ``metrics=`` is set.

    A ``@generated`` file with ``skip_generated`` left at its
    default must still return ``None`` regardless of ``metrics=``
    — the filter runs before the metric-selection plumbing.
    """
    result = bca.analyze(FIXTURES / "generated.rs", metrics=["loc"])
    assert result is None
