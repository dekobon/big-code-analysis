"""Tests for ``bca.analyze_batch`` and ``bca.AnalysisError``.

Covers the issue-#266 contract: never-raise per-file semantics,
1:1 ordering, the three documented ``error_kind`` values, generator
input, the ``AnalysisError`` constructor's hash/equality/repr
behaviour, and the programmer-error surfaces that still raise.

Run via::

    maturin develop
    python -m pytest big-code-analysis-py/tests/test_batch.py
"""

from __future__ import annotations

import sys
from collections.abc import Iterator
from pathlib import Path

import pytest

import big_code_analysis as bca

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

    Asserts the per-file ``name`` and ``kind`` fields rather than
    deep-comparing the metrics — the latter would re-fight the
    parity tests in ``test_smoke.py``. The point here is that
    batch does not collapse the language map onto a single
    detector instance.
    """
    paths = [FIXTURES / "hello.py", FIXTURES / "Hello.java", FIXTURES / "hello.rs"]
    results = bca.analyze_batch(paths)
    assert all(isinstance(r, dict) for r in results)
    names = [r["name"] for r in results if isinstance(r, dict)]
    assert names == [str(p) for p in paths]


def test_ordering_preserved_with_interleaved_failures(tmp_path: Path) -> None:
    """Result list zips back against the input list 1:1.

    The success / failure / success pattern catches a regression
    that *appends* errors at the end instead of slotting them at
    the matching index.
    """
    paths = [
        FIXTURES / "hello.py",
        tmp_path / "does_not_exist.py",
        FIXTURES / "hello.rs",
    ]
    results = bca.analyze_batch(paths)
    assert len(results) == 3
    assert isinstance(results[0], dict)
    assert isinstance(results[1], bca.AnalysisError)
    assert isinstance(results[2], dict)


# ── analyze_batch: error taxonomy ───────────────────────────────


def test_missing_file_yields_io_error(tmp_path: Path) -> None:
    missing = tmp_path / "absent.py"
    results = bca.analyze_batch([missing])
    assert len(results) == 1
    err = results[0]
    assert isinstance(err, bca.AnalysisError)
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
    assert isinstance(err, bca.AnalysisError)
    assert err.error_kind == "UnsupportedLanguage"


def test_directory_path_yields_io_error(tmp_path: Path) -> None:
    """A path that exists but is a directory.

    ``std::fs::read`` returns ``EISDIR`` here, which the bridge
    maps to ``AnalysisError::Io``; in batch that flows through to
    ``error_kind="IoError"``. Pins the I/O arm against the
    directory case without needing to fabricate a permission
    setup that varies by CI runner.
    """
    results = bca.analyze_batch([tmp_path])
    err = results[0]
    assert isinstance(err, bca.AnalysisError)
    assert err.error_kind == "IoError"


# ParseError is currently unreachable from the Python surface:
# tree-sitter is permissive and the upstream `MetricsError::Parse*`
# variants are reserved for a future strict-parse mode. The issue
# (#266) lists a ParseError test for completeness; we keep the
# coverage on the mapping itself in ``src/batch.rs`` (Rust unit
# tests) and mark the Python-side check as expected-to-skip so a
# future strict-parse landing flips it green without test edits.
@pytest.mark.xfail(
    reason="tree-sitter is permissive — MetricsError::Parse not yet reachable; "
    "kept as a tripwire for the strict-parse mode reserved upstream.",
    strict=False,
)
def test_syntactically_broken_source_yields_parse_error(tmp_path: Path) -> None:
    broken = tmp_path / "broken.rs"
    broken.write_bytes(b"\x00\x01\x02\x03 not even close to source")
    results = bca.analyze_batch([broken])
    err = results[0]
    assert isinstance(err, bca.AnalysisError)
    assert err.error_kind == "ParseError"


# ── analyze_batch: input shapes ─────────────────────────────────


def test_empty_iterable_returns_empty_list() -> None:
    assert bca.analyze_batch([]) == []
    assert bca.analyze_batch(iter([])) == []
    assert bca.analyze_batch(p for p in ()) == []


def test_generator_input_works() -> None:
    """Generators are consumed lazily without being materialised first."""

    def gen() -> Iterator[Path]:
        yield FIXTURES / "hello.py"
        yield FIXTURES / "hello.rs"

    results = bca.analyze_batch(gen())
    assert len(results) == 2
    assert all(isinstance(r, dict) for r in results)


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

    ``metrics=None`` (the default) means "no selection"; an
    explicit empty list is meaningless — the analysis would
    silently compute nothing. Phase 4 (#268) will add the
    selection plumbing; today the kwarg is only validated.
    """
    with pytest.raises(ValueError):
        bca.analyze_batch([FIXTURES / "hello.py"], metrics=[])


def test_none_metrics_is_accepted() -> None:
    results = bca.analyze_batch([FIXTURES / "hello.py"], metrics=None)
    assert isinstance(results[0], dict)


def test_unknown_metric_name_accepted_today_but_reserved() -> None:
    """Today the kwarg is validated only for emptiness.

    Phase 4 (#268) will start rejecting unknown names; pinning the
    current behaviour avoids a silent contract drift.
    """
    results = bca.analyze_batch([FIXTURES / "hello.py"], metrics=["bogus"])
    assert isinstance(results[0], dict)


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
    assert all(isinstance(r, bca.AnalysisError) for r in results)
    kinds = [r.error_kind for r in results if isinstance(r, bca.AnalysisError)]
    assert kinds == ["IoError", "UnsupportedLanguage", "IoError"]


# ── AnalysisError: shape / equality / hashing ───────────────────


def test_analysis_error_attributes_are_readable() -> None:
    err = bca.AnalysisError("p.py", "boom", "IoError")
    assert err.path == "p.py"
    assert err.error == "boom"
    assert err.error_kind == "IoError"


def test_analysis_error_is_not_an_exception_subclass() -> None:
    """Sanity check: ``AnalysisError`` is *returned*, never raised.

    Catches a regression where someone makes it inherit
    ``BaseException`` (perhaps to enable ``raise``-style use); the
    issue contract explicitly forbids that so callers do not have
    to worry about implicit exception propagation through ``raise
    AnalysisError(...)`` typos.
    """
    err = bca.AnalysisError("p.py", "boom", "IoError")
    assert not isinstance(err, BaseException)


def test_analysis_error_equality_by_value() -> None:
    a = bca.AnalysisError("p.py", "msg", "IoError")
    b = bca.AnalysisError("p.py", "msg", "IoError")
    c = bca.AnalysisError("p.py", "msg", "ParseError")
    d = bca.AnalysisError("q.py", "msg", "IoError")
    assert a == b
    assert a != c
    assert a != d


def test_analysis_error_hash_matches_equality() -> None:
    """Equal errors hash equal; the contract that lets callers
    deduplicate via ``set`` / ``dict`` keys.
    """
    a = bca.AnalysisError("p.py", "msg", "IoError")
    b = bca.AnalysisError("p.py", "msg", "IoError")
    assert hash(a) == hash(b)
    assert len({a, b}) == 1
    assert len({a, bca.AnalysisError("p.py", "other", "IoError")}) == 2


def test_analysis_error_repr_includes_all_fields() -> None:
    """``repr`` is debuggable — every field appears, labelled.

    A bare ``repr`` defaulting to ``<AnalysisError at 0x...>``
    would slip past a less specific assertion; pinning each field
    by name keeps the repr useful for ``log.warning(r)`` output.
    """
    err = bca.AnalysisError("p.py", "missing", "IoError")
    r = repr(err)
    assert "AnalysisError" in r
    assert "path=" in r and "p.py" in r
    assert "error=" in r and "missing" in r
    assert "error_kind=" in r and "IoError" in r


def test_analysis_error_rejects_unknown_kind() -> None:
    """Constructor validates ``error_kind`` against the closed set.

    The taxonomy is part of the public contract; silent acceptance
    of typos like ``"IOError"`` (note the capitalised middle ``O``)
    would let callers ship code that compares against a value
    that batch will never actually emit.
    """
    with pytest.raises(ValueError):
        bca.AnalysisError("p.py", "msg", "NotARealKind")
    with pytest.raises(ValueError):
        bca.AnalysisError("p.py", "msg", "IOError")


def test_analysis_error_is_frozen() -> None:
    """``#[pyclass(frozen)]`` means assignment to fields raises.

    A regression that drops ``frozen`` would let callers mutate
    ``err.path`` in place — breaking the ``set`` / ``dict``-key
    use case (mutation would corrupt hash consistency).
    """
    err = bca.AnalysisError("p.py", "msg", "IoError")
    with pytest.raises(AttributeError):
        err.path = "other.py"  # type: ignore[misc]


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
    """1k duplicated paths — pins the linear scan, no per-result
    blow-up (each output is a small dict-or-error).

    The issue mentions "10k+ paths" as an aspirational target; the
    1k bar is what we run in CI without a slow-test gate. A
    regression to O(n²) memory in result storage would surface as
    a flaky OOM here even at 1k.
    """
    paths = [FIXTURES / "hello.py"] * 1_000
    results = bca.analyze_batch(paths)
    assert len(results) == 1_000
    assert all(isinstance(r, dict) for r in results)
