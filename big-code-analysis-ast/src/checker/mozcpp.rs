//! `Checker` implementation for Mozilla C++.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for MozcppCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Mozcpp::Comment
    }

    fn is_useful_comment<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
        get_aho_corasick_match(&code[node.start_byte()..node.end_byte()])
    }

    // Issue #285 contract: every `Mozcpp::FunctionDefinition*` alias must
    // be enumerated here AND in `is_func`, `get_func_space_name`, and
    // `get_space_kind` (see `src/getter.rs`). Aliased kind_ids
    // 489/491/494 are not emitted by the currently pinned
    // `tree-sitter-mozcpp` parse tables on any input we can construct,
    // so a missing variant won't fail a parse-and-assert test — it
    // will silently drop those nodes from FuncSpace creation the next
    // time a grammar bump starts emitting them (see lesson 2 in
    // `docs/development/lessons_learned.md`).
    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Mozcpp::TranslationUnit
                | Mozcpp::FunctionDefinition
                | Mozcpp::FunctionDefinition2
                | Mozcpp::FunctionDefinition3
                | Mozcpp::FunctionDefinition4
                | Mozcpp::StructSpecifier
                | Mozcpp::ClassSpecifier
                | Mozcpp::NamespaceDefinition
        )
    }

    // Issue #285 contract: keep this in sync with `is_func_space` and
    // the C++ getters — see comment above `is_func_space`.
    fn is_func<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        matches!(
            node.kind_id().into(),
            Mozcpp::FunctionDefinition
                | Mozcpp::FunctionDefinition2
                | Mozcpp::FunctionDefinition3
                | Mozcpp::FunctionDefinition4
        )
    }

    fn is_closure<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        node.kind_id() == Mozcpp::LambdaExpression
    }

    // See `CppCode::is_non_member_function` (#1301). The Mozilla fork
    // spells `friend_declaration` with its own kind id but nests it
    // identically, so the parent check transfers unchanged.
    fn is_non_member_function<'a>(
        node: &Node<'a>,
        _code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        ancestors.parent_has_kind(node, Mozcpp::FriendDeclaration as u16)
    }

    // See `CCode::is_call` (#1254). The Mozilla fork inherits upstream
    // C++'s always-aliased `preproc_call_expression`, so the unsuffixed
    // variant never reaches `kind_id()` here either. Kept listed
    // defensively; `new_expression` deliberately not listed, with the same
    // scope and the same functional-style caveat as `CppCode::is_call`.
    fn is_call(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Mozcpp::CallExpression | Mozcpp::CallExpression2
        )
    }

    // C's `(void)` marker, shared with C++, Mozcpp and Objective-C.
    fn is_empty_param_marker(param: &Node, code: &[u8]) -> bool {
        c_family_void_parameter(param, code)
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Mozcpp::LPAREN | Mozcpp::LPAREN2 | Mozcpp::COMMA | Mozcpp::RPAREN
        )
    }

    impl_simple_is_string!(Mozcpp, StringLiteral, ConcatenatedString, RawStringLiteral);

    impl_is_else_if_parent_clause!(Mozcpp, IfStatement, ElseClause);

    #[inline]
    fn is_primitive(node: &Node) -> bool {
        node.kind_id() == Mozcpp::PrimitiveType
    }
}
