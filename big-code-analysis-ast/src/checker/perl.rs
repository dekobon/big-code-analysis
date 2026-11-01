//! `Checker` implementation for Perl.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for PerlCode {
    // The `Comment` and `DocComment` rows of `roles/perl.rs` (#1699):
    // `pod_statement` sits on `DocComment` alone, so `is_comment`'s
    // union is what keeps a `=begin comment` region a comment.
    // perldoc extracts POD wherever it sits, a sub body included.
    impl_doc_comment_rows_in_bodies!(perl);

    // The space tree is declared in `roles/perl.rs` (#1696): the
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
        crate::roles::perl::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::perl::func_opener::is_callable(node, &[], ancestors)
    }

    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::perl::closure::owns(node, &[], ancestors)
    }

    // `Call` and `StringLit` are the rows of `roles/perl.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(perl, call, is_call, is_call_with_code);
    impl_row_predicates!(perl, string_lit, is_string, is_string_with_code);

    // `NormalComma` (369) is the separator inside a subroutine signature;
    // the bare `COMMA` (12) it wraps is that node's *child*, so listing
    // only `COMMA` let every signature separator count as a parameter
    // (#1147).
    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Perl::LPAREN | Perl::COMMA | Perl::RPAREN | Perl::FatComma | Perl::NormalComma
        )
    }

    // The `ElseIf` row of `roles/perl.rs`: the `elsif_clause` itself,
    // a direct child of the `if_statement`, is the else-if (#1697).
    impl_else_if_from_row!(perl);
}
