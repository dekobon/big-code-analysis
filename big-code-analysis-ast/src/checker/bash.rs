//! `Checker` implementation for Bash.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for BashCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Bash::Comment
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Bash::Program | Bash::FunctionDefinition
        )
    }

    fn is_func<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        node.kind_id() == Bash::FunctionDefinition
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Bash::Command
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Bash::LPAREN | Bash::RPAREN | Bash::COMMA | Bash::SEMI
        )
    }

    // tree-sitter-bash 0.25.1 only emits the `heredoc_body`
    // parser-node symbol (`HeredocBody2`) in observed parse trees;
    // the duplicate `HeredocBody` entry plus the hidden
    // `_heredoc_body` (`HeredocBody3`) and `_simple_heredoc_body`
    // (`SimpleHeredocBody`) rules do not surface, so they are
    // intentionally omitted here.
    impl_simple_is_string!(
        Bash,
        String,
        RawString,
        AnsiCString,
        TranslatedString,
        HeredocBody2,
    );

    // tree-sitter-bash exposes a dedicated `elif_clause` node, so the
    // clause node itself is the else-if (no nested `if` to inspect).
    impl_is_else_if_clause!(Bash, ElifClause);
}
