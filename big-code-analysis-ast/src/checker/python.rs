//! `Checker` implementation for Python.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for PythonCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Python::Comment
    }

    fn is_useful_comment<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
        // comment containing coding info are useful
        node.start_row() <= 1
            && RE
                .get_or_init(|| {
                    Regex::new(r"^[ \t\f]*#.*?coding[:=][ \t]*([-_.a-zA-Z0-9]+)")
                        .expect("constant Python coding-declaration regex always compiles")
                })
                .is_match(&code[node.start_byte()..node.end_byte()])
    }

    // The space tree is declared in `roles/python.rs` (#1696): a `def`
    // opens a `Function` space and is the callable, a class a `Class`;
    // a `lambda` is a closure and opens none (contract 2 of #1690). The
    // rows read no bytes and no chain, so the byte-less spelling
    // forwards with none and answers the same.
    fn is_func_space(node: &Node) -> bool {
        Self::is_func_space_with_code(node, &[], Ancestors::known(&[]))
    }

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::python::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::python::func_opener::is_callable(node, &[], ancestors)
    }

    // The `Closure` row lists `Lambda` (196, the concrete production
    // emitted today) and `Lambda2` (197, the currently-unseen hidden
    // alias) — the same set `python_is_lambda` accepts for the cognitive
    // lambda-scope walks — and not `Lambda3` (73, the `lambda` keyword
    // token). The drift-guard test below asserts `Lambda2` stays unseen
    // until a grammar bump promotes it (#419, #422; lesson 2).
    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::python::closure::owns(node, &[], ancestors)
    }

    // `Call` and `StringLit` are the rows of `roles/python.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_call_row!(python);
    impl_string_lit_row!(python);

    fn is_non_arg(node: &Node) -> bool {
        // tree-sitter-python emits the PEP 570 positional-only marker `/`
        // as a `positional_separator` node and the PEP 3102 keyword-only
        // marker `*` as a `keyword_separator` node, both as direct children
        // of the `parameters` list. They are punctuation, not parameters, so
        // they must be excluded or they inflate nargs by one each (issue #414).
        matches!(
            node.kind_id().into(),
            Python::LPAREN
                | Python::COMMA
                | Python::RPAREN
                | Python::PositionalSeparator
                | Python::KeywordSeparator
        )
    }

    // Python models `elif` as a dedicated `elif_clause` node, which is
    // handled directly by cognitive/cyclomatic dispatch as a branch
    // extension — `is_else_if` is intentionally never invoked for
    // `elif_clause` because it is not an `if_statement` and is not in
    // any of the structural kind sets that `count_specific_ancestors`
    // walks for nesting (issue #274).
    //
    // `else: if x: ...` chains also exist — semantically equivalent to
    // `else if` — but the grammar wraps the inner `if_statement` in a
    // `block` node, so the shape is `else_clause → block → if_statement`
    // rather than the direct `else_clause → if_statement` used by
    // C++/JS/TS/TSX/Rust. Match the chained shape by walking through
    // the `block` and requiring the inner `if` to be the block's sole
    // named child; sibling statements would mean a real nested-if, not
    // a chain (issue #276).
    //
    // `block` has two aliased kind_ids in tree-sitter-python
    // (`Block` = 135, `Block2` = 160 — both surface as `"block"`); we
    // accept either via `python_is_block`, the single normalization
    // point for the aliases (issue #419; lesson 2 in
    // `docs/development/lessons_learned.md`).
    fn is_else_if<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        node.kind_id() == Python::IfStatement
            && ancestors.iter(node).next().is_some_and(|(parent, above)| {
                crate::lang_helpers::python::python_is_block(&parent)
                    && parent.children().filter(Node::is_named).count() == 1
                    && above
                        .parent(&parent)
                        .is_some_and(|gp| gp.kind_id() == Python::ElseClause)
            })
    }
}
