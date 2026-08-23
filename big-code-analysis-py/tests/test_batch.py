"""Tests for ``bca.analyze_batch`` and ``bca.AnalysisFailure``.

Covers the issue-#266 contract: never-raise per-file semantics,
1:1 ordering, the three documented ``error_kind`` values, generator
input, the ``AnalysisFailure`` constructor's hash/equality/repr
behaviour, and the programmer-error surfaces that still raise.

Run via::

    maturin develop
    python -m pytest big-code-analysis-py/tests/test_batch.py
"""

from __future__ import annotations

import json
import pickle
import sys
from collections.abc import Iterator
from pathlib import Path

import big_code_analysis as bca
import pytest

FIXTURES = Path(__file__).parent / "fixtures"


# ── analyze_batch: happy path ───────────────────────────────────


def test_all_valid_inputs_match_individual_analyze_outputs() -> None:
    """Batch results equal what per-file ``analyze`` would return.

    Pins the "same content as individual ``analyze()`` calls"
    requirement from the issue. The fixtures cover three distinct
    languages (Python / Rust / C++) so a language-routing
    regression in batch — e.g. always sending bytes to one parser —
    fails here.
    """
    paths = [FIXTURES / "hello.py", FIXTURES / "hello.rs", FIXTURES / "hello.cpp"]
    batch = bca.analyze_batch(paths)
    individual = [bca.analyze(p, skip_generated=False) for p in paths]
    assert batch == individual


def test_mixed_language_batch_preserves_per_file_routing() -> None:
    """Each input position carries its own language detection.

    Asserts the **inner** ``FuncSpace`` shape per result so a
    regression that collapsed the language map onto a single
    parser would fail here. The Java fixture's inner
    ``kind == "class"`` is the discriminating signal: parsing the
    same bytes with the Python or Rust grammar produces an error
    tree with no ``class``-kind inner space, so the assertion
    below would fall over. The Python/Rust fixtures each define a
    single ``hello`` function whose presence at the inner level
    is similarly load-bearing — a top-level ``name`` check would
    not catch the regression because ``name`` echoes the input
    path regardless of parser.
    """
    paths = [FIXTURES / "hello.py", FIXTURES / "Hello.java", FIXTURES / "hello.rs"]
    results = bca.analyze_batch(paths)
    assert all(isinstance(r, dict) for r in results)
    py_result, java_result, rs_result = results
    assert isinstance(py_result, dict)
    assert isinstance(java_result, dict)
    assert isinstance(rs_result, dict)

    # Top-level name still mirrors the input path; pin it as the
    # cheap-to-check half of the contract.
    assert py_result["name"] == str(paths[0])
    assert java_result["name"] == str(paths[1])
    assert rs_result["name"] == str(paths[2])

    # Inner-shape discriminator. The exact (name, kind) tuple per
    # fixture is enumerated below so a future fixture edit that
    # changes the symbol shape forces a test edit rather than
    # silently weakening the assertion.
    assert {(s["name"], s["kind"]) for s in py_result["spaces"]} >= {
        ("hello", "function"),
    }, "Python parser must produce a `hello` function in hello.py"
    assert {(s["name"], s["kind"]) for s in java_result["spaces"]} >= {
        ("Hello", "class"),
    }, "Java parser must produce a `Hello` class in Hello.java"
    assert {(s["name"], s["kind"]) for s in rs_result["spaces"]} >= {
        ("hello", "function"),
    }, "Rust parser must produce a `hello` function in hello.rs"


def test_ordering_preserved_with_interleaved_failures(tmp_path: Path) -> None:
    """Result list zips back against the input list 1:1.

    The success / failure / success pattern catches a regression
    that *appends* errors at the end instead of slotting them at
    the matching index. Pinning each success slot's ``name`` to its
    source path additionally catches a *transposition* of the two
    same-typed success results — a reorder the bare ``isinstance``
    checks could not see (#916). The two successes are deliberately
    different languages so a same-language collapse cannot satisfy
    the per-slot ``name`` check by accident.
    """
    paths = [
        FIXTURES / "hello.py",
        tmp_path / "does_not_exist.py",
        FIXTURES / "hello.rs",
    ]
    results = bca.analyze_batch(paths)
    assert len(results) == 3
    py_result, failure, rs_result = results
    assert isinstance(py_result, dict)
    assert isinstance(failure, bca.AnalysisFailure)
    assert isinstance(rs_result, dict)

    # Each success slot must carry its own source path; a transposition
    # of the two would put hello.rs's result at index 0 and fail here.
    assert py_result["name"] == str(paths[0])
    assert rs_result["name"] == str(paths[2])
    # The failure slot is pinned to the missing seed, mirroring
    # test_missing_file_yields_io_error.
    assert str(paths[1]) in failure.path


# ── analyze_batch: error taxonomy ───────────────────────────────


def test_missing_file_yields_io_error(tmp_path: Path) -> None:
    missing = tmp_path / "absent.py"
    results = bca.analyze_batch([missing])
    assert len(results) == 1
    err = results[0]
    assert isinstance(err, bca.AnalysisFailure)
    assert err.error_kind == "IoError"
    assert str(missing) in err.path


def test_unknown_extension_yields_unsupported_language(tmp_path: Path) -> None:
    """An on-disk file with an unrecognised extension.

    File must exist — the bridge reads before language inference
    (lesson from #314), so a *missing* file with an unknown
    extension surfaces as ``IoError``, not ``UnsupportedLanguage``.
    """
    bad = tmp_path / "noise.totallyunknown"
    bad.write_text("nothing parseable here\n")
    results = bca.analyze_batch([bad])
    err = results[0]
    assert isinstance(err, bca.AnalysisFailure)
    assert err.error_kind == "UnsupportedLanguage"


def test_directory_path_yields_io_error(tmp_path: Path) -> None:
    """A path that exists but is a directory.

    ``std::fs::read`` returns ``EISDIR`` here, which the bridge
    maps to ``AnalysisFailure::Io``; in batch that flows through to
    ``error_kind="IoError"``. Pins the I/O arm against the
    directory case without needing to fabricate a permission
    setup that varies by CI runner.
    """
    results = bca.analyze_batch([tmp_path])
    err = results[0]
    assert isinstance(err, bca.AnalysisFailure)
    assert err.error_kind == "IoError"


# ParseError is currently unreachable from the Python surface:
# tree-sitter is permissive and the upstream `MetricsError::Parse*`
# variants are reserved for a future strict-parse mode. The issue
# (#266) lists a ParseError test for completeness; we keep the
# coverage on the mapping itself in ``src/batch.rs`` (Rust unit
# tests) and mark the Python-side check as a *strict* xfail so
# that the day a future strict-parse landing makes ParseError
# reachable, this test XPASSes — and pytest fails the suite
# loudly, forcing a contributor to remove the xfail marker and
# inspect the new error surface. `strict=False` would silently
# accept XPASS, which is precisely the failure mode tripwire
# tests are meant to avoid.
@pytest.mark.xfail(
    reason="tree-sitter is permissive — MetricsError::Parse not yet reachable; "
    "kept as a strict tripwire for the strict-parse mode reserved upstream.",
    strict=True,
)
def test_syntactically_broken_source_yields_parse_error(tmp_path: Path) -> None:
    broken = tmp_path / "broken.rs"
    broken.write_bytes(b"\x00\x01\x02\x03 not even close to source")
    results = bca.analyze_batch([broken])
    err = results[0]
    assert isinstance(err, bca.AnalysisFailure)
    assert err.error_kind == "ParseError"


# ── analyze_batch: input shapes ─────────────────────────────────


def test_empty_iterable_returns_empty_list() -> None:
    empty: list[Path] = []
    assert bca.analyze_batch([]) == []
    assert bca.analyze_batch(iter([])) == []
    assert bca.analyze_batch(p for p in empty) == []


def test_generator_input_works() -> None:
    """Generators are accepted as the ``paths`` argument.

    Pins the duck-typed-iterable contract: the only thing this
    test verifies is that an arbitrary Python iterator (not just
    a list / tuple) reaches the analysis loop and produces
    matching-order results. Lazy consumption is an *implementation*
    property of the Rust loop (`paths.try_iter()` followed by
    per-item `next()`) that the per-file analysis observably
    requires but the assertions below do not probe in isolation
    — the dedicated bridge tests cover that.
    """

    def gen() -> Iterator[Path]:
        yield FIXTURES / "hello.py"
        yield FIXTURES / "hello.rs"

    results = bca.analyze_batch(gen())
    assert len(results) == 2
    py_result, rs_result = results
    assert isinstance(py_result, dict)
    assert isinstance(rs_result, dict)
    # Back the "matching-order results" docstring claim with a per-slot
    # name assertion so a generator-path reorder is caught (#916).
    assert py_result["name"] == str(FIXTURES / "hello.py")
    assert rs_result["name"] == str(FIXTURES / "hello.rs")


def test_pathlib_and_str_inputs_both_accepted() -> None:
    """Both ``os.PathLike[str]`` and plain ``str`` paths work."""
    paths: list[str | Path] = [
        str(FIXTURES / "hello.py"),
        FIXTURES / "hello.rs",
    ]
    results = bca.analyze_batch(paths)
    assert all(isinstance(r, dict) for r in results)


# ── analyze_batch: programmer-error raises ──────────────────────


def test_non_iterable_paths_raises_type_error() -> None:
    with pytest.raises(TypeError):
        bca.analyze_batch(42)  # type: ignore[arg-type]


def test_non_path_element_raises_type_error() -> None:
    """Iterable is OK, but a non-path element inside should raise."""
    with pytest.raises(TypeError):
        bca.analyze_batch([FIXTURES / "hello.py", 7])  # type: ignore[list-item]


def test_empty_metrics_list_raises_value_error() -> None:
    """``metrics=[]`` is a programmer error.

    ``metrics=None`` (the default) means "compute the full suite";
    an explicit empty list is meaningless — the analysis would
    silently compute nothing. The full ``metrics=`` selection
    contract lives in ``test_metrics_select.py``; this test
    survives here because the empty-list rejection is the original
    pre-#268 contract analyze_batch exposed and the regression cost
    if it silently lapsed is high.
    """
    # The test pins the *type* (ValueError), not the wording — the
    # message is verified by the dedicated test in test_metrics_select.
    with pytest.raises(ValueError):  # noqa: PT011
        bca.analyze_batch([FIXTURES / "hello.py"], metrics=[])


def test_none_metrics_is_accepted() -> None:
    results = bca.analyze_batch([FIXTURES / "hello.py"], metrics=None)
    assert isinstance(results[0], dict)


def test_paths_len_runtime_error_propagates_not_swallowed() -> None:
    """A custom container whose ``__len__`` raises a non-``TypeError``
    surfaces the error instead of silently falling back to ``cap=0``.

    The Rust bridge only swallows ``TypeError`` from ``len()`` (the
    expected "object has no len" signal from generators); any other
    exception — e.g. a lazy proxy raising ``RuntimeError`` to signal
    a transient backing-store failure — propagates so the caller
    sees the real problem instead of getting an empty / partial
    result list with no diagnostic.

    Pins the documented behaviour at _native.pyi: "any exception
    raised by the input iterator itself (e.g. … a custom container
    whose ``__len__`` raises a non-``TypeError``) also propagates
    out". Regressing the match arm to ``Err(_) => 0`` would
    silently re-introduce the swallow.
    """

    class HostileLen:
        def __iter__(self) -> Iterator[Path]:
            return iter([FIXTURES / "hello.py"])

        def __len__(self) -> int:
            raise RuntimeError("snapshot unavailable")

    with pytest.raises(RuntimeError, match="snapshot unavailable"):
        bca.analyze_batch(HostileLen())


# ── analyze_batch: never-raise on per-file failures ─────────────


def test_batch_does_not_raise_when_every_file_fails(tmp_path: Path) -> None:
    """Pure-failure batches still return — none of the entries
    raises into the caller.

    Regression guard for the never-raise contract: a refactor that
    forgot to convert one of the bridge-layer error arms into a
    ``PyAnalysisError`` would explode the call here.
    """
    missing = tmp_path / "absent.py"
    unknown = tmp_path / "noise.unknownext"
    unknown.write_text("nothing\n")
    results = bca.analyze_batch([missing, unknown, tmp_path])
    assert len(results) == 3
    assert all(isinstance(r, bca.AnalysisFailure) for r in results)
    kinds = [r.error_kind for r in results if isinstance(r, bca.AnalysisFailure)]
    assert kinds == ["IoError", "UnsupportedLanguage", "IoError"]


# ── AnalysisFailure: shape / equality / hashing ───────────────────


def test_analysis_error_attributes_are_readable() -> None:
    err = bca.AnalysisFailure("p.py", "boom", "IoError")
    assert err.path == "p.py"
    assert err.error == "boom"
    assert err.error_kind == "IoError"


def test_analysis_error_is_not_an_exception_subclass() -> None:
    """Sanity check: ``AnalysisFailure`` is *returned*, never raised.

    Catches a regression where someone makes it inherit
    ``BaseException`` (perhaps to enable ``raise``-style use); the
    issue contract explicitly forbids that so callers do not have
    to worry about implicit exception propagation through ``raise
    AnalysisFailure(...)`` typos.
    """
    err = bca.AnalysisFailure("p.py", "boom", "IoError")
    assert not isinstance(err, BaseException)


def test_analysis_failure_class_is_not_a_base_exception_subclass() -> None:
    """#614: the class itself (not just an instance) is not raisable.

    The rename from ``AnalysisError`` exists precisely because the
    ``…Error`` suffix made readers write ``except bca.AnalysisError:``,
    which raises ``TypeError`` (a non-``BaseException`` class cannot be
    caught). Pin the value-type contract under the new name at the
    *class* level — ``issubclass(AnalysisFailure, BaseException)`` is
    ``False`` — and confirm the misleading old name is gone.
    """
    assert not issubclass(bca.AnalysisFailure, BaseException)
    assert not hasattr(bca, "AnalysisError"), (
        "the misleading AnalysisError name must be removed at 2.0 (#614)"
    )


def test_analyze_batch_returns_analysis_failure_not_raises(tmp_path: Path) -> None:
    """#614: a per-file failure is *returned* as ``AnalysisFailure``,
    never raised — the value-type contract under the new name.
    """
    missing = tmp_path / "nope.rs"
    # No raise even though the file does not exist.
    results = bca.analyze_batch([missing])
    assert len(results) == 1
    assert isinstance(results[0], bca.AnalysisFailure)
    assert results[0].error_kind == "IoError"


def test_analysis_error_equality_by_value() -> None:
    a = bca.AnalysisFailure("p.py", "msg", "IoError")
    b = bca.AnalysisFailure("p.py", "msg", "IoError")
    c = bca.AnalysisFailure("p.py", "msg", "ParseError")
    d = bca.AnalysisFailure("q.py", "msg", "IoError")
    assert a == b
    assert a != c
    assert a != d


def test_analysis_error_hash_matches_equality() -> None:
    """Equal errors hash equal; the contract that lets callers
    deduplicate via ``set`` / ``dict`` keys.
    """
    a = bca.AnalysisFailure("p.py", "msg", "IoError")
    b = bca.AnalysisFailure("p.py", "msg", "IoError")
    assert hash(a) == hash(b)
    assert len({a, b}) == 1
    assert len({a, bca.AnalysisFailure("p.py", "other", "IoError")}) == 2


def test_analysis_error_repr_includes_all_fields() -> None:
    """``repr`` is debuggable — every field appears, labelled, and
    each label sits next to its own value.

    Adjacency matters: independent substring checks for the
    labels and the values would still pass if a regression
    swapped two field values (e.g. ``path="missing", error="p.py"``)
    because both labels and both values would still appear in
    the output. Pin the label-value pair so a swap is caught.

    The Rust impl routes each field through Python's ``repr()``
    builtin (so ``eval(repr(x))`` round-trips for arbitrary string
    content — see
    ``test_analysis_error_repr_round_trips_through_eval_for_non_ascii``).
    Python's ``repr`` surrounds simple strings with single quotes,
    so the expected fragment is ``path='p.py'``.
    """
    err = bca.AnalysisFailure("p.py", "missing", "IoError")
    r = repr(err)
    assert r.startswith("AnalysisFailure(")
    assert "path='p.py'" in r
    assert "error='missing'" in r
    assert "error_kind='IoError'" in r


def test_analysis_error_rejects_unknown_kind() -> None:
    """Constructor validates ``error_kind`` against the closed set.

    The taxonomy is part of the public contract; silent acceptance
    of typos like ``"IOError"`` (note the capitalised middle ``O``)
    would let callers ship code that compares against a value
    that batch will never actually emit.
    """
    # Both raises pin the *type* (ValueError) — the constructor's
    # rejection message wording is not part of this test's contract.
    # The type: ignore is load-bearing: the stub restricts ``kind`` to
    # ``Literal['UnsupportedLanguage', 'ParseError', 'IoError']`` and
    # the whole point of this test is to verify that the constructor
    # rejects values outside that set at runtime.
    with pytest.raises(ValueError):  # noqa: PT011
        bca.AnalysisFailure("p.py", "msg", "NotARealKind")  # type: ignore[arg-type]
    with pytest.raises(ValueError):  # noqa: PT011
        bca.AnalysisFailure("p.py", "msg", "IOError")  # type: ignore[arg-type]


def test_analysis_error_is_frozen() -> None:
    """``#[pyclass(frozen)]`` means assignment to any field raises.

    A regression that drops ``frozen`` — or that introduces a
    per-field ``#[pyo3(set)]`` on just one of the three — would let
    callers mutate an instance in place and break the
    ``set`` / ``dict``-key dedup contract (mutation corrupts hash
    consistency). Cover every field so a partial-frozen regression
    can't slip past a single-attribute check.
    """
    err = bca.AnalysisFailure("p.py", "msg", "IoError")
    for attr, value in [
        ("path", "other.py"),
        ("error", "new message"),
        ("error_kind", "ParseError"),
    ]:
        with pytest.raises(AttributeError):
            setattr(err, attr, value)


def test_analysis_error_accepts_keyword_arguments() -> None:
    """Constructor takes positional OR keyword args (no ``/`` in
    the PyO3 signature, matching the stub).

    Regression guard for the stub/runtime mismatch fixed in this
    PR — the original stub declared positional-only via ``/``
    while the Rust ``#[pyo3(signature = (...))]`` accepts kwargs,
    so a strict-mypy caller writing ``AnalysisFailure(path=...,
    ...)`` would have failed type-check despite the runtime
    accepting it.
    """
    err = bca.AnalysisFailure(path="p.py", error="boom", error_kind="IoError")
    assert err.path == "p.py"
    assert err.error == "boom"
    assert err.error_kind == "IoError"


# ── Edge cases from the issue ───────────────────────────────────


def test_symlinked_path_is_followed(tmp_path: Path) -> None:
    """A symlink to a valid source resolves transparently.

    Pins that batch does not require canonical paths — the OS
    resolves the symlink during ``std::fs::read``.
    """
    if sys.platform.startswith("win"):
        pytest.skip("symlink creation requires elevated rights on Windows")
    target = FIXTURES / "hello.py"
    link = tmp_path / "link.py"
    link.symlink_to(target)
    results = bca.analyze_batch([link])
    assert isinstance(results[0], dict)


def test_large_batch_processes_every_input() -> None:
    """1k duplicated paths — smoke test that ``analyze_batch``
    completes in CI time on a representative bulk input.

    What this test does NOT catch: at n=1000 with ~8B refs per
    result, even an O(n²) regression in result storage only
    reaches ~8 MB, well below any CI runner's OOM threshold —
    quadratic blow-up would need n ≥ 10k AND distinct paths
    (to defeat any cache that's O(1) for repeated inputs) to be
    detectable here. Treat this as a "does it finish" smoke
    rather than a perf regression guard; the issue's aspirational
    10k+ target is exercised manually, not in CI.
    """
    paths = [FIXTURES / "hello.py"] * 1_000
    results = bca.analyze_batch(paths)
    assert len(results) == 1_000
    assert all(isinstance(r, dict) for r in results)


def test_analysis_error_round_trips_through_pickle() -> None:
    """``pickle.dumps`` / ``pickle.loads`` preserves all three fields.

    Unlocks the ``concurrent.futures.ProcessPoolExecutor`` and
    ``multiprocessing.Pool`` workflows for CPU-bound batch
    pipelines. The ``__reduce__`` hook on the Rust side reuses the
    public ``__init__`` constructor, so an out-of-taxonomy
    ``error_kind`` in a tampered pickle stream still raises
    ``ValueError`` on ``pickle.loads`` — the second assertion
    below pins that validation actually runs on the way back in
    (without it, a future regression that bypassed ``__init__``
    via ``__setstate__`` would still pass the round-trip
    assertion but silently weaken the closed-taxonomy contract).
    """
    original = bca.AnalysisFailure("p.py", "boom", "IoError")
    revived = pickle.loads(pickle.dumps(original))
    assert revived == original
    assert revived.path == "p.py"
    assert revived.error == "boom"
    assert revived.error_kind == "IoError"
    # Hash equality is the dedup contract; round-tripping must
    # preserve it so a worker-process result can be looked up in
    # a parent-process set.
    assert hash(revived) == hash(original)

    # Tamper-rejection: substitute an out-of-taxonomy kind in the
    # pickle byte stream and confirm ``pickle.loads`` raises.
    # ``IoError`` and ``BadKind`` are both 7 chars so the pickle
    # length prefix stays valid — the substitution mutates the
    # value but not the framing.
    tampered = pickle.dumps(original).replace(b"IoError", b"BadKind")
    # Pin the *type* (ValueError) — the pickle-rejection wording is
    # an implementation detail of __setstate__, not the contract.
    with pytest.raises(ValueError):  # noqa: PT011
        pickle.loads(tampered)


def test_analysis_error_repr_round_trips_through_eval_for_non_ascii() -> None:
    """``eval(repr(err))`` reconstructs the object even when fields
    contain non-printable / non-ASCII characters.

    Pins the docstring claim that ``__repr__`` produces a Python
    literal. A regression to Rust's ``{:?}`` Debug formatter would
    emit ``\\u{1F600}`` brace syntax — valid Rust but ``SyntaxError``
    in Python — which the bare-ASCII fixtures elsewhere in this
    file do not catch.
    """
    err = bca.AnalysisFailure("/tmp/\x01中.py", "boom ሴ", "IoError")
    # ``eval`` needs ``AnalysisFailure`` bound in scope to reconstruct.
    revived = eval(repr(err), {"AnalysisFailure": bca.AnalysisFailure})
    assert revived == err


def test_generated_file_is_skipped_by_default(tmp_path: Path) -> None:
    """Since 2.0 (#542) ``analyze_batch`` defaults to
    ``skip_generated=True``, aligning with single-file ``analyze()``.

    A generated file therefore yields **no** result element, so the
    output list is shorter than the input. This is the behaviour
    test that fails against the pre-2.0 ``skip_generated=False``
    default (which produced one dict per input).
    """
    generated = tmp_path / "gen.rs"
    generated.write_bytes(b"// @generated by some-tool. DO NOT EDIT.\npub fn x() {}\n")

    # Single-file analyze() with the default skip_generated=True
    # returns None for the same file — confirms the fixture is
    # actually marked as generated.
    assert bca.analyze(generated) is None

    # Aligned default: the generated file is skipped, not analysed.
    results = bca.analyze_batch([generated])
    assert results == [], (
        "with the aligned skip_generated=True default a generated file "
        "must be skipped, leaving no result element"
    )


def test_skip_generated_false_restores_one_result_per_input(tmp_path: Path) -> None:
    """``skip_generated=False`` is the explicit opt-out (the pre-2.0
    default) that guarantees one element per input position.

    It covers the *generated* skip source only; the read gate is the
    other one, and it is unconditional — see
    ``test_skip_generated_false_holds_the_slot_for_unreadable_inputs``
    for the case this test cannot reach (#1238).
    """
    generated = tmp_path / "gen.rs"
    generated.write_bytes(b"// @generated by some-tool. DO NOT EDIT.\npub fn x() {}\n")

    results = bca.analyze_batch([generated], skip_generated=False)
    assert len(results) == 1
    assert isinstance(results[0], dict), (
        "skip_generated=False must analyse the generated file, yielding one dict per input"
    )


# One input per branch of the walker's ``read_file_with_eol`` gate:
# three bytes (treated as empty), a UTF-16 BOM (rejected on the BOM,
# before any UTF-8 validation runs), and a leading window that is not
# valid UTF-8 and carries no BOM. All three skips are unconditional —
# no kwarg turns them off — which is what made them outlive
# ``skip_generated=False`` (#1238).
_TINY_SOURCE = b"ab\n"
_UTF16_BOM_SOURCE = b"\xff\xfe\x00\x01 fn main() {}\n"
_BINARY_SOURCE = b"\x80\x81\x82\x83\n"


def _unreadable_batch(tmp_path: Path) -> list[Path]:
    """The issue #1238 reproducer: every read-gate skip ahead of a
    perfectly ordinary file, so a dropped slot shows up as the good
    file's metrics being attributed to ``tiny.rs``.

    Returns the inputs in order; the last is the only parseable file,
    so callers can assert on ``results[-1]``.
    """
    written: list[Path] = []
    for name, content in (
        ("tiny.rs", _TINY_SOURCE),
        ("bom.rs", _UTF16_BOM_SOURCE),
        ("binary.rs", _BINARY_SOURCE),
        ("good.rs", b"fn main() {}\n"),
    ):
        path = tmp_path / name
        path.write_bytes(content)
        written.append(path)
    return written


def test_skip_generated_false_holds_the_slot_for_unreadable_inputs(
    tmp_path: Path,
) -> None:
    """#1238: ``skip_generated=False`` must keep the list index-aligned
    even for files the read gate declines to parse.

    Before the fix this returned a single element for three inputs, so
    ``zip(inputs, results)`` — the pattern the docstrings endorse —
    attributed ``good.rs``'s metrics to ``tiny.rs``, silently, with no
    error and no ``AnalysisFailure``.
    """
    inputs = _unreadable_batch(tmp_path)
    good = inputs[-1]

    # Single-file `analyze` returns None for each declined file, which
    # is the shape the batch placeholder mirrors. Asserting it here
    # keeps the fixtures honest: if a gate change ever made one of these
    # parseable, the test below would pass for the wrong reason.
    for declined in inputs[:-1]:
        assert bca.analyze(declined) is None, f"{declined.name} must be declined"

    results = bca.analyze_batch(inputs, skip_generated=False)

    assert len(results) == len(inputs), (
        "skip_generated=False must yield one element per input so "
        "zip(inputs, results) keeps its pairing"
    )
    for index, slot in enumerate(results[:-1]):
        assert slot is None, (
            f"slot {index}: a file the read gate declines must hold its "
            "slot as None, matching what single-file analyze() returns"
        )
    surviving = results[-1]
    assert isinstance(surviving, dict)
    assert surviving["name"] == str(good), (
        "the parseable input must land in its own slot rather than shifting onto tiny.rs"
    )


def test_read_gate_skips_still_drop_their_slot_by_default(tmp_path: Path) -> None:
    """#1238 changed nothing under the default: a file the read gate
    declines yields no element at all, so the list stays shorter than
    the input.

    ``good.rs`` is in the batch so the length assertion is ``1`` rather
    than the ``0`` an all-skipped input would produce whether or not the
    drop happened.
    """
    inputs = _unreadable_batch(tmp_path)

    results = bca.analyze_batch(inputs)

    assert len(results) == 1, "with skip_generated=True an unreadable file yields no slot"
    [only] = results
    assert isinstance(only, dict)
    assert only["name"] == str(inputs[-1])


def test_to_sarif_accepts_the_placeholder_slots(tmp_path: Path) -> None:
    """The #1238 placeholders must stay transparent to ``to_sarif``,
    which documents ``None`` entries as "no record emitted" and skips
    them — so a caller can hand it ``analyze_batch``'s list verbatim.

    This is a forward guard, not regression coverage for #1238: it
    passes against the pre-fix code too, because a dropped slot and a
    skipped slot render the same document. What it would catch is a
    future ``to_sarif`` that raised on — or emitted a finding for — a
    ``None`` entry.
    """
    inputs = _unreadable_batch(tmp_path)
    results = bca.analyze_batch(inputs, skip_generated=False)

    # A threshold every analysed file trips, so "no findings" cannot be
    # mistaken for "the placeholders were skipped".
    doc = json.loads(bca.to_sarif(results, thresholds={"loc.sloc": 0}))

    [run] = doc["runs"]
    flagged = [
        location["physicalLocation"]["artifactLocation"]["uri"]
        for result in run["results"]
        for location in result["locations"]
    ]
    assert flagged == [str(inputs[-1])], (
        "only the parseable file may produce a finding; the None "
        "placeholders are skipped, not rendered as findings or errors"
    )


def test_exclude_tests_kwarg_is_effective(tmp_path: Path) -> None:
    """``exclude_tests=True`` mirrors ``analyze``: a ``#[test]`` fn is
    pruned from the batch result, so the two entry points agree."""
    src = tmp_path / "lib.rs"
    src.write_bytes(b"pub fn prod() {}\n#[test]\nfn t() {}\n")

    [with_tests] = bca.analyze_batch([src])
    [without_tests] = bca.analyze_batch([src], exclude_tests=True)
    assert isinstance(with_tests, dict)
    assert isinstance(without_tests, dict)

    # `with_tests["spaces"]` is `list[FuncSpaceDict]` (#623); the
    # isinstance guards above narrow the batch entries off AnalysisFailure.
    with_spaces = with_tests["spaces"]
    without_spaces = without_tests["spaces"]
    assert len(without_spaces) < len(with_spaces), (
        "exclude_tests=True must drop the #[test] function space"
    )


def test_allow_lossy_path_kwarg_is_accepted(tmp_path: Path) -> None:
    """``allow_lossy_path`` is exposed as a real kwarg (additive).

    A valid UTF-8 path is unaffected by the policy, so the result is
    identical to the default — this pins that the kwarg is accepted
    and plumbed without changing the common-case output.
    """
    src = tmp_path / "main.rs"
    src.write_bytes(b"fn main() {}\n")

    [default] = bca.analyze_batch([src])
    [lossy] = bca.analyze_batch([src], allow_lossy_path=True)
    assert default == lossy
