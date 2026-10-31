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
    impl_row_predicates!(python, call, is_call, is_call_with_code);
    impl_row_predicates!(python, string_lit, is_string, is_string_with_code);

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

    // The `ElseIf` row of `roles/python.rs`, intersected with `Branch`:
    // the row owns both the `elif_clause` and the chained `if` (the sole
    // statement of an `else:` block, #276), and this predicate has only
    // ever answered for the `if` — the clause is told apart by every
    // consumer on its own kind. The row's `sole_statement_of_else_block`
    // gate is the `else_clause → block → if_statement` walk that lived
    // here, blocks' two aliases included (#419). The rows read no bytes,
    // so the empty code is exact (#1697).
    fn is_else_if<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        // The same pin `impl_else_if_from_row!` carries: the empty slice
        // is exact only while neither row reads bytes.
        const _: () = assert!(
            !crate::roles::python::branch::TEXT_GATED && !crate::roles::python::else_if::TEXT_GATED,
            "a Python `Branch` / `ElseIf` row reads bytes; `is_else_if` cannot forward with `&[]`"
        );
        let kind = node.kind_id();
        crate::roles::python::branch::owns_kind(kind, node, &[], ancestors)
            && crate::roles::python::else_if::owns_kind(kind, node, &[], ancestors)
    }
}
