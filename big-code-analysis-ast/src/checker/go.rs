//! `Checker` implementation for Go.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for GoCode {
    // The `Comment`, `DocTarget` and `DocRunDirective` rows of
    // `roles/go.rs` (#1699): go/doc's targets under the function-body
    // scope gate, read with the node's own chain; a directive by its
    // bytes.
    impl_comment_from_rows!(go);

    impl_doc_target_from_row!(go);

    impl_doc_run_directive_from_row!(go);

    // The space tree is declared in `roles/go.rs` (#1696): the
    // `FuncOpener` and `Container` rows are what the walk promotes and
    // `opens_space` labels. The rows read no bytes and no chain, so the
    // byte-less spelling forwards with none and answers the same.
    fn is_func_space(node: &Node) -> bool {
        Self::is_func_space_with_code(node, &[], Ancestors::known(&[]))
    }

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::go::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::go::func_opener::is_callable(node, &[], ancestors)
    }

    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::go::closure::owns(node, &[], ancestors)
    }

    // `Call` and `StringLit` are the rows of `roles/go.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(go, call, is_call, is_call_with_code);
    impl_row_predicates!(go, string_lit, is_string, is_string_with_code);

    fn is_non_arg(node: &Node) -> bool {
        matches!(node.kind_id().into(), Go::LPAREN | Go::COMMA | Go::RPAREN)
    }

    // The `ElseIf` row of `roles/go.rs`: an `if_statement` whose parent
    // is the outer `if_statement` (the grammar has no else clause node),
    // read off the caller's chain. The row reads no bytes (#1697).
    impl_else_if_from_row!(go);
}
