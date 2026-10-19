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

    // tsc reads `/***` as JSDoc, so TypeScript follows tsc, not JSDoc.
    impl_js_family_doc_comment!(Typescript, tsdoc_block);

    // tsc's `getJSDocCommentsAndTags` takes the last `/**` before a
    // declaration's first token, past every plain comment; a comment after
    // a decorator is inside the declaration and documents nothing. TypeDoc
    // withholds a `@module` or `@packageDocumentation` comment from the
    // declaration and reads it as the file's (#1613). tsc reads a
    // declaration's leading comments from the line break after the token
    // before it, so a doc comment after code on its row documents nothing,
    // except before a parameter, whose trailing comments it reads too.
    impl_js_family_doc_binding!(
        Typescript,
        |node, code| Self::is_comment(node) && !Self::is_doc_comment(node, code),
        crate::lang_helpers::doc_comment::tsdoc_module,
        tsdoc_target,
        |target| matches!(
            target.kind_id().into(),
            Typescript::RequiredParameter
                | Typescript::RequiredParameter2
                | Typescript::OptionalParameter
                | Typescript::OptionalParameter2
        )
    );

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
