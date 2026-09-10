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
                // Property accessors and `init { … }` carry executable
                // code but were referenced nowhere outside the generated
                // enum, so they opened no space at all: their control
                // flow was charged to the enclosing class and `bca check`
                // could never flag one however complex it got (#1184).
                //
                // Note this does *not* change `nom.functions`, which
                // keys on `is_func` — an accessor-only file still
                // reports 0 there, deliberately, for the reason below.
                // The defect fixed here is the missing metric scope.
                //
                // `is_func_space` and not `is_func`: none of the three is
                // a callable a user names at a call site — in Kotlin you
                // write `p.foo`, not `p.getFoo()` — and putting an
                // accessor in `is_func` would have `npm` count the same
                // property once as an attribute and again as a method,
                // skewing the NPA/NPM ratio the OOP metrics exist to
                // report. This is the same split the JS family already
                // uses for `FunctionExpression`.
                | Kotlin::Getter
                | Kotlin::Setter
                | Kotlin::AnonymousInitializer
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
