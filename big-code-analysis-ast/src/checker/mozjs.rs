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

    // JSDoc ignores `/***`; plain JavaScript follows it.
    impl_js_family_doc_comment!(Mozjs, javadoc_block);

    // JSDoc's `getLeadingJsdocComment` takes the nearest block comment
    // before a node, line comments filtered out, so a plain `/* */` in
    // between detaches a doc comment and a `//` does not. A virtual
    // comment (`@file`, `@module`, a tag that names a symbol) documents
    // that symbol and stands alone (#1613). The Babel leading comments it
    // reads hold every comment since the node before, so a doc comment
    // after code on its row documents the declaration below it.
    impl_js_family_doc_binding!(
        Mozjs,
        |node, code| Self::is_comment(node)
            && !crate::lang_helpers::doc_comment::node_bytes(node, code).starts_with(b"/*"),
        crate::lang_helpers::doc_comment::jsdoc_virtual,
        jsdoc_target,
        |_| true
    );

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

    // `String2` is MozJS's anonymous string-*literal* alias (same
    // production as JS's, e.g. `import "m"` module strings), so it
    // counts as a string alongside `String` (#283).
    impl_js_family_is_string!(Mozjs, String2);

    impl_is_else_if_parent_clause!(Mozjs, IfStatement, ElseClause);
}
