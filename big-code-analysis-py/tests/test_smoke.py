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
    """
    cargo_toml = REPO_ROOT / "Cargo.toml"
    in_workspace_package = False
    for line in cargo_toml.read_text().splitlines():
        stripped = line.strip()
        if stripped == "[workspace.package]":
            in_workspace_package = True
            continue
        if stripped.startswith("[") and stripped.endswith("]"):
            in_workspace_package = False
            continue
        if in_workspace_package and stripped.startswith("version"):
            _, _, value = stripped.partition("=")
            return value.strip().strip('"')
    raise RuntimeError("could not parse workspace version from Cargo.toml")


def _bca_binary() -> str | None:
    """Locate the ``bca`` CLI binary.

    Prefer ``$BCA_BINARY`` if set; otherwise look in the workspace's
    ``target/debug`` and ``target/release`` directories. Tests that
    require the CLI are skipped when the binary cannot be found.
    """
    env_path = os.environ.get("BCA_BINARY")
    if env_path and Path(env_path).is_file():
        return env_path
    for candidate in (
        REPO_ROOT / "target" / "debug" / "bca",
        REPO_ROOT / "target" / "release" / "bca",
    ):
        if candidate.is_file():
            return str(candidate)
    return shutil.which("bca")


def _cli_metrics(path: Path) -> Any:
    """Run ``bca metrics --output-format json`` on ``path`` and parse.

    Skips the calling test if the CLI binary is unavailable.
    """
    bca_path = _bca_binary()
    if bca_path is None:
        pytest.skip("bca CLI not built; run `cargo build` first or set BCA_BINARY")
    result = subprocess.run(
        [bca_path, "metrics", "--output-format", "json", "--paths", str(path)],
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
    ],
)
def test_analyze_matches_cli_json(fixture: str) -> None:
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
    cli_result = _cli_metrics(path)
    assert py_result == cli_result


@pytest.mark.parametrize(
    "fixture",
    [
        "hello.py",
        "hello.rs",
        "Hello.java",
        "hello.cpp",
    ],
)
def test_analyze_key_order_matches_cli(fixture: str) -> None:
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
    cli_result = _cli_metrics(path)
    assert list(py_result.keys()) == list(cli_result.keys()), (
        f"top-level key order diverged: py={list(py_result.keys())} "
        f"cli={list(cli_result.keys())}"
    )


def test_analyze_source_str_bytes_bytearray_agree() -> None:
    """All three input forms must yield byte-identical results."""
    code = "def f(x):\n    return x + 1\n"
    from_str = bca.analyze_source(code, "python")
    from_bytes = bca.analyze_source(code.encode("utf-8"), "python")
    from_bytearray = bca.analyze_source(bytearray(code, "utf-8"), "python")
    assert from_str == from_bytes == from_bytearray


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
    with pytest.raises(bca.UnsupportedLanguageError):
        bca.analyze(bogus)


def test_analyze_source_raises_unsupported_language_for_unknown_name() -> None:
    with pytest.raises(bca.UnsupportedLanguageError):
        bca.analyze_source("noise", "klingon")


def test_language_extensions_raises_unsupported_language_for_unknown_name() -> None:
    with pytest.raises(bca.UnsupportedLanguageError):
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
    missing = tmp_path / "does_not_exist.rs"
    with pytest.raises(FileNotFoundError) as exc_info:
        bca.analyze(missing)
    err = exc_info.value
    assert err.errno == 2  # ENOENT
    assert err.filename == str(missing)


def test_analyze_source_rejects_non_text_non_bytes_code() -> None:
    with pytest.raises(ValueError):
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
    assert isinstance(result["spaces"], list)
    assert isinstance(result["metrics"], dict)
