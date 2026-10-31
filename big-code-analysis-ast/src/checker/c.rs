//! `Checker` implementation for C.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for CCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == C::Comment
    }

    impl_doxygen_doc_comment!();

    // C as Doxygen reads it: a doc block documents the next declaration,
    // macro or typedef, read past `#include` and an empty conditional, and
    // a forward `struct s;` takes none (#1613).
    impl_doxygen_doc_binding!();

    fn is_useful_comment<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
        get_aho_corasick_match(&code[node.start_byte()..node.end_byte()])
    }

    // C has no classes/namespaces and no methods, so the only code
    // spaces are the translation unit and function definitions. Struct /
    // The space tree is declared in `roles/c.rs` (#1696): the file and
    // both aliased `function_definition` ids (#285, lesson 2). Struct /
    // union / enum specifiers hold no functions in C, so — unlike C++ —
    // they are deliberately no spaces (each would be an empty one). The
    // rows read no bytes and no chain, so the byte-less spelling forwards
    // with none.
    fn is_func_space(node: &Node) -> bool {
        Self::is_func_space_with_code(node, &[], Ancestors::known(&[]))
    }

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::c::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::c::func_opener::is_callable(node, &[], ancestors)
    }

    // C has no closures/lambdas.
    fn is_closure<'a>(_node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        false
    }

    // C's `(void)` marker, shared with C++, Mozcpp and Objective-C.
    fn is_empty_param_marker(param: &Node, code: &[u8]) -> bool {
        c_family_void_parameter(param, code)
    }

    // `Call` and `StringLit` are the rows of `roles/c.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(c, call, is_call, is_call_with_code);
    impl_row_predicates!(c, string_lit, is_string, is_string_with_code);

    fn is_non_arg(node: &Node) -> bool {
        matches!(node.kind_id().into(), C::LPAREN | C::COMMA | C::RPAREN)
    }

    // The `ElseIf` row of `roles/c.rs`: an `if_statement` whose
    // parent is the `else_clause`, read off the caller's chain. The row
    // reads no bytes, so the empty code is exact (#1697).
    impl_else_if_from_row!(c);

    // A primitive is the `Primitive` row's kind table, gates aside.
    #[inline]
    fn is_primitive(node: &Node) -> bool {
        crate::roles::c::primitive::lists_kind(node.kind_id())
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
        node.kind_id() == C::PrimitiveType
    }
}
