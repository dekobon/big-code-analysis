//! `Checker` implementation for TSX.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for TsxCode {
    fn is_comment(node: &Node) -> bool {
        // See MozjsCode::is_comment — `html_comment` (Annex-B `<!-- -->`)
        // is a comment kind for loc/tokens/find parity (#697).
        matches!(node.kind_id().into(), Tsx::Comment | Tsx::HtmlComment)
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Tsx::Program
                | Tsx::FunctionExpression
                | Tsx::Class
                | Tsx::GeneratorFunction
                | Tsx::FunctionDeclaration
                | Tsx::MethodDefinition
                | Tsx::GeneratorFunctionDeclaration
                | Tsx::ClassDeclaration
                | Tsx::AbstractClassDeclaration
                | Tsx::InterfaceDeclaration
                | Tsx::ArrowFunction
                // ES2022 `static { … }`. Referenced nowhere
                // outside the generated enum before #1184, so a
                // class static block opened no space and its
                // control flow was charged to the class.
                | Tsx::ClassStaticBlock
        )
    }

    is_js_func_and_closure_checker!(Tsx, PublicFieldDefinition);

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Tsx::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Tsx::LPAREN | Tsx::COMMA | Tsx::RPAREN
        )
    }

    // TSX's `String2` (kind_id 261) is its string-*literal* alias —
    // ordinary and JSX-attribute literals both parse as it — so it
    // counts as a string (#283). `String3` (kind_id 141) is the
    // `: string` type-annotation keyword (the role TS's `String2`
    // plays) and is deliberately excluded, matching TS (#1261).
    impl_js_family_is_string!(Tsx, String2);

    impl_is_else_if_parent_clause!(Tsx, IfStatement, ElseClause);

    #[inline]
    fn is_primitive(node: &Node) -> bool {
        node.kind_id() == Tsx::PredefinedType
    }
}
