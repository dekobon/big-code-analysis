//! `Checker` implementation for C#.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for CsharpCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Csharp::Comment
    }

    // A bodied indexer (`this[int i] { get; set; }`) or property
    // (`int X { get; set; }`) defers to its `accessor_declaration` children
    // for its function spaces — counting it here too double-counts (property,
    // #472) or triple-counts (indexer, #464) the member in nom/wmc. Only the
    // accessor-less expression-bodied form (`this[int i] => _d[i];` /
    // `int W => _w;`) opens a space directly, matching the npm `.max(1)`
    // fallback and the way the implicit getter is the sole callable.
    fn is_func_space(node: &Node) -> bool {
        if matches!(
            node.kind_id().into(),
            Csharp::IndexerDeclaration | Csharp::PropertyDeclaration
        ) {
            return !csharp_member_has_accessors(node);
        }
        matches!(
            node.kind_id().into(),
            Csharp::CompilationUnit
                | Csharp::ClassDeclaration
                | Csharp::StructDeclaration
                | Csharp::RecordDeclaration
                | Csharp::InterfaceDeclaration
                | Csharp::EnumDeclaration
                | Csharp::MethodDeclaration
                | Csharp::ConstructorDeclaration
                | Csharp::DestructorDeclaration
                | Csharp::LocalFunctionStatement
                | Csharp::LambdaExpression
                | Csharp::AnonymousMethodExpression
                | Csharp::AccessorDeclaration
                | Csharp::OperatorDeclaration
                | Csharp::ConversionOperatorDeclaration
        )
    }

    fn is_func<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        if matches!(
            node.kind_id().into(),
            Csharp::IndexerDeclaration | Csharp::PropertyDeclaration
        ) {
            return !csharp_member_has_accessors(node);
        }
        matches!(
            node.kind_id().into(),
            Csharp::MethodDeclaration
                | Csharp::ConstructorDeclaration
                | Csharp::DestructorDeclaration
                | Csharp::LocalFunctionStatement
                | Csharp::AccessorDeclaration
                | Csharp::OperatorDeclaration
                | Csharp::ConversionOperatorDeclaration
        )
    }

    fn is_closure<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        matches!(
            node.kind_id().into(),
            Csharp::LambdaExpression | Csharp::AnonymousMethodExpression
        )
    }

    fn is_call(node: &Node) -> bool {
        // The C# grammar emits three aliased `kind_id`s for
        // `invocation_expression`; matching only the unsuffixed variant
        // silently drops the rest (lesson #2 in lessons_learned.md).
        matches!(node.kind_id().into(), csharp_invocation_expr_kinds!())
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Csharp::LPAREN | Csharp::COMMA | Csharp::RPAREN
        )
    }

    impl_simple_is_string!(
        Csharp,
        StringLiteral,
        VerbatimStringLiteral,
        RawStringLiteral,
        InterpolatedStringExpression,
    );

    // tree-sitter-c-sharp models `else if` as an `Else` keyword token
    // followed by a nested `if_statement` (no wrapping `else_clause` node).
    impl_is_else_if_prev_sibling!(Csharp, IfStatement, Else);

    #[inline]
    fn is_primitive(node: &Node) -> bool {
        // Without this, every `PredefinedType` keyword (`int`, `string`,
        // `bool`, `object`, …) collapses into a single Halstead operator
        // because they share one `kind_id`. Returning `true` here routes
        // them through the lexeme-keyed `primitive_operators` map so
        // distinct keywords count as distinct operators (issue #286).
        node.kind_id() == Csharp::PredefinedType as u16
    }
}
