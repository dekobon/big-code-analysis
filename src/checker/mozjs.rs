//! `Checker` implementation for Mozilla JavaScript.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for MozjsCode {
    fn is_comment(node: &Node) -> bool {
        // `html_comment` is the ECMAScript Annex-B `<!-- -->` form, a
        // distinct named+extra grammar node (Mozjs 133 / JS 132 / TS 162
        // / Tsx 168) alongside the `//` and `/* */` `comment` kind. It
        // must classify as a comment for loc/tokens/find parity (#697).
        matches!(node.kind_id().into(), Mozjs::Comment | Mozjs::HtmlComment)
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Mozjs::Program
                | Mozjs::FunctionExpression
                | Mozjs::Class
                | Mozjs::GeneratorFunction
                | Mozjs::FunctionDeclaration
                | Mozjs::MethodDefinition
                | Mozjs::GeneratorFunctionDeclaration
                | Mozjs::ClassDeclaration
                | Mozjs::ArrowFunction
                // ES2022 `static { … }`. Referenced nowhere
                // outside the generated enum before #1184, so a
                // class static block opened no space and its
                // control flow was charged to the class.
                | Mozjs::ClassStaticBlock
        )
    }

    is_js_func_and_closure_checker!(Mozjs, FieldDefinition);

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Mozjs::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Mozjs::LPAREN | Mozjs::COMMA | Mozjs::RPAREN
        )
    }

    impl_js_family_is_string!(Mozjs);

    impl_is_else_if_parent_clause!(Mozjs, IfStatement, ElseClause);
}
