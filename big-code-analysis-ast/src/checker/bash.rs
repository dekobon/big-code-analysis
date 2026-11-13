//! `Checker` implementation for Bash.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for BashCode {
    // The one `Comment` row of `roles/bash.rs`; no `DocComment` row (#1699).
    impl_comment_from_rows!(bash);

    // The space tree is declared in `roles/bash.rs` (#1696): the
    // `FuncOpener` and `Container` rows are what the walk promotes and
    // `opens_space` labels. The rows read no bytes and no chain, so the
    // byte-less spelling forwards with none and answers the same.
    impl_byte_less_func_space!(bash);

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::bash::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func_with_code<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::bash::func_opener::is_callable(node, code, ancestors)
    }

    // `Call` and `StringLit` are the rows of `roles/bash.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(bash, call, is_call, is_call_with_code);
    impl_row_predicates!(bash, string_lit, is_string, is_string_with_code);

    // No `is_non_arg`: a Bash function declares no parameters, and
    // tree-sitter-bash 0.25.1 has no `parameters` field for the default
    // `NArgs::compute` to find, so the trait's negative filter is never
    // consulted and the default `false` stands.

    // The `ElseIf` row of `roles/bash.rs`: the dedicated `elif_clause`
    // itself is the else-if (#1697).
    impl_else_if_from_row!(bash);
}
