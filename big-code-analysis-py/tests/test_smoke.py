"""Smoke tests for the ``big_code_analysis`` Python bindings.

These tests exercise the public API and pin the JSON byte-for-byte
parity against the ``bca`` CLI for one fixture per representative
language. Run via::

    uv pip install -e .[dev]
    maturin develop
    python -m pytest big-code-analysis-py/tests
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import tomllib
from pathlib import Path
from typing import Any

import pytest

import big_code_analysis as bca

REPO_ROOT = Path(__file__).resolve().parents[2]
FIXTURES = Path(__file__).parent / "fixtures"


def _workspace_version() -> str:
    """Read ``[workspace.package].version`` from the root Cargo.toml.

    Avoids hard-coding the version in the test — bumping the workspace
    version must propagate to ``bca.__version__`` without test edits.
    Uses ``tomllib`` (stdlib since 3.11; the bindings target >=3.12)
    rather than a hand-rolled line parser, which would mis-match a
    future ``versioning-strategy = ...`` key or a multi-line value.
    """
    cargo_toml = REPO_ROOT / "Cargo.toml"
    with cargo_toml.open("rb") as fh:
        data = tomllib.load(fh)
    return data["workspace"]["package"]["version"]


def _workspace_target_dir() -> Path:
    """Resolve the cargo target directory, honouring ``$CARGO_TARGET_DIR``.

    Falls back to ``<repo-root>/target`` when the env var is unset.
    Without this, CI configs that set ``$CARGO_TARGET_DIR`` (sccache,
    isolated build dirs, …) would build the CLI successfully but
    leave ``_locate_workspace_binary`` returning ``None``.
    """
    env_dir = os.environ.get("CARGO_TARGET_DIR")
    if env_dir:
        return Path(env_dir)
    return REPO_ROOT / "target"


def _locate_workspace_binary() -> str | None:
    """Look for a freshly-built ``bca`` under the workspace target dir.

    Searches the standard ``debug`` and ``release`` subdirs of the
    effective ``CARGO_TARGET_DIR``. Returns ``None`` if neither
    exists — the fixture's job is then to invoke ``cargo build`` and
    retry.

    Deliberately does NOT fall back to ``shutil.which("bca")`` — a
    system-wide ``bca`` may be a different version than the
    workspace currently checked out, which would silently break the
    parity tests with mismatched JSON shape. The parity contract is
    tested against THIS workspace's CLI; the fixture builds one if
    needed.
    """
    target = _workspace_target_dir()
    for candidate in (target / "debug" / "bca", target / "release" / "bca"):
        if candidate.is_file():
            return str(candidate)
    return None


@pytest.fixture(scope="session")
def bca_binary() -> str:
    """Session-scoped path to the ``bca`` CLI binary, built if needed.

    The parity tests compare ``analyze()`` output against the CLI's
    JSON, so a missing CLI silently skipping every parity test
    (G1: ``pytest.skip`` with no signal in the summary) would defeat
    the whole contract. This fixture instead BUILDS the CLI on demand
    via ``cargo build -p big-code-analysis-cli`` and only fails when
    even that cannot succeed (no cargo, no source, network down on a
    fresh check-out, …) — surfaced as a fixture error, not a silent
    skip.

    Always invokes ``cargo build`` unless ``$BCA_BINARY`` is set
    explicitly. Cargo is idempotent (no-op when nothing changed), so
    the repeated invocation is essentially free on cache hits. The
    rebuild is load-bearing: a stale ``target/debug/bca`` from a
    previous branch checkout is the same failure class as a system-
    wide ``bca`` of a different version — the fixture's contract is
    "match the source tree currently checked out", and only a
    freshly-built binary satisfies that.
    """
    env_path = os.environ.get("BCA_BINARY")
    if env_path:
        # Caller knows what they're doing — trust the path without
        # rebuilding. Useful for CI configs that build the CLI in a
        # separate job and pass it through artifact upload/download.
        if not Path(env_path).is_file():
            pytest.fail(
                f"$BCA_BINARY={env_path!r} does not point at a "
                "regular file; either fix the path or unset to let "
                "the fixture build the CLI."
            )
        return env_path
    cargo = shutil.which("cargo")
    if cargo is None:
        pytest.fail(
            "bca CLI cannot be built: `cargo` is not on PATH. Set "
            "$BCA_BINARY to a prebuilt binary, or install Rust and "
            "re-run."
        )
    result = subprocess.run(
        [cargo, "build", "-p", "big-code-analysis-cli", "--quiet"],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        pytest.fail(
            "`cargo build -p big-code-analysis-cli` failed; parity "
            f"tests cannot run.\nstdout:\n{result.stdout}\n"
            f"stderr:\n{result.stderr}"
        )
    built = _locate_workspace_binary()
    if built is None:
        target = _workspace_target_dir()
        pytest.fail(
            f"cargo build succeeded but no bca binary was found "
            f"under {target}/{{debug,release}}/. If you set "
            "$CARGO_TARGET_DIR, ensure the fixture sees the same "
            "value as the cargo invocation."
        )
    return built


def _cli_metrics(bca_path: str, path: Path, *, exclude_tests: bool = False) -> Any:
    """Run ``bca metrics --output-format json`` on ``path`` and parse.

    ``--exclude-tests`` is the CLI's global flag (declared on
    ``GlobalOpts`` in ``big-code-analysis-cli/src/lib.rs``); clap
    accepts it either before the subcommand or after it. We place
    it before ``metrics`` to match the documented invocation form
    in the README, which is what users will type to reach for the
    Python kwarg via the bindings.
    """
    argv = [bca_path]
    if exclude_tests:
        argv.append("--exclude-tests")
    argv.extend(["metrics", "--output-format", "json", "--paths", str(path)])
    result = subprocess.run(
        argv,
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(result.stdout)


# ----- Version pin ---------------------------------------------------------


def test_version_matches_workspace_cargo_toml() -> None:
    assert bca.__version__ == _workspace_version()


# ----- Round-trip parity vs the CLI ---------------------------------------


@pytest.mark.parametrize(
    "fixture",
    [
        "hello.py",
        "hello.rs",
        "Hello.java",
        "hello.cpp",
        # Extension-less script with a `#!/usr/bin/env python`
        # shebang. Exercises CLI parity for `guess_language`'s
        # shebang fallback (#314) — both the CLI and the bindings
        # must resolve the language from the first line and emit
        # the same `FuncSpace` JSON.
        "install",
    ],
)
def test_analyze_matches_cli_json(fixture: str, bca_binary: str) -> None:
    """``analyze(path)`` must match ``bca metrics --output-format json`` byte-for-byte.

    Both sides serialise the same ``FuncSpace`` through
    ``serde_json::to_string``; the bindings then parse that JSON
    with CPython's ``json.loads`` (which preserves insertion order
    on 3.7+). So the parsed CLI JSON and the bindings' returned
    dict are equal *with key order*, not just structurally.

    The dict-equality check below is insensitive to key order, but
    the dedicated ``test_analyze_key_order_matches_cli`` test pins
    the ordering explicitly so a future regression to the old
    ``to_value``-based path (which silently sorted keys
    alphabetically via ``BTreeMap``) is caught.

    Note: Python's ``dict ==`` treats ``1 == 1.0`` as equal, so an
    int-vs-float drift between the two sides would pass silently
    here. The current metric serialisers emit identical numeric
    types on both sides, so this is not a present concern.
    """
    path = FIXTURES / fixture
    py_result = bca.analyze(path)
    cli_result = _cli_metrics(bca_binary, path)
    assert py_result == cli_result


@pytest.mark.parametrize(
    "fixture",
    [
        "hello.py",
        "hello.rs",
        "Hello.java",
        "hello.cpp",
        # Extension-less script with a `#!/usr/bin/env python`
        # shebang. Exercises CLI parity for `guess_language`'s
        # shebang fallback (#314) — both the CLI and the bindings
        # must resolve the language from the first line and emit
        # the same `FuncSpace` JSON.
        "install",
    ],
)
def test_analyze_key_order_matches_cli(fixture: str, bca_binary: str) -> None:
    """Bindings must preserve the CLI's ``FuncSpace`` field order.

    Parses the CLI's stdout with ``json.loads`` so both sides yield
    CPython dicts populated in serialisation order, then walks the
    top-level keys and asserts they line up. This is the regression
    test for the byte-for-byte claim: if anyone re-introduces the
    ``serde_json::to_value`` path (which routes through
    ``BTreeMap`` and re-sorts keys alphabetically), this test
    fails immediately.
    """
    path = FIXTURES / fixture
    py_result = bca.analyze(path)
    cli_result = _cli_metrics(bca_binary, path)
    assert list(py_result.keys()) == list(cli_result.keys()), (
        f"top-level key order diverged: py={list(py_result.keys())} "
        f"cli={list(cli_result.keys())}"
    )


def test_analyze_exclude_tests_matches_cli_for_rust_fixture(bca_binary: str) -> None:
    """``analyze(path, exclude_tests=True)`` must match ``bca --exclude-tests``.

    Uses the ``rust_with_tests.rs`` fixture, which has a production
    function plus a ``#[cfg(test)] mod tests`` block containing two
    ``#[test]`` functions and one helper. Without ``exclude_tests``,
    the FuncSpace counts the four functions; with it, only ``prod``
    survives — the language checker's ``should_skip_subtree`` hook
    prunes the ``#[cfg(test)]`` module before any metric runs.

    The parity check is a structural ``dict ==``: both sides must
    yield byte-equivalent ``FuncSpace`` JSON. Asserting the pruned
    ``functions`` count explicitly anchors the test against a
    regression where the kwarg is silently ignored (both sides
    would still agree at 4, but the contract would be broken).
    """
    path = FIXTURES / "rust_with_tests.rs"
    py_pruned = bca.analyze(path, exclude_tests=True)
    cli_pruned = _cli_metrics(bca_binary, path, exclude_tests=True)
    assert py_pruned == cli_pruned

    # Anchor: only `prod` must remain after pruning.
    assert py_pruned["metrics"]["nom"]["functions"] == 1.0, (
        f"expected exclude_tests to prune everything but `prod`, got "
        f"functions={py_pruned['metrics']['nom']['functions']!r}"
    )

    # Sanity: baseline (no flag) sees all four functions, proving the
    # fixture exercises the pruning path rather than a degenerate
    # both-sides-empty agreement.
    py_baseline = bca.analyze(path)
    assert py_baseline["metrics"]["nom"]["functions"] == 4.0, (
        f"baseline should count prod + helper + 2 tests = 4, got "
        f"functions={py_baseline['metrics']['nom']['functions']!r}"
    )


def test_analyze_source_exclude_tests_prunes_rust_tests() -> None:
    """In-memory variant of the Rust ``exclude_tests`` parity check.

    Mirrors the path-based test above but exercises
    ``analyze_source`` so the kwarg surface is covered on both
    PyO3 entry points. No CLI parity check here — the CLI has no
    stdin form for ``analyze_source`` — but the int counts pin
    the load-bearing behaviour.
    """
    source = (
        "fn prod() -> i32 { 1 + 2 }\n"
        "\n"
        "#[test]\n"
        "fn t() { assert_eq!(1 + 1, 2); }\n"
    )
    baseline = bca.analyze_source(source, "rust")
    pruned = bca.analyze_source(source, "rust", exclude_tests=True)
    assert baseline["metrics"]["nom"]["functions"] == 2.0
    assert pruned["metrics"]["nom"]["functions"] == 1.0


def test_analyze_source_exclude_tests_is_no_op_for_non_rust_language() -> None:
    """``exclude_tests=True`` must be a no-op for languages without a
    ``Checker::should_skip_subtree`` override.

    Pins the documented contract that the flag is currently Rust-only
    — Python (and every other language) must emit the identical
    ``FuncSpace`` JSON whether the flag is set or not. Without this
    pin, a future regression that wires the flag through a language
    checker which is *not* opt-in (e.g. matching by attribute name
    in Python's grammar) would silently change Python-side numbers.
    """
    code = "def f(x):\n    return x + 1\n"
    baseline = bca.analyze_source(code, "python")
    flagged = bca.analyze_source(code, "python", exclude_tests=True)
    assert baseline == flagged


def test_analyze_source_str_bytes_bytearray_agree() -> None:
    """All three input forms must yield equivalent results.

    ``dict ==`` is structural (order-insensitive, ``1 == 1.0``), so it
    is a necessary check but not a sufficient one. The companion
    ``json.dumps`` assertion below catches an int-vs-float drift or a
    nested-key reorder between the input forms — both of which would
    leave ``dict ==`` passing silently.
    """
    code = "def f(x):\n    return x + 1\n"
    from_str = bca.analyze_source(code, "python")
    from_bytes = bca.analyze_source(code.encode("utf-8"), "python")
    from_bytearray = bca.analyze_source(bytearray(code, "utf-8"), "python")
    assert from_str == from_bytes == from_bytearray
    # `sort_keys=False` (the default) preserves CPython dict insertion
    # order, so this comparison fires on any divergence — including
    # the int-vs-float and nested-reorder cases that `dict ==` masks.
    assert json.dumps(from_str) == json.dumps(from_bytes) == json.dumps(from_bytearray)


# ----- Language metadata --------------------------------------------------


def test_supported_languages_is_non_empty_list_of_strings() -> None:
    langs = bca.supported_languages()
    assert isinstance(langs, list)
    assert langs, "supported_languages() must return at least one language"
    for lang in langs:
        assert isinstance(lang, str) and lang


def test_language_extensions_round_trips_to_language_for_file() -> None:
    """Every advertised extension must resolve back to its language.

    Walks ``supported_languages() x language_extensions(lang)`` and
    asserts ``language_for_file('foo.<ext>')`` returns ``lang`` for
    each pair. This is the Python-side cross-check that backs the
    Rust ``language_extensions_round_trips_for_every_supported_language``
    unit test in ``src/language.rs``.
    """
    for lang in bca.supported_languages():
        extensions = bca.language_extensions(lang)
        assert extensions, f"language {lang} has no extensions registered"
        for ext in extensions:
            resolved = bca.language_for_file(f"foo.{ext}")
            assert resolved == lang, (
                f".{ext} resolved to {resolved}, expected {lang}"
            )


def test_language_for_file_returns_none_for_unknown_extension() -> None:
    assert bca.language_for_file("foo.unknownext") is None
    assert bca.language_for_file("README") is None


def test_language_for_file_accepts_pathlike() -> None:
    """PEP 519 — ``os.PathLike`` objects must work, not just ``str``."""
    p = Path("foo.py")
    assert bca.language_for_file(p) == "python"


# ----- Error mapping -------------------------------------------------------


def test_unsupported_language_error_is_value_error_subclass() -> None:
    assert issubclass(bca.UnsupportedLanguageError, ValueError)
    assert issubclass(bca.ParseError, ValueError)


def test_analyze_raises_unsupported_language_for_unknown_extension(
    tmp_path: Path,
) -> None:
    bogus = tmp_path / "thing.unknownext"
    bogus.write_text("noise")
    with pytest.raises(
        bca.UnsupportedLanguageError,
        match=r"no language registered for path .*\.unknownext",
    ):
        bca.analyze(bogus)


def test_analyze_source_raises_unsupported_language_for_unknown_name() -> None:
    with pytest.raises(bca.UnsupportedLanguageError, match=r"^klingon$"):
        bca.analyze_source("noise", "klingon")


def test_language_extensions_raises_unsupported_language_for_unknown_name() -> None:
    with pytest.raises(bca.UnsupportedLanguageError, match=r"^klingon$"):
        bca.language_extensions("klingon")


def test_analyze_raises_filenotfounderror_with_errno_and_filename(
    tmp_path: Path,
) -> None:
    """`analyze` on a missing file must dispatch to FileNotFoundError
    and populate ``errno`` / ``filename`` so idiomatic Python handling
    works.

    A bare ``OSError(message)`` (one-argument form) does not trigger
    CPython's subclass dispatch and leaves ``errno`` / ``filename``
    set to ``None``.
    """
    import errno as _errno

    missing = tmp_path / "does_not_exist.rs"
    with pytest.raises(FileNotFoundError) as exc_info:
        bca.analyze(missing)
    err = exc_info.value
    assert err.errno == _errno.ENOENT
    assert err.filename == str(missing)


def test_analyze_source_rejects_non_text_non_bytes_code() -> None:
    # Pin the exact message so a future regression where the bad
    # input falls through to the language resolver (and raises
    # UnsupportedLanguageError, which is also a ValueError subclass)
    # is caught — G7 from the post-#265 code review.
    with pytest.raises(
        ValueError, match=r"code must be str, bytes, or bytearray"
    ):
        bca.analyze_source(12345, "rust")  # type: ignore[arg-type]


# ----- Shape checks --------------------------------------------------------


def test_analyze_source_returns_dict_with_expected_keys() -> None:
    result = bca.analyze_source("fn main() {}\n", "rust")
    assert isinstance(result, dict)
    # FuncSpace serde shape: name / start_line / end_line / kind /
    # spaces / metrics. The first four are always present; `spaces`
    # and `metrics` always render as a list and a dict respectively.
    for key in ("name", "start_line", "end_line", "kind", "spaces", "metrics"):
        assert key in result, f"missing key {key} in {result!r}"
    # `spaces` must contain the inner FuncSpace for `main`, and `metrics`
    # must carry the populated metric table — checking only the container
    # type would let a regression that returned `spaces: [], metrics: {}`
    # pass silently.
    assert isinstance(result["spaces"], list) and result["spaces"], (
        f"expected at least one child FuncSpace for `fn main`, got {result['spaces']!r}"
    )
    assert isinstance(result["metrics"], dict) and result["metrics"], (
        f"expected populated metric table, got {result['metrics']!r}"
    )
    # Spot-check one canonical metric that every language emits — guards
    # against a future regression where `metrics` carries unrelated keys.
    assert "cyclomatic" in result["metrics"], (
        f"expected `cyclomatic` in metrics, got keys {list(result['metrics'])!r}"
    )
