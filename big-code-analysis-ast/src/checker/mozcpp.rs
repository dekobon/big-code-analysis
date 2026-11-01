//! `Checker` implementation for Mozilla C++.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl_doxygen_row!(MozcppCode, mozcpp);

impl Checker for MozcppCode {
    // The `Comment` and `DocComment` rows of `roles/mozcpp.rs` (#1699):
    // the one comment kind, documentation under one of Doxygen's markers.
    // Doxygen appends every doc block in a function body to that
    // function's documentation as its in-body description
    // (`HIDE_IN_BODY_DOCS` defaults to NO, #1597).
    impl_doc_comment_rows_in_bodies!(mozcpp);

    // Mozilla's C++ grammar names its nodes as tree-sitter-cpp does, so
    // C++'s rule applies unchanged (#1613).
    impl_doxygen_doc_binding!();

    fn is_useful_comment<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
        get_aho_corasick_match(&code[node.start_byte()..node.end_byte()])
    }

    // The space tree is declared in `roles/mozcpp.rs` (#1696), the same
    // shared fragments as `checker/cpp.rs` reads; the rows read no bytes
    // and no chain, so the byte-less spelling forwards with none.
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
    impl_row_predicates!(mozcpp, call, is_call, is_call_with_code);
    impl_row_predicates!(mozcpp, string_lit, is_string, is_string_with_code);

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Mozcpp::LPAREN | Mozcpp::LPAREN2 | Mozcpp::COMMA | Mozcpp::RPAREN
        )
    }

    // The `ElseIf` row of `roles/mozcpp.rs`: an `if_statement` whose
    // parent is the `else_clause`, read off the caller's chain. The row
    // reads no bytes, so the empty code is exact (#1697).
    impl_else_if_from_row!(mozcpp);

    // A primitive is the `Primitive` row's kind table, gates aside.
    #[inline]
    fn is_primitive(node: &Node) -> bool {
        crate::roles::mozcpp::primitive::lists_kind(node.kind_id())
    }

    #[inline]
    fn keys_operator_by_text<'a>(
        node: &Node<'a>,
        _code: &[u8],
        _ancestors: Ancestors<'a, '_>,
    ) -> bool {
        // role-bypass: allow(Primitive) — key selection after the row
        // has classified the node: which of the row's kinds is one kind
        // for many spellings and so is keyed by text, not whether the
        // node is a primitive
        node.kind_id() == Mozcpp::PrimitiveType
    }
}
