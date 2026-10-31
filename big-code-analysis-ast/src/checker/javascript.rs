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

    // JSDoc ignores `/***`; plain JavaScript follows it.
    impl_js_family_doc_comment!(Javascript, javadoc_block);

    // JSDoc's `getLeadingJsdocComment` takes the nearest block comment
    // before a node, line comments filtered out, so a plain `/* */` in
    // between detaches a doc comment and a `//` does not. A virtual
    // comment (`@file`, `@module`, a tag that names a symbol) documents
    // that symbol and stands alone (#1613). The Babel leading comments it
    // reads hold every comment since the node before, so a doc comment
    // after code on its row documents the declaration below it.
    impl_js_family_doc_binding!(
        Javascript,
        |node, code| Self::is_comment(node)
            && !crate::lang_helpers::doc_comment::node_bytes(node, code).starts_with(b"/*"),
        crate::lang_helpers::doc_comment::jsdoc_virtual,
        jsdoc_target,
        |_| true
    );

    // The space tree is declared in `roles/javascript.rs` (#1696): the
    // `FuncOpener` row (every function form, the ES2022 `static { … }`
    // block — #1184) and the `Container` row are what the walk
    // promotes, and `opens_space` labels them; which function form is
    // a *function* rather than a closure is the binding-site question
    // the rows' gates answer off the chain (#1188). The byte-less
    // `is_func_space` is the legacy wrapper: no `opens` here is gated,
    // so it answers the same with no chain.
    fn is_func_space(node: &Node) -> bool {
        Self::is_func_space_with_code(node, &[], Ancestors::known(&[]))
    }

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::javascript::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::javascript::func_opener::is_callable(node, &[], ancestors)
    }

    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::javascript::closure::owns(node, &[], ancestors)
    }

    // `Call` and `StringLit` are the rows of `roles/javascript.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(javascript, call, is_call, is_call_with_code);
    impl_row_predicates!(javascript, string_lit, is_string, is_string_with_code);

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Javascript::LPAREN | Javascript::COMMA | Javascript::RPAREN
        )
    }

    // The `ElseIf` row of `roles/javascript.rs`: an `if_statement` whose
    // parent is the `else_clause`, read off the caller's chain. The row
    // reads no bytes, so the empty code is exact (#1697).
    impl_else_if_from_row!(javascript);
}
