//! `Checker` implementation for Mozilla C++.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for MozcppCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Mozcpp::Comment
    }

    impl_doxygen_doc_comment!();

    // Mozilla's C++ grammar names its nodes as tree-sitter-cpp does, so
    // C++'s rule applies unchanged (#1613).
    impl_doxygen_doc_binding!();

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
    //
    // A struct / class specifier opens a space only when it has a body,
    // gated by the same predicate as `get_space_kind` (#1555).
    // The space tree is declared in `roles/mozcpp.rs` (#1696): the
    // `FuncOpener` and `Container` rows are what the walk promotes and
    // `opens_space` labels. The rows read no bytes and no chain (the
    // body gate reads the node's own fields), so the byte-less spelling
    // forwards with none and answers the same.
    fn is_func_space(node: &Node) -> bool {
        Self::is_func_space_with_code(node, &[], Ancestors::known(&[]))
    }

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::mozcpp::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    // Issue #285 contract: keep this in sync with `is_func_space` and
    // the C++ getters — see comment above `is_func_space`.
    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::mozcpp::func_opener::is_callable(node, &[], ancestors)
    }

    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::mozcpp::closure::owns(node, &[], ancestors)
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

    // C's `(void)` marker, shared with C++, Mozcpp and Objective-C.
    fn is_empty_param_marker(param: &Node, code: &[u8]) -> bool {
        c_family_void_parameter(param, code)
    }

    // `Call` and `StringLit` are the rows of `roles/mozcpp.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_call_row!(mozcpp);
    impl_string_lit_row!(mozcpp);

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Mozcpp::LPAREN | Mozcpp::LPAREN2 | Mozcpp::COMMA | Mozcpp::RPAREN
        )
    }

    impl_is_else_if_parent_clause!(Mozcpp, IfStatement, ElseClause);

    #[inline]
    fn is_primitive(node: &Node) -> bool {
        // role-bypass: allow(Primitive) — key selection after the row
        // has classified the node: which of the row's kinds is one kind
        // for many spellings and so is keyed by text, not whether the
        // node is a primitive
        node.kind_id() == Mozcpp::PrimitiveType
    }
}
