//! `Checker` implementation for TSX.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for TsxCode {
    fn is_comment(node: &Node) -> bool {
        // See MozjsCode::is_comment — `html_comment` (Annex-B `<!-- -->`)
        // is a comment kind for loc/tokens/find parity (#697).
        matches!(node.kind_id().into(), Tsx::Comment | Tsx::HtmlComment)
    }

    // tsc reads `/***` as JSDoc, so TSX follows tsc, not JSDoc.
    impl_js_family_doc_comment!(Tsx, tsdoc_block);

    // tsc's `getJSDocCommentsAndTags` takes the last `/**` before a
    // declaration's first token, past every plain comment; a comment after
    // a decorator is inside the declaration and documents nothing. TypeDoc
    // withholds a `@module` or `@packageDocumentation` comment from the
    // declaration and reads it as the file's (#1613). tsc reads a
    // declaration's leading comments from the line break after the token
    // before it, so a doc comment after code on its row documents nothing,
    // except before a parameter, whose trailing comments it reads too.
    impl_js_family_doc_binding!(
        Tsx,
        |node, code| Self::is_comment(node) && !Self::is_doc_comment(node, code),
        crate::lang_helpers::doc_comment::tsdoc_module,
        tsdoc_target,
        |target| matches!(
            target.kind_id().into(),
            Tsx::RequiredParameter
                | Tsx::RequiredParameter2
                | Tsx::OptionalParameter
                | Tsx::OptionalParameter2
        )
    );

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
