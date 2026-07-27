//! `Checker` implementation for Go.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for GoCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Go::Comment
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Go::SourceFile | Go::FunctionDeclaration | Go::MethodDeclaration | Go::FuncLiteral
        )
    }

    fn is_func(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Go::FunctionDeclaration | Go::MethodDeclaration
        )
    }

    fn is_closure(node: &Node) -> bool {
        node.kind_id() == Go::FuncLiteral
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Go::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(node.kind_id().into(), Go::LPAREN | Go::COMMA | Go::RPAREN)
    }

    impl_simple_is_string!(Go, InterpretedStringLiteral, RawStringLiteral);

    #[inline]
    fn is_else_if<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        node.kind_id() == Go::IfStatement
            && ancestors
                .parent(node)
                .is_some_and(|p| p.kind_id() == Go::IfStatement)
    }
}
