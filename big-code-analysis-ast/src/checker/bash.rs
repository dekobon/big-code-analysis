//! `Checker` implementation for Bash.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for BashCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Bash::Comment
    }

    // The space tree is declared in `roles/bash.rs` (#1696): the
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
        crate::roles::bash::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::bash::func_opener::is_callable(node, &[], ancestors)
    }

    // `Call` and `StringLit` are the rows of `roles/bash.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(bash, call, is_call, is_call_with_code);
    impl_row_predicates!(bash, string_lit, is_string, is_string_with_code);

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Bash::LPAREN | Bash::RPAREN | Bash::COMMA | Bash::SEMI
        )
    }

    // The `ElseIf` row of `roles/bash.rs`: the dedicated `elif_clause`
    // itself is the else-if (#1697).
    impl_else_if_from_row!(bash);
}
