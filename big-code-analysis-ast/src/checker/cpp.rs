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

    // Issue #285 contract: every `Cpp::FunctionDefinition*` alias must
    // be enumerated here AND in `is_func`, `get_func_space_name`, and
    // `get_space_kind` (see `src/getter.rs`). Aliased kind_ids
    // 489/491/494 are not emitted by the currently pinned
    // `tree-sitter-cpp` parse tables on any input we can construct,
    // so a missing variant won't fail a parse-and-assert test — it
    // will silently drop those nodes from FuncSpace creation the next
    // time a grammar bump starts emitting them (see lesson 2 in
    // `docs/development/lessons_learned.md`).
    //
    // A struct / class specifier opens a space only when it has a body,
    // gated by the same predicate as `get_space_kind` (#1555).
    // The space tree is declared in `roles/cpp.rs` (#1696): the
    // `FuncOpener` and `Container` rows are what the walk promotes and
    // `opens_space` labels. The rows read no bytes and no chain (the
    // body gate reads the node's own fields), so the byte-less spelling
    // forwards with none and answers the same.
    fn is_func_space(node: &Node) -> bool {
        Self::is_func_space_with_code(node, &[], Ancestors::unknown())
    }

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::cpp::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    // Issue #285 contract: keep this in sync with `is_func_space` and
    // the C++ getters — see comment above `is_func_space`.
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

    // See `CCode::is_call` (#1254): the unsuffixed variant is the
    // grammar's always-aliased `preproc_call_expression` and never
    // reaches `kind_id()`, so every real call — free, member (`o.m()` /
    // `p->m()`), qualified (`ns::f()`) and through a function pointer —
    // arrives as `CallExpression2`. Kept listed defensively.
    //
    // `new_expression` is deliberately not listed: object creation is an
    // ABC concern rather than a call site, matching Groovy / Java / C#
    // (#430) — note the ABC walker in `src/metrics/abc/cpp.rs` *does*
    // count it. This excludes `new T(4)` and declaration-position
    // `T t(1)` only. Functional-style construction (`return T(4)`) is
    // still counted, because tree-sitter-cpp emits it as an ordinary
    // `call_expression` and nothing in the tree distinguishes it.
    fn is_call(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Cpp::CallExpression | Cpp::CallExpression2
        )
    }

    // C's `(void)` marker, shared with C++, Mozcpp and Objective-C.
    fn is_empty_param_marker(param: &Node, code: &[u8]) -> bool {
        c_family_void_parameter(param, code)
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Cpp::LPAREN | Cpp::LPAREN2 | Cpp::COMMA | Cpp::RPAREN
        )
    }

    impl_simple_is_string!(Cpp, StringLiteral, ConcatenatedString, RawStringLiteral);

    impl_is_else_if_parent_clause!(Cpp, IfStatement, ElseClause);

    #[inline]
    fn is_primitive(node: &Node) -> bool {
        // role-bypass: allow(Primitive) — key selection after the row
        // has classified the node: which of the row's kinds is one kind
        // for many spellings and so is keyed by text, not whether the
        // node is a primitive
        node.kind_id() == Cpp::PrimitiveType
    }
}
