//! `Checker` implementation for Kotlin.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for KotlinCode {
    fn is_comment(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Kotlin::LineComment | Kotlin::BlockComment
        )
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Kotlin::SourceFile
                | Kotlin::ClassDeclaration
                | Kotlin::ObjectDeclaration
                | Kotlin::CompanionObject
                | Kotlin::ObjectLiteral
        )
    }

    fn is_func<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        matches!(
            node.kind_id().into(),
            Kotlin::FunctionDeclaration | Kotlin::SecondaryConstructor
        )
    }

    fn is_closure<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        matches!(
            node.kind_id().into(),
            Kotlin::LambdaLiteral | Kotlin::AnonymousFunction
        )
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Kotlin::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Kotlin::LPAREN | Kotlin::COMMA | Kotlin::RPAREN
        )
    }

    impl_simple_is_string!(Kotlin, StringLiteral, MultilineStringLiteral);

    // tree-sitter-kotlin models `else if` as an `else` keyword sibling
    // followed by an `if_expression` (not an `if_statement`, and no
    // wrapping clause node).
    impl_is_else_if_prev_sibling!(Kotlin, IfExpression, Else);
}
