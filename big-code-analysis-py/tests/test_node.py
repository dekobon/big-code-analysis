"""Tests for the lazy ``Node`` traversal surface (issue #728).

Exercise the ``big_code_analysis.Node`` handle reached from
``Ast.root_node`` / ``Ast.find``: py-tree-sitter-style traversal (``kind``,
byte offsets, points, ``children``, ``walk()``, ``descendants_by_kind``)
over the *raw* tree, without materialising it into dicts. The memory-safety
contract behind the handle (it keeps its ``Ast`` alive across a GC, and is
safe to share across threads) is pinned here too — that is the load-bearing
property of the lifetime-erased design.
"""

from __future__ import annotations

import collections.abc
import gc
from collections.abc import Callable
from concurrent.futures import ThreadPoolExecutor

import big_code_analysis as bca
import pytest
from big_code_analysis import AstNodeDict, Node

RUST_SRC = "fn main() {\n    let x = 1 + 2;\n    foo(x);\n}\n"


def _preorder_kinds(node: Node) -> list[str]:
    """Ground-truth pre-order kinds via the ``children`` accessor, to check
    ``walk()`` against an independent traversal."""
    return [node.kind, *(k for child in node.children for k in _preorder_kinds(child))]


def _collect(node: AstNodeDict, pred: Callable[[str], bool]) -> list[AstNodeDict]:
    hits = [node] if pred(node["type"]) else []
    return hits + [h for child in node["children"] for h in _collect(child, pred)]


# ----- root / basics -------------------------------------------------------


def test_root_node_is_the_grammar_root() -> None:
    root = bca.Ast.parse(RUST_SRC, "rust").root_node
    assert root.kind == "source_file"
    assert isinstance(root.kind_id, int)
    assert root.is_named
    assert not root.has_error
    assert root.parent is None
    assert root.child_count >= 1
    assert len(root.children) == root.child_count


def test_repr_is_informative() -> None:
    text = repr(bca.Ast.parse(RUST_SRC, "rust").root_node)
    assert "Node(" in text
    assert "source_file" in text


# ----- coordinate systems --------------------------------------------------


def test_points_are_zero_based_lines_are_one_based() -> None:
    ast = bca.Ast.parse(RUST_SRC, "rust")
    root = ast.root_node
    assert root.start_point == (0, 0)
    # The documented relationship between the two vocabularies.
    assert root.start_line == root.start_point[0] + 1
    assert root.end_line == root.end_point[0] + 1
    # Check the relationship on a node off row 0, so a regression that
    # returned the raw 0-based row for `start_line` (which would still
    # satisfy `1 == 0 + 1` at the root) is caught. `foo(x)` sits on the
    # third line — 0-based row 2.
    call = ast.find(["call_expression"])[0]
    assert call.start_point == (2, 4)
    assert call.start_line == 3
    assert call.start_line == call.start_point[0] + 1


def test_span_matches_dump_one_based_shape() -> None:
    ast = bca.Ast.parse(RUST_SRC, "rust")
    span = ast.root_node.span
    assert set(span) == {
        "start_line",
        "start_col",
        "end_line",
        "end_col",
        "start_byte",
        "end_byte",
    }
    # The Node's span is byte-for-byte the dict dump() emits for the root.
    dumped = ast.dump()
    assert dumped is not None
    assert dumped["span"] == span


def test_byte_offsets_and_text_round_trip_source() -> None:
    ast = bca.Ast.parse(RUST_SRC, "rust")
    source = ast.source
    for node in ast.root_node.walk():
        # Every node's text is exactly its byte slice of the source.
        assert node.text() == source[node.start_byte : node.end_byte]


# ----- raw vs. altered taxonomy (the documented divergence) ----------------


def test_raw_kind_retains_children_that_dump_flattens() -> None:
    # The Alterator flattens string-literal nodes in dump() (strips their
    # grammar children); the raw lazy tree keeps them. This is the
    # intentional, documented divergence between Node.kind and dump() types.
    src = 'fn f() {\n    let s = "hello";\n}\n'
    ast = bca.Ast.parse(src, "rust")

    raw_strings = [n for n in ast.root_node.walk() if "string" in n.kind]
    assert raw_strings, "expected a raw string node in the unaltered tree"
    assert any(n.children for n in raw_strings), (
        "raw string node must retain its grammar children (quotes, content)"
    )

    root = ast.dump()
    assert root is not None
    dump_strings = _collect(root, lambda t: "string" in t)
    assert dump_strings, "expected a string node in the dumped tree"
    assert all(not d["children"] for d in dump_strings), (
        "dump() flattens string nodes to childless leaves"
    )


# ----- navigation ----------------------------------------------------------


def test_children_named_children_and_indexing_agree() -> None:
    root = bca.Ast.parse(RUST_SRC, "rust").root_node
    fn = root.children[0]
    assert fn.kind == "function_item"
    # named_children is the is_named subset of children, in the same order.
    assert [c.kind for c in fn.named_children] == [c.kind for c in fn.children if c.is_named]
    assert fn.named_child_count == len(fn.named_children)
    # Positional access matches the children list.
    first = fn.child(0)
    assert first is not None
    assert first.kind == fn.children[0].kind
    assert fn.child(fn.child_count) is None  # out of range
    assert fn.named_child(fn.named_child_count) is None


def test_parent_and_sibling_links() -> None:
    root = bca.Ast.parse(RUST_SRC, "rust").root_node
    fn = root.children[0]
    # parent climbs back to the root.
    assert fn.parent == root
    assert fn.parent is not None
    body = fn.child_by_field_name("body")
    assert body is not None
    stmts = body.named_children
    assert len(stmts) >= 2
    # Sibling links chain the statements together.
    assert stmts[0].next_named_sibling == stmts[1]
    assert stmts[1].prev_named_sibling == stmts[0]
    assert stmts[0].prev_sibling is not None  # the opening `{`


def test_field_name_and_child_by_field_name() -> None:
    fn = bca.Ast.parse(RUST_SRC, "rust").root_node.children[0]
    name = fn.child_by_field_name("name")
    assert name is not None
    assert name.kind == "identifier"
    assert name.text() == b"main"
    # The reached child knows the field it was reached through.
    assert name.field_name == "name"
    # And the parent can name the field for that child index.
    idx = next(i for i, c in enumerate(fn.children) if c == name)
    assert fn.field_name_for_child(idx) == "name"
    # An absent field yields None, not an error.
    assert fn.child_by_field_name("no_such_field") is None
    # The root has no parent, hence no field name.
    assert fn.parent is not None
    assert fn.parent.field_name is None


def test_children_by_field_name_returns_every_match() -> None:
    # A call's `arguments` field; assert children_by_field_name finds it.
    ast = bca.Ast.parse("fn f() { g(1, 2); }", "rust")
    calls = ast.find(["call_expression"])
    assert calls
    args = calls[0].children_by_field_name("arguments")
    assert [a.kind for a in args] == ["arguments"]


# ----- error / missing nodes ----------------------------------------------


def test_error_recovery_nodes_are_flagged() -> None:
    # A clearly broken function body triggers parser error recovery.
    root = bca.Ast.parse("fn f( {", "rust").root_node
    assert root.has_error
    flagged = [n for n in root.walk() if n.is_error or n.is_missing]
    assert flagged, "expected at least one ERROR/MISSING node"


# ----- walk ----------------------------------------------------------------


def test_walk_is_a_lazy_preorder_iterator() -> None:
    root = bca.Ast.parse(RUST_SRC, "rust").root_node
    it = root.walk()
    assert isinstance(it, collections.abc.Iterator)
    # Lazy: the first node is available without draining the iterator.
    assert next(it).kind == "source_file"


def test_walk_matches_recursive_children_order() -> None:
    root = bca.Ast.parse(RUST_SRC, "rust").root_node
    assert [n.kind for n in root.walk()] == _preorder_kinds(root)


# ----- descendants_by_kind / find -----------------------------------------


def test_descendants_by_kind_matches_find_for_exact_kind() -> None:
    ast = bca.Ast.parse(RUST_SRC, "rust")
    # `main`, the `x` binding, the two `x` uses (+ `foo`) are identifiers.
    by_descendants = ast.root_node.descendants_by_kind(["identifier"])
    by_find = ast.find(["identifier"])
    assert [n.text() for n in by_descendants] == [b"main", b"x", b"foo", b"x"]
    # The whole-tree find and the root-subtree walk see the same set.
    assert len(by_descendants) == len(by_find)


def test_descendants_by_kind_unions_and_empties() -> None:
    root = bca.Ast.parse(RUST_SRC, "rust").root_node
    idents = root.descendants_by_kind(["identifier"])
    assert root.descendants_by_kind(["no_such_kind"]) == []
    widened = root.descendants_by_kind(["identifier", "integer_literal"])
    assert len(widened) > len(idents)


def test_find_accepts_the_count_filter_vocabulary() -> None:
    ast = bca.Ast.parse(RUST_SRC, "rust")
    # `function` is a semantic filter from the `bca count` vocabulary.
    funcs = ast.find(["function"])
    assert funcs
    calls = ast.find(["call"])
    assert any(n.text().startswith(b"foo") for n in calls)


# ----- identity / equality / hash -----------------------------------------


def test_identity_within_one_ast() -> None:
    root = bca.Ast.parse(RUST_SRC, "rust").root_node
    a = root.children[0]
    b = root.children[0]
    # Distinct Python objects, but equal and hash-consistent.
    assert a is not b
    assert a == b
    assert hash(a) == hash(b)
    # Different nodes of the same tree are unequal.
    assert a != root
    # A Node is usable as a dict/set key.
    assert len({a, b, root}) == 2


def test_nodes_from_distinct_asts_are_unequal() -> None:
    # Same source, two parses → two Ast objects → unequal nodes even though
    # the node ids may coincide (identity is keyed on the Ast object too).
    a = bca.Ast.parse(RUST_SRC, "rust").root_node
    b = bca.Ast.parse(RUST_SRC, "rust").root_node
    assert a != b


def test_eq_with_non_node_is_false_not_error() -> None:
    root = bca.Ast.parse(RUST_SRC, "rust").root_node
    assert root != "not a node"
    assert root != 42


# ----- language dispatch ---------------------------------------------------


def test_traversal_dispatches_non_rust_language() -> None:
    ast = bca.Ast.parse("def greet(name):\n    return name\n", "python")
    root = ast.root_node
    assert root.kind == "module"
    funcs = ast.find(["function_definition"])
    assert len(funcs) == 1
    name = funcs[0].child_by_field_name("name")
    assert name is not None
    assert name.text() == b"greet"


# ----- memory safety: keep-alive + threading -------------------------------


def test_node_keeps_ast_alive_after_gc() -> None:
    # The ONLY strong reference to the parsed Ast is the one the Node holds
    # internally (the Ast was an unnamed temporary). If the lifetime-erased
    # node did not keep its Ast alive, this would be a use-after-free; the
    # GC pass makes any premature collection observable.
    node = bca.Ast.parse(RUST_SRC, "rust").root_node
    gc.collect()
    assert node.kind == "source_file"
    # text() reads the retained source bytes — they must still be alive.
    idents = node.descendants_by_kind(["identifier"])
    assert [n.text() for n in idents] == [b"main", b"x", b"foo", b"x"]


def test_deep_child_outlives_dropped_root() -> None:
    # A child derived deep in the tree carries its own keep-alive, so it
    # stays valid after every other handle to the parse is dropped.
    ast = bca.Ast.parse(RUST_SRC, "rust")
    child = ast.find(["identifier"])[0]
    del ast
    gc.collect()
    assert child.kind == "identifier"
    assert child.text() == b"main"
    # Navigation back up the tree still resolves.
    assert child.parent is not None
    assert child.parent.kind == "function_item"


def test_nodes_are_safe_to_share_across_threads() -> None:
    # tree_sitter Tree/Node are Send+Sync, so the frozen pyclass fans out
    # under a thread pool exactly like Ast itself. Each worker walks a
    # shared node and must agree on the kind sequence.
    root = bca.Ast.parse(RUST_SRC, "rust").root_node
    expected = [n.kind for n in root.walk()]

    def walk_kinds(_: int) -> list[str]:
        return [n.kind for n in root.walk()]

    with ThreadPoolExecutor(max_workers=4) as pool:
        results = list(pool.map(walk_kinds, range(16)))
    assert all(r == expected for r in results)


# ----- node is not constructible from Python -------------------------------


def test_node_has_no_public_constructor() -> None:
    # Nodes only come from an Ast; the pyclass exposes no constructor.
    with pytest.raises(TypeError):
        Node()
