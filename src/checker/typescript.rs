//! `Checker` implementation for TypeScript.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for TypescriptCode {
    fn is_comment(node: &Node) -> bool {
        // See MozjsCode::is_comment — `html_comment` (Annex-B `<!-- -->`)
        // is a comment kind for loc/tokens/find parity (#697).
        matches!(
            node.kind_id().into(),
            Typescript::Comment | Typescript::HtmlComment
        )
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Typescript::Program
                | Typescript::FunctionExpression
                | Typescript::Class
                | Typescript::GeneratorFunction
                | Typescript::FunctionDeclaration
                | Typescript::MethodDefinition
                | Typescript::GeneratorFunctionDeclaration
                | Typescript::ClassDeclaration
                | Typescript::AbstractClassDeclaration
                | Typescript::InterfaceDeclaration
                | Typescript::ArrowFunction
                // ES2022 `static { … }`. Referenced nowhere
                // outside the generated enum before #1184, so a
                // class static block opened no space and its
                // control flow was charged to the class.
                | Typescript::ClassStaticBlock
        )
    }

    is_js_func_and_closure_checker!(Typescript, PublicFieldDefinition);

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Typescript::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Typescript::LPAREN | Typescript::COMMA | Typescript::RPAREN
        )
    }

    // TS's only anonymous `"string"` alias, `String2` (kind_id 135),
    // is the `: string` type-annotation keyword — not a literal — so
    // it is deliberately excluded: `find string` / `count string`
    // report literals, and `Getter::get_op_type` classifies the
    // keyword's `predefined_type` wrapper as an operator (#1261,
    // retiring the #313 parity that made the keyword an operand).
    impl_js_family_is_string!(Typescript);

    impl_is_else_if_parent_clause!(Typescript, IfStatement, ElseClause);

    #[inline]
    fn is_primitive(node: &Node) -> bool {
        node.kind_id() == Typescript::PredefinedType
    }
}
