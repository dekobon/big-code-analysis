//! `Checker` implementation for C++.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for CppCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Cpp::Comment
    }

    impl_doxygen_doc_comment!();

    // C++ adds namespaces, `using`, templates (a forward-declared template
    // included), friends and access specifiers, which Doxygen reads past;
    // an explicit instantiation takes none (#1613).
    impl_doxygen_doc_binding!();

    fn is_useful_comment<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
        get_aho_corasick_match(&code[node.start_byte()..node.end_byte()])
    }

    // The space tree is declared in `roles/cpp.rs` (#1696): the
    // `FuncOpener` row — the four aliased `function_definition` ids,
    // three never emitted at the pin and kept so a bump that starts
    // emitting one does not silently drop its spaces (#285, lesson 2) —
    // and the `Container` row, whose struct / class specifier opens a
    // space only with a body (#1555). The rows read no bytes and no
    // chain (the body gate reads the node's own fields), so the
    // byte-less spelling forwards with none and answers the same.
    fn is_func_space(node: &Node) -> bool {
        Self::is_func_space_with_code(node, &[], Ancestors::known(&[]))
    }

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::cpp::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::cpp::func_opener::is_callable(node, &[], ancestors)
    }

    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::cpp::closure::owns(node, &[], ancestors)
    }

    // A `friend` with an inline body sits inside the class braces but is
    // not a member of it, so `wmc` must not weight it into the class's
    // WMC (#1301). `npm` already declines to count it, by not matching
    // `friend_declaration` among the `field_declaration_list` children it
    // walks (`src/metrics/npm/cpp.rs`).
    //
    // The parent check is the whole predicate for both shapes the grammar
    // admits: a plain friend parses as `friend_declaration >
    // function_definition`, and a templated one as `template_declaration
    // > friend_declaration > function_definition` — the function's parent
    // is `friend_declaration` either way. A friend *declared* without a
    // body (`friend void f();`, `friend A operator+(const A&, const A&);`)
    // parses as `friend_declaration > declaration`, opens no function
    // space, and so never reaches here.
    fn is_non_member_function<'a>(
        node: &Node<'a>,
        _code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        ancestors.parent_has_kind(node, Cpp::FriendDeclaration as u16)
    }

    // C's `(void)` marker, shared with C++, Mozcpp and Objective-C.
    fn is_empty_param_marker(param: &Node, code: &[u8]) -> bool {
        c_family_void_parameter(param, code)
    }

    // `Call` and `StringLit` are the rows of `roles/cpp.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(cpp, call, is_call, is_call_with_code);
    impl_row_predicates!(cpp, string_lit, is_string, is_string_with_code);

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Cpp::LPAREN | Cpp::LPAREN2 | Cpp::COMMA | Cpp::RPAREN
        )
    }

    impl_is_else_if_parent_clause!(Cpp, IfStatement, ElseClause);

    // A primitive is the `Primitive` row's kind table, gates aside.
    #[inline]
    fn is_primitive(node: &Node) -> bool {
        crate::roles::cpp::primitive::lists_kind(node.kind_id())
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
        node.kind_id() == Cpp::PrimitiveType
    }
}
