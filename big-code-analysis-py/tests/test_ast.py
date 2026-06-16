"""Tests for the ``Ast`` parse-once seam (issue #727).

Exercise the public ``big_code_analysis.Ast`` handle: parse once, then draw
metrics, the node tree, function spans, the Halstead op tree, a node count,
comment-stripped source, and suppression markers from the same parse. Parity
against ``analyze`` / ``analyze_source`` is pinned where it matters (the whole
point of the seam is that structure and metrics never disagree).
"""

from __future__ import annotations

import tomllib
from pathlib import Path

import big_code_analysis as bca
import pytest
from big_code_analysis import AstNodeDict

REPO_ROOT = Path(__file__).resolve().parents[2]
FIXTURES = Path(__file__).parent / "fixtures"

RUST_SRC = "fn main() {\n    let x = 1;\n}\n"


def _grammar_pin(dep_key: str) -> str:
    """Read the pinned ``=X.Y.Z`` for ``dep_key`` from the workspace manifest.

    Lets the grammar-version assertion track the real pin without
    hard-coding a version the next grammar bump would falsify.
    """
    data = tomllib.loads((REPO_ROOT / "Cargo.toml").read_text())
    spec = data["workspace"]["dependencies"][dep_key]
    raw = spec if isinstance(spec, str) else spec["version"]
    return raw.lstrip("=")


# ----- parse ---------------------------------------------------------------


def test_parse_detects_language_and_source() -> None:
    ast = bca.Ast.parse(RUST_SRC, "rust")
    assert ast.language == "rust"
    assert ast.source == RUST_SRC.encode()


@pytest.mark.parametrize("code", [RUST_SRC, RUST_SRC.encode(), bytearray(RUST_SRC.encode())])
def test_parse_accepts_str_bytes_bytearray(code: str | bytes | bytearray) -> None:
    ast = bca.Ast.parse(code, "rust")
    assert ast.source == RUST_SRC.encode()


def test_parse_is_case_insensitive_on_language() -> None:
    assert bca.Ast.parse(RUST_SRC, "RUST").language == "rust"


def test_parse_unknown_language_raises() -> None:
    with pytest.raises(bca.UnsupportedLanguageError):
        bca.Ast.parse(RUST_SRC, "klingon")


def test_parse_rejects_non_buffer_code() -> None:
    with pytest.raises((TypeError, ValueError)):
        bca.Ast.parse(123, "rust")  # type: ignore[arg-type]


# ----- metrics parity ------------------------------------------------------


def test_metrics_matches_analyze_source() -> None:
    """``Ast.parse(...).metrics()`` is byte-for-byte ``analyze_source``."""
    ast = bca.Ast.parse(RUST_SRC, "rust")
    assert ast.metrics() == bca.analyze_source(RUST_SRC, "rust")


def test_metrics_selection_subsets_blocks() -> None:
    only_cognitive = bca.Ast.parse(RUST_SRC, "rust").metrics(metrics=["cognitive"])
    assert only_cognitive == bca.analyze_source(RUST_SRC, "rust", metrics=["cognitive"])
    assert "cognitive" in only_cognitive["metrics"]
    assert "halstead" not in only_cognitive["metrics"]


def test_metrics_reused_parse_is_stable() -> None:
    ast = bca.Ast.parse(RUST_SRC, "rust")
    # Two walks over the one parse agree (no per-call parse drift).
    assert ast.metrics() == ast.metrics()


def test_parse_name_lands_on_metrics_space() -> None:
    # The optional `name=` is recorded as the top-level FuncSpace name.
    metrics = bca.Ast.parse(RUST_SRC, "rust", name="logical.rs").metrics()
    assert metrics["name"] == "logical.rs"


def test_metrics_exclude_tests_matches_analyze_source() -> None:
    # A #[cfg(test)] module plus a real function: exclude_tests prunes the
    # test space, and the seam stays byte-for-byte with analyze_source under
    # both settings (#727).
    src = "#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { assert!(true); }\n}\nfn real() {}\n"
    ast = bca.Ast.parse(src, "rust")
    excluded = ast.metrics(exclude_tests=True)
    included = ast.metrics(exclude_tests=False)
    assert excluded == bca.analyze_source(src, "rust", exclude_tests=True)
    assert included == bca.analyze_source(src, "rust", exclude_tests=False)
    # The flag actually prunes a space, so the two results differ.
    assert len(excluded["spaces"]) < len(included["spaces"])


# ----- from_path -----------------------------------------------------------


def test_from_path_detects_language_and_matches_analyze() -> None:
    path = FIXTURES / "hello.rs"
    ast = bca.Ast.from_path(path)
    assert ast.language == "rust"
    # No-magic from_path reads through the same reader as analyze, so the
    # metrics agree for a plain source file (#727).
    assert ast.metrics() == bca.analyze(path)


def test_from_path_unknown_language_raises(tmp_path: Path) -> None:
    mystery = tmp_path / "mystery.zzz"
    mystery.write_text("some unrecognized content\n")
    with pytest.raises(bca.UnsupportedLanguageError):
        bca.Ast.from_path(mystery)


def test_from_path_missing_file_raises(tmp_path: Path) -> None:
    with pytest.raises(FileNotFoundError):
        bca.Ast.from_path(tmp_path / "does-not-exist.rs")


def test_from_path_binary_file_raises(tmp_path: Path) -> None:
    blob = tmp_path / "blob.rs"
    blob.write_bytes(b"\x00\x01\x02\xff\xfe garbage \x00 bytes here \x00\x00")
    with pytest.raises(ValueError, match="not valid UTF-8 source text"):
        bca.Ast.from_path(blob)


# ----- dump ----------------------------------------------------------------


def _leaves(node: AstNodeDict) -> list[AstNodeDict]:
    if not node["children"]:
        return [node]
    return [leaf for child in node["children"] for leaf in _leaves(child)]


def test_dump_shape() -> None:
    root = bca.Ast.parse(RUST_SRC, "rust").dump()
    assert root is not None
    assert set(root) == {"type", "value", "span", "field_name", "children"}
    assert isinstance(root["children"], list)


def test_dump_span_byte_offsets_slice_source() -> None:
    """Every node's ``[start_byte, end_byte)`` slices ``source`` back to its
    text — the structural-consumer payoff of #727."""
    ast = bca.Ast.parse(RUST_SRC, "rust")
    source = ast.source
    root = ast.dump()
    assert root is not None
    span = root["span"]
    assert span is not None
    assert set(span) == {
        "start_line",
        "start_col",
        "end_line",
        "end_col",
        "start_byte",
        "end_byte",
    }
    # The root starts at byte 0 and never runs past the source.
    assert span["start_byte"] == 0
    assert 0 < span["end_byte"] <= len(source)
    # Each leaf's byte range recovers exactly the leaf's text.
    for leaf in _leaves(root):
        leaf_span = leaf["span"]
        assert leaf_span is not None
        assert source[leaf_span["start_byte"] : leaf_span["end_byte"]].decode() == leaf["value"]


def test_dump_span_false_omits_span() -> None:
    root = bca.Ast.parse(RUST_SRC, "rust").dump(span=False)
    assert root is not None
    assert root["span"] is None


def _node_types(node: AstNodeDict) -> list[str]:
    return [node["type"], *(t for child in node["children"] for t in _node_types(child))]


def test_dump_comment_flag_controls_comment_nodes() -> None:
    # Pins the (counterintuitive) `comment` polarity matching the CLI / /ast
    # convention: comment=False (default) KEEPS comment nodes, comment=True
    # omits them. A regression that inverted the flag would flip both lists.
    src = "// a standalone comment\nfn f() {}\n"
    ast = bca.Ast.parse(src, "rust")
    kept = ast.dump(comment=False)
    omitted = ast.dump(comment=True)
    assert kept is not None
    assert omitted is not None
    assert any("comment" in t for t in _node_types(kept))
    assert not any("comment" in t for t in _node_types(omitted))


# ----- functions / ops / count --------------------------------------------


def test_functions_lists_spans() -> None:
    ast = bca.Ast.parse("fn a() {}\nfn b() {}\n", "rust")
    funcs = ast.functions()
    names = {f["name"] for f in funcs}
    assert {"a", "b"} <= names
    for f in funcs:
        assert set(f) == {"name", "start_line", "end_line"}
        assert f["start_line"] <= f["end_line"]


def test_ops_returns_operator_operand_tree() -> None:
    ops = bca.Ast.parse(RUST_SRC, "rust").ops()
    assert set(ops) >= {"name", "start_line", "end_line", "kind", "spaces", "operands", "operators"}
    # `fn main() { let x = 1; }` has real Halstead operators (`fn`, `let`,
    # `=`, ...) and operands (`main`, `x`, `1`), plus the nested `main`
    # function space. Empty lists here would mean ops() returned a hollow
    # tree — the bug the non-empty assertions guard against.
    assert ops["operators"], "expected non-empty operators"
    assert ops["operands"], "expected non-empty operands"
    assert any(space["name"] == "main" for space in ops["spaces"])


def test_seam_dispatches_non_rust_language() -> None:
    # Every seam test above uses Rust; this exercises the language-dispatch
    # path with Python so a grammar-routing bug can't hide behind Rust-only
    # coverage (#727).
    ast = bca.Ast.parse("def greet(name):\n    return name\n", "python")
    assert ast.language == "python"
    root = ast.dump()
    assert root is not None
    assert root["type"] == "module"
    assert "greet" in {f["name"] for f in ast.functions()}


def test_count_returns_matching_and_total() -> None:
    matching, total = bca.Ast.parse(RUST_SRC, "rust").count(["identifier"])
    assert isinstance(matching, int)
    assert isinstance(total, int)
    # `fn main() { let x = 1; }` has identifier nodes (`main`, `x`), so a
    # working filter matches some — but not all — of the tree's nodes.
    # `0 < matching < total` fails both ways count could break: matching
    # nothing (filter ignored, → 0) or matching everything (filter not
    # applied, → total).
    assert 0 < matching < total


# ----- strip_comments / suppressions --------------------------------------


def test_strip_comments_drops_comment_text() -> None:
    ast = bca.Ast.parse("// gone\nfn f() {}\n", "rust")
    stripped = ast.strip_comments()
    assert stripped is not None
    assert b"gone" not in stripped
    assert b"fn f" in stripped


def test_suppressions_reports_markers() -> None:
    src = "fn f() {\n    // bca: suppress(cognitive)\n    let x = 1;\n}\n"
    markers = bca.Ast.parse(src, "rust").suppressions()
    assert len(markers) == 1
    marker = markers[0]
    assert set(marker) >= {"line", "target", "scope", "dialect", "function"}
    assert marker["target"] == "function"
    assert marker["function"] == "f"


# ----- language_grammar_version --------------------------------------------


@pytest.mark.parametrize("language", ["rust", "bash", "python", "typescript"])
def test_language_grammar_version_matches_pin(language: str) -> None:
    dep_key = {
        "rust": "tree-sitter-rust",
        "bash": "tree-sitter-bash",
        "python": "tree-sitter-python",
        "typescript": "tree-sitter-typescript",
    }[language]
    assert bca.language_grammar_version(language) == _grammar_pin(dep_key)


def test_language_grammar_version_unknown_raises() -> None:
    with pytest.raises(bca.UnsupportedLanguageError):
        bca.language_grammar_version("klingon")


# ----- repr ----------------------------------------------------------------


def test_repr_is_informative() -> None:
    text = repr(bca.Ast.parse(RUST_SRC, "rust"))
    assert "Ast(" in text
    assert "rust" in text
