//! `Checker` implementation for Tcl.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for TclCode {
    // The one `Comment` row of `roles/tcl.rs`; no `DocComment` row (#1699).
    impl_comment_from_rows!(tcl);

    // The space tree is declared in `roles/tcl.rs` (#1696): the
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
        crate::roles::tcl::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::tcl::func_opener::is_callable(node, &[], ancestors)
    }

    // Tcl closures (`apply`) are ordinary commands; the grammar has no distinct closure node.
    fn is_closure<'a>(_: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        false
    }

    // `Call` and `StringLit` are the rows of `roles/tcl.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(tcl, call, is_call, is_call_with_code);
    impl_row_predicates!(tcl, string_lit, is_string, is_string_with_code);

    // Tcl arguments are whitespace-separated; no punctuation to exclude.
    fn is_non_arg(_: &Node) -> bool {
        false
    }

    // The `ElseIf` row of `roles/tcl.rs`: the dedicated `elseif` node
    // itself is the else-if, never a nested `if`. `Elseif2` is the
    // `elseif` keyword token, not the clause, and the row leaves it out
    // (#1697).
    impl_else_if_from_row!(tcl);
}
