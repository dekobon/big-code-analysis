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
import subprocess
import tomllib
from collections.abc import Mapping
from pathlib import Path
from typing import Any, cast

import big_code_analysis as bca
import pytest

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
    version = data["workspace"]["package"]["version"]
    assert isinstance(version, str)
    return version


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
    assert py_result is not None
    cli_result = _cli_metrics(bca_binary, path)
    assert list(py_result.keys()) == list(cli_result.keys()), (
        f"top-level key order diverged: py={list(py_result.keys())} cli={list(cli_result.keys())}"
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
    assert py_pruned is not None
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
    assert py_baseline is not None
    assert py_baseline["metrics"]["nom"]["functions"] == 4.0, (
        f"baseline should count prod + helper + 2 tests = 4, got "
        f"functions={py_baseline['metrics']['nom']['functions']!r}"
    )


def test_analyze_skip_generated_default_returns_none_for_generated_file() -> None:
    """Default ``skip_generated=True`` must mirror the CLI walker.

    The fixture's first line carries ``@generated ... DO NOT EDIT.``
    so the upstream ``is_generated`` predicate matches. The CLI walker
    drops the file from its output entirely; the bindings surface that
    decision as ``None`` so callers can drop the file from their own
    pipeline with one ``if result is None`` check.

    Test-via-revert: passing ``skip_generated=False`` (covered by the
    next test) demonstrates the kwarg is load-bearing rather than
    always-skip.
    """
    path = FIXTURES / "generated.rs"
    assert bca.analyze(path) is None


def test_analyze_skip_generated_false_parses_generated_file() -> None:
    """``skip_generated=False`` opts out of the CLI walker filter.

    With the marker check bypassed, the same fixture parses normally
    and returns a populated ``FuncSpace`` — the regression check that
    the kwarg actually controls behaviour rather than being ignored.
    The structural check on inner ``spaces`` pins that the Rust
    parser ran successfully (a regression that surfaced an empty
    ``FuncSpace`` would slip past a bare ``kind == "unit"`` check).
    """
    path = FIXTURES / "generated.rs"
    result = bca.analyze(path, skip_generated=False)
    assert result is not None, "skip_generated=False must yield a dict"
    assert result["kind"] == "unit"
    # The fixture defines `pub fn generated()`; with the marker check
    # bypassed, the parser sees it and emits a child FuncSpace.
    spaces: list[Any] = result["spaces"]
    # Match _flatten.py's defensive `isinstance(child, Mapping)` guard
    # rather than a tighter `isinstance(s, dict)` — if a future PyO3
    # change emits e.g. MappingProxyType instead of dict, the test
    # should still observe the children rather than silently dropping
    # them. The pyright ignore acknowledges that the PyO3-returned
    # mapping's value type is genuinely Any.
    inner_names: set[Any] = {
        s.get("name")  # pyright: ignore[reportUnknownMemberType]
        for s in spaces
        if isinstance(s, Mapping)
    }
    assert "generated" in inner_names, (
        f"expected `generated` fn in spaces, got names {inner_names!r}"
    )


def test_analyze_skip_generated_matches_cli_walker(bca_binary: str) -> None:
    """CLI parity for generated files (#317).

    ``bca metrics --output-format json --paths <generated.rs>`` runs
    the walker against a single file: the walker checks
    ``is_generated`` before parsing and emits **nothing at all** for
    our fixture (zero-byte stdout, exit 0). The bindings represent
    that same "no record" decision as ``None`` from ``analyze``.

    The CLI emits per-file JSON records to stdout as it walks; when
    every input is filtered out, there are simply no records to
    write — not an empty array, not a wrapping container. Asserting
    on the raw stdout bytes (rather than ``json.loads``) pins the
    CLI's no-output convention and prevents a future regression
    where the walker starts emitting `null` / `{}` / `[]` for
    skipped files (which would silently break callers piping the
    output through `jq`).
    """
    path = FIXTURES / "generated.rs"
    argv = [
        bca_binary,
        "metrics",
        "--output-format",
        "json",
        "--paths",
        str(path),
    ]
    result = subprocess.run(argv, check=True, capture_output=True, text=True)
    assert result.stdout == "", (
        f"expected CLI to emit no output for a generated file, got "
        f"{result.stdout!r}; if this fails the CLI walker semantics "
        "have changed and the bindings' parity claim needs revisiting."
    )
    assert bca.analyze(path) is None


def test_analyze_combines_skip_generated_false_with_exclude_tests() -> None:
    """Both kwargs cooperate on the same file.

    The fixture is marked ``@generated`` AND carries a ``#[cfg(test)]``
    block. With the defaults (``skip_generated=True``) it returns
    ``None`` like the CLI walker. Opting back into parsing
    (``skip_generated=False``) AND simultaneously requesting test
    pruning (``exclude_tests=True``) must:

    1. parse the file (the marker no longer skips it), and
    2. prune the ``#[cfg(test)]`` subtree before metric computation
       (matches the bare-``exclude_tests`` behaviour for non-generated
       files).

    Pins that the three kwargs are independent levers — each one
    routes to a distinct upstream knob (early-return,
    ``MetricsOptions``, ``Path::to_string_lossy``) and they cannot
    interfere with each other. A regression that, say, made the
    ``is_generated`` check toggle ``MetricsOptions::exclude_tests``
    instead of the early-return would slip past the bare-kwarg
    tests but show up here.
    """
    path = FIXTURES / "generated_with_tests.rs"

    # Default: skip_generated=True → bindings (and CLI) see no record.
    assert bca.analyze(path) is None

    # Opt out of the skip but keep exclude_tests on. The combined
    # call must hit the parser AND prune the inner `#[cfg(test)] mod
    # tests` subtree.
    baseline = bca.analyze(path, skip_generated=False, exclude_tests=False)
    pruned = bca.analyze(path, skip_generated=False, exclude_tests=True)
    assert baseline is not None
    assert pruned is not None
    # `nom.functions` counts: `prod`, `helper`, `checks_positive`.
    # Pruning the `#[cfg(test)]` mod removes `helper` + `checks_positive`,
    # leaving only `prod`.
    assert baseline["metrics"]["nom"]["functions"] == 3.0, (
        f"baseline must count prod + helper + checks_positive, got "
        f"{baseline['metrics']['nom']['functions']!r}"
    )
    assert pruned["metrics"]["nom"]["functions"] == 1.0, (
        f"exclude_tests=True must elide the #[cfg(test)] subtree, "
        f"leaving only prod; got {pruned['metrics']['nom']['functions']!r}"
    )


def test_analyze_source_exclude_tests_prunes_rust_tests() -> None:
    """In-memory variant of the Rust ``exclude_tests`` parity check.

    Mirrors the path-based test above but exercises
    ``analyze_source`` so the kwarg surface is covered on both
    PyO3 entry points. No CLI parity check here — the CLI has no
    stdin form for ``analyze_source`` — but the int counts pin
    the load-bearing behaviour.
    """
    source = "fn prod() -> i32 { 1 + 2 }\n\n#[test]\nfn t() { assert_eq!(1 + 1, 2); }\n"
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
        assert isinstance(lang, str)
        assert lang


def test_language_extensions_round_trips_to_language_for_file(
    tmp_path: Path,
) -> None:
    """Every advertised extension must resolve back to its language.

    Walks ``supported_languages() x language_extensions(lang)`` and
    asserts ``language_for_file('foo.<ext>')`` returns ``lang`` for
    each pair. This is the Python-side cross-check that backs the
    Rust ``language_extensions_round_trips_for_every_supported_language``
    unit test in ``src/language.rs``.

    Each ``foo.<ext>`` is materialised under ``tmp_path`` because
    ``language_for_file`` now reads the file before falling back to
    shebang / emacs-mode inspection (#318) — a stub-path string
    would surface as ``FileNotFoundError`` instead of resolving
    through the extension table.
    """
    for lang in bca.supported_languages():
        extensions = bca.language_extensions(lang)
        assert extensions, f"language {lang} has no extensions registered"
        for ext in extensions:
            fixture = tmp_path / f"foo.{ext}"
            fixture.write_bytes(b"")
            resolved = bca.language_for_file(fixture)
            assert resolved == lang, f".{ext} resolved to {resolved}, expected {lang}"


def test_language_for_file_returns_none_for_unknown_extension(
    tmp_path: Path,
) -> None:
    bogus = tmp_path / "foo.unknownext"
    bogus.write_text("noise")
    assert bca.language_for_file(bogus) is None

    plain = tmp_path / "README"
    plain.write_text("plain text\n")
    assert bca.language_for_file(plain) is None


def test_language_for_file_resolves_extension_less_shebang() -> None:
    """#318 — parity with :func:`analyze` for shebang-detected files.

    The ``tests/fixtures/install`` fixture is an extension-less
    script with a ``#!/usr/bin/env python`` first line. Pre-fix,
    ``language_for_file`` was extension-only and returned ``None``
    while ``analyze`` on the same path resolved to Python — the
    asymmetry this test pins closed.
    """
    fixture = Path(__file__).parent / "fixtures" / "install"
    assert bca.language_for_file(fixture) == "python"


def test_language_for_file_raises_file_not_found_for_missing_path(
    tmp_path: Path,
) -> None:
    """#318 — the new contract drops "Never raises".

    A missing file surfaces as :class:`FileNotFoundError` (same
    typed ``OSError`` dispatch :func:`analyze` uses) so callers can
    differentiate "language unknown" (``None``) from "could not
    read the file" — collapsing both to ``None`` would let typos in
    caller paths silently route to "no language."
    """
    missing = tmp_path / "does-not-exist.rs"
    with pytest.raises(FileNotFoundError) as excinfo:
        bca.language_for_file(missing)
    assert excinfo.value.filename == str(missing)
    assert excinfo.value.errno is not None


def test_language_for_file_accepts_pathlike(tmp_path: Path) -> None:
    """PEP 519 — ``os.PathLike`` objects must work, not just ``str``."""
    p = tmp_path / "foo.py"
    p.write_text("print('hi')\n")
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


@pytest.mark.skipif(
    os.name != "posix",
    reason=(
        "Non-UTF-8 path fixtures use OsStrExt::from_bytes / "
        "os.fsencode of a surrogateescape string, which is unix-only. "
        "Windows has its own non-UTF-8 mechanism (unpaired surrogates "
        "via OsStringExt::from_wide); covering it is a separate fixture."
    ),
)
def test_analyze_rejects_non_utf8_path_by_default(tmp_path: Path) -> None:
    """Default policy: non-UTF-8 path bytes raise ``ValueError`` (#316).

    Constructs a path via ``os.fsdecode(bytes)``: on Linux with
    PEP 383 surrogateescape, raw 0xff bytes round-trip through ``str``
    as lone surrogates. PyO3's path extractor calls ``os.fsencode`` on
    the str to recover the original bytes, so the underlying ``Path``
    is the same byte sequence the OS sees.
    """
    raw_name = b"\xff\xff.rs"
    str_name = os.fsdecode(raw_name)
    bogus = tmp_path / str_name
    bogus.write_bytes(b"fn main() {}\n")
    with pytest.raises(
        ValueError,
        match=r"path is not valid UTF-8.*allow_lossy_path=True",
    ):
        bca.analyze(bogus)


@pytest.mark.skipif(
    os.name != "posix",
    reason="See test_analyze_rejects_non_utf8_path_by_default.",
)
def test_analyze_allow_lossy_path_mirrors_cli_substitution(
    tmp_path: Path,
) -> None:
    """``allow_lossy_path=True`` substitutes U+FFFD instead of raising.

    The CLI emits non-UTF-8 path bytes as U+FFFD via
    ``Path::to_string_lossy``; the opt-in kwarg routes the bindings
    through the same call so the ``name`` field matches byte-for-byte
    in lossy mode. Asserts on substring presence (rather than the
    full name) to stay robust against the tempdir prefix.
    """
    raw_name = b"\xff\xff.rs"
    str_name = os.fsdecode(raw_name)
    bogus = tmp_path / str_name
    bogus.write_bytes(b"fn main() {}\n")
    result = bca.analyze(bogus, allow_lossy_path=True)
    assert result is not None
    assert "�" in result["name"], f"expected U+FFFD substitution in name, got {result['name']!r}"
    # Sanity: the file still analysed; lossy mode is a name-rendering
    # toggle, not a parser flag.
    assert result["kind"] == "unit"


def test_analyze_source_rejects_non_text_non_bytes_code() -> None:
    # Pin the exact message so a future regression where the bad
    # input falls through to the language resolver (and raises
    # UnsupportedLanguageError, which is also a ValueError subclass)
    # is caught — G7 from the post-#265 code review.
    with pytest.raises(ValueError, match=r"code must be str, bytes, or bytearray"):
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
    assert isinstance(result["spaces"], list), (
        f"expected list for spaces, got {type(result['spaces']).__name__}: {result['spaces']!r}"
    )
    assert result["spaces"], (
        f"expected at least one child FuncSpace for `fn main`, got {result['spaces']!r}"
    )
    assert isinstance(result["metrics"], dict), (
        f"expected dict for metrics, got {type(result['metrics']).__name__}: {result['metrics']!r}"
    )
    assert result["metrics"], f"expected populated metric table, got {result['metrics']!r}"
    # ``result["metrics"]`` is ``dict[str, Any]`` from the bindings;
    # pyright sees it as ``dict[Unknown, Unknown]`` after the index
    # access. The cast acknowledges the same erasure the previous
    # block flagged.
    metrics_dict = cast("dict[str, Any]", result["metrics"])
    metric_keys: list[str] = list(metrics_dict)
    # Spot-check one canonical metric that every language emits — guards
    # against a future regression where `metrics` carries unrelated keys.
    assert "cyclomatic" in metrics_dict, (
        f"expected `cyclomatic` in metrics, got keys {metric_keys!r}"
    )
