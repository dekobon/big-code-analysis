//! `Checker` implementation for JavaScript.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for JavascriptCode {
    fn is_comment(node: &Node) -> bool {
        // See MozjsCode::is_comment — `html_comment` (Annex-B `<!-- -->`)
        // is a comment kind for loc/tokens/find parity (#697).
        matches!(
            node.kind_id().into(),
            Javascript::Comment | Javascript::HtmlComment
        )
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Javascript::Program
                | Javascript::FunctionExpression
                | Javascript::Class
                | Javascript::GeneratorFunction
                | Javascript::FunctionDeclaration
                | Javascript::MethodDefinition
                | Javascript::GeneratorFunctionDeclaration
                | Javascript::ClassDeclaration
                | Javascript::ArrowFunction
                // ES2022 `static { … }`. Referenced nowhere
                // outside the generated enum before #1184, so a
                // class static block opened no space and its
                // control flow was charged to the class.
                | Javascript::ClassStaticBlock
        )
    }

    is_js_func_and_closure_checker!(Javascript, FieldDefinition);

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Javascript::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Javascript::LPAREN | Javascript::COMMA | Javascript::RPAREN
        )
    }

    // `String2` (kind_id 221) is JS's anonymous string-*literal* alias
    // (e.g. the module string in `import "m"`), so it counts as a
    // string alongside `String` (#283).
    impl_js_family_is_string!(Javascript, String2);

    impl_is_else_if_parent_clause!(Javascript, IfStatement, ElseClause);
}
