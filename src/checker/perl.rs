//! `Checker` implementation for Perl.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for PerlCode {
    fn is_comment(node: &Node) -> bool {
        matches!(node.kind_id().into(), Perl::Comments | Perl::PodStatement)
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Perl::SourceFile
                | Perl::FunctionDefinition
                | Perl::FunctionDefinitionWithoutSub
                | Perl::AnonymousFunction
        )
    }

    fn is_func(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Perl::FunctionDefinition | Perl::FunctionDefinitionWithoutSub
        )
    }

    fn is_closure(node: &Node) -> bool {
        node.kind_id() == Perl::AnonymousFunction
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

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Perl::LPAREN | Perl::COMMA | Perl::RPAREN | Perl::FatComma
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
