//! `Checker` implementation for Groovy.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for GroovyCode {
    fn is_comment(node: &Node) -> bool {
        // `groovydoc_comment` is Groovy's `/** … */` Groovydoc kind, a
        // distinct grammar node (Groovy 150) from the `//` LineComment
        // and `/* */` BlockComment. The `Loc` cloc arm already counts it
        // (loc.rs); this restores parity so tokens/find agree too (#697).
        matches!(
            node.kind_id().into(),
            Groovy::LineComment | Groovy::BlockComment | Groovy::GroovydocComment
        )
    }

    // Mirrors `impl Checker for JavaCode` exactly: `EnumDeclaration`,
    // `RecordDeclaration`, and `AnnotationTypeDeclaration` open class
    // spaces so `Npa`/`Npm`/`Wmc` walk their bodies. Cross-language parity
    // with Java is the point — identical-shape sources must agree on
    // class-shaped metrics (issue #280).
    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Groovy::SourceFile
                | Groovy::ClassDeclaration
                | Groovy::TraitDeclaration
                | Groovy::InterfaceDeclaration
                | Groovy::EnumDeclaration
                | Groovy::RecordDeclaration
                | Groovy::AnnotationTypeDeclaration
        )
    }

    fn is_func<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        matches!(
            node.kind_id().into(),
            Groovy::MethodDeclaration | Groovy::ConstructorDeclaration
        )
    }

    fn is_closure<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        matches!(node.kind_id().into(), Groovy::Closure)
    }

    // `command_chain` is the new grammar's distinct node for Groovy's
    // command-style juxtaposed calls (`foo bar baz`) which the prior
    // amaanq grammar mis-modelled as `juxt_function_call`; it is a
    // genuine method-call form and stays in `is_call`.
    //
    // Intentionally excludes `ObjectCreationExpression` (`new Foo()`):
    // `is_call` follows the Java-family convention (Java's `is_call` =
    // `MethodInvocation`, C#'s = `InvocationExpression`) of counting
    // method/function call sites only. Constructor invocations are an
    // ABC concern — Groovy's ABC `branches` counts `New` separately
    // (see `groovy_count_token_branch` in metrics/abc.rs).
    fn is_call(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Groovy::MethodInvocation | Groovy::CommandChain
        )
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Groovy::LPAREN | Groovy::COMMA | Groovy::RPAREN
        )
    }

    impl_simple_is_string!(Groovy, StringLiteral);

    // The dekobon Groovy grammar models `if_statement` with the `else`
    // keyword token emitted inline followed by the inner `if_statement`
    // sibling — same shape as the prior amaanq grammar and Java.
    impl_is_else_if_prev_sibling!(Groovy, IfStatement, Else);
}
