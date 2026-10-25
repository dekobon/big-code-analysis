//! `Checker` implementation for Perl.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::lang_helpers::doc_comment::{node_bytes, pod_comment_region};
use crate::lang_helpers::perl::perl_bareword_is_call;

impl Checker for PerlCode {
    fn is_comment(node: &Node) -> bool {
        matches!(node.kind_id().into(), Perl::Comments | Perl::PodStatement)
    }

    // perldoc extracts POD wherever it sits, a sub body included.
    const DOC_COMMENTS_IN_FUNCTION_BODIES: bool = true;

    fn is_doc_comment(node: &Node, code: &[u8]) -> bool {
        // POD is Perl's documentation syntax; a `#` comment never is, and
        // neither is a POD block opening a `comment` region, Perl's idiom
        // for commenting code out.
        node.kind_id() == Perl::PodStatement && !pod_comment_region(node_bytes(node, code))
    }

    // The space tree is declared in `roles/perl.rs` (#1696): the
    // `FuncOpener` and `Container` rows are what the walk promotes and
    // `opens_space` labels. The rows read no bytes and no chain, so the
    // byte-less spelling forwards with none and answers the same.
    fn is_func_space(node: &Node) -> bool {
        Self::is_func_space_with_code(node, &[], Ancestors::unknown())
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

    fn is_call(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Perl::CallExpressionWithSpacedArgs
                | Perl::CallExpressionWithSub
                | Perl::CallExpressionWithArgsWithBrackets
                | Perl::CallExpressionWithVariable
                | Perl::CallExpressionWithBareword
                | Perl::CallExpressionRecursive
                | Perl::MethodInvocation
        )
    }

    fn is_call_with_code<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
        Self::is_call(node)
            && (node.kind_id() != Perl::CallExpressionWithBareword as u16
                || perl_bareword_is_call(node, ancestors))
    }

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

    // `HeredocBodyStatement` wraps the heredoc body text (and any
    // `Interpolation` children) that appears as a top-level
    // statement after the heredoc-introducing `<<TAG`; it is the
    // visible literal node and is treated as a string here, the
    // same way Bash's `heredoc_body` is treated as a string.
    impl_simple_is_string!(
        Perl,
        StringSingleQuoted,
        StringDoubleQuoted,
        StringQQuoted,
        StringQqQuoted,
        BacktickQuoted,
        CommandQxQuoted,
        HeredocBodyStatement,
    );

    // tree-sitter-perl emits `elsif_clause` as a direct child of the
    // surrounding `if_statement` (not as a wrapper around a nested
    // `if`), so the clause node itself is the else-if.
    impl_is_else_if_clause!(Perl, ElsifClause);
}
