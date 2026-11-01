//! `Checker` implementation for Rust.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::lang_helpers::doc_comment::node_bytes;

/// Whether an inner doc comment (`//!`, `/*! */`) that `body` holds as a
/// direct child opens `function`'s body, and so documents the function:
/// `body` is a `function_item`'s body block. Inner attributes are valid
/// in a body only directly after its `{` (Rust Reference, "Functions"),
/// and an inner doc comment after a statement, an item or an outer doc
/// comment is a hard error (E0753), so for valid Rust the block alone
/// decides. Only rustc-rejected input could tell a prefix scan apart, and
/// that is left undecided (grammar-dispatch rule 6).
// role-bypass: allow(FuncOpener) — where an inner doc comment sits, not
// whether the item opens a space
fn rust_inner_doc_opens_body(function: &Node, body: &Node) -> bool {
    function.kind_id() == Rust::FunctionItem
        && function
            .child_by_field_name("body")
            .is_some_and(|actual| actual.id() == body.id())
}

/// An inner doc comment, `//!` or `/*! */`: one that documents the item
/// enclosing it rather than the one after it.
fn rust_inner_doc(bytes: &[u8]) -> bool {
    bytes.starts_with(b"//!") || bytes.starts_with(b"/*!")
}

/// Whether a Rust doc comment attaches to what follows it: an outer one
/// outside a macro's token trees (#1613).
fn rust_doc_binds_forward<'a>(
    comment: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    let inner = rust_inner_doc(node_bytes(comment, code));
    let in_macro = ancestors.parent(comment).is_some_and(|parent| {
        // role-bypass: allow(DocTarget) — a comment *inside* a macro definition's
        // token trees documents what the expansion makes of it; binding context
        // over the parent, not whether the definition is a target
        matches!(
            parent.kind_id().into(),
            Rust::TokenTree
                | Rust::TokenTree2
                | Rust::TokenTreePattern
                | Rust::TokenRepetition
                | Rust::TokenRepetitionPattern
                | Rust::MacroDefinition
        )
    });
    !inner && !in_macro
}

impl Checker for RustCode {
    // The `Comment` and `DocComment` rows of `roles/rust.rs` (#1699):
    // both comment kinds, documentation by the grammar's `doc_comment`
    // child — a structural gate, so the bytes are never read.
    impl_comment_from_rows!(rust);
    impl_doc_comment_from_row!(rust);

    // rustc lints a doc comment on a statement or expression as unused and
    // rustdoc renders no item nested in a body, so a body holds its doc
    // comments (the default `holds_doc_comment_in_body`) — except the
    // inner doc comment opening a function's body, which is that
    // function's own `#![doc]`. Only the function two levels up
    // (comment → body block → `function_item`) can grant it, and
    // `documents_where_it_sits` asks exactly those two levels.
    fn doc_comment_opens_body(comment: &Node, ancestor: &Node, below: &Node, code: &[u8]) -> bool {
        rust_inner_doc(node_bytes(comment, code)) && rust_inner_doc_opens_body(ancestor, below)
    }

    // rustc makes every outer doc comment above an item one of its
    // attributes, across blank rows, plain comments and the item's other
    // attributes, and rustdoc concatenates them (#1613). An inner doc
    // comment documents the item around it, and one in a macro's token
    // trees documents whatever the expansion makes of it; both keep the
    // unbound reading.
    const DOC_BINDING: Option<DocBinding> = Some(DocBinding {
        skips: |node, _| node.kind_id() == Rust::AttributeItem,
        forward: rust_doc_binds_forward,
        ..DocBinding::new(KeptDocs::All)
    });

    // The `DocTarget` row of `roles/rust.rs` (#1699), plus the one
    // clause no kind list spells: a tuple struct's field has no node of
    // its own, so its first token — any named node under the
    // `ordered_field_declaration_list` that is not a comment — stands
    // in. Context on the parent, not membership, so it stays here.
    fn is_doc_target<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
        // role-bypass: allow(DocTarget) — the tuple-field slot: a position
        // under the field list, decided for any named kind
        if ancestors.parent_has_kind(node, Rust::OrderedFieldDeclarationList as u16) {
            return node.is_named() && !Self::is_comment(node);
        }
        crate::roles::rust::doc_target::owns(node, code, ancestors)
    }

    fn is_useful_comment<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
        if let Some(parent) = ancestors.parent(node)
            && parent.kind_id() == Rust::TokenTree
        {
            // A comment could be a macro token
            return true;
        }
        let code = &code[node.start_byte()..node.end_byte()];
        code.starts_with(b"/// cbindgen:")
    }

    // The space tree is declared in `roles/rust.rs` (#1696): the
    // `FuncOpener` and `Container` rows are what the walk promotes and
    // `opens_space` labels. The rows read no bytes and no chain, so the
    // byte-less spelling forwards with none and answers the same.
    fn is_func_space(node: &Node) -> bool {
        Self::is_func_space_with_code(node, &[], Ancestors::known(&[]))
    }

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::rust::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::rust::func_opener::is_callable(node, &[], ancestors)
    }

    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::rust::closure::owns(node, &[], ancestors)
    }

    // `Call` and `StringLit` are the rows of `roles/rust.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(rust, call, is_call, is_call_with_code);
    impl_row_predicates!(rust, string_lit, is_string, is_string_with_code);

    fn is_non_arg(node: &Node) -> bool {
        // A *typed* receiver (`self: Box<Self>`, `self: Rc<Self>`,
        // `self: Pin<&mut Self>` — arbitrary self types) does not parse as
        // `SelfParameter`; the grammar models it as an ordinary `parameter`
        // node whose binding is the `self` keyword (`Rust::Zelf`). It is
        // still a receiver, so it is excluded too, for parity with the
        // bare-receiver case and with Go/C++ (#457). A normal `parameter`
        // such as `x: i32` binds an `identifier`, never `self`, so this
        // child check is unambiguous.
        let is_typed_self_receiver = node.kind_id() == Rust::Parameter
            && node.children().any(|child| child.kind_id() == Rust::Zelf);

        // `SelfParameter` is Rust's bare method receiver (`self`, `&self`,
        // `&mut self`). Like Go's `receiver` field and C++'s implicit
        // `this`, it is not a formal parameter and must not be counted
        // (see #457).
        matches!(
            node.kind_id().into(),
            Rust::LPAREN
                | Rust::COMMA
                | Rust::RPAREN
                | Rust::PIPE
                | Rust::AttributeItem
                | Rust::SelfParameter
        ) || is_typed_self_receiver
    }

    // The `ElseIf` row of `roles/rust.rs`: an `if_expression` whose
    // parent is the `else_clause`, read off the caller's chain. The row
    // reads no bytes, so the empty code is exact (#1697).
    impl_else_if_from_row!(rust);

    // Every kind the `Primitive` row owns is one `primitive_type` alias
    // keyed by text, so the row's own test is the key selection; the
    // seventeen aliases are listed once, in `roles/rust.rs` (#1693).
    #[inline]
    fn is_primitive(node: &Node) -> bool {
        crate::roles::rust::primitive::OWNERS.contains(&node.kind_id())
    }

    /// Skip the subtree when `node` is a `mod`, `fn`, `impl`,
    /// `trait`, `const`, or `static` item marked test-only by an
    /// outer or inner attribute (`#[test]`, `#[cfg(test)]`,
    /// `#[tokio::test]`, `#![cfg(test)]`, …), or is one of the
    /// `#[…]` siblings that mark it. The metric walk (`metrics_inner`
    /// in the root crate's `spaces::compute`) only consults this hook
    /// when the caller opts in via `MetricsOptions::exclude_tests`,
    /// so a default `MetricsOptions` leaves every subtree in.
    fn should_skip_subtree<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> SubtreeSkip {
        if rust_prunable_item(node) {
            return SubtreeSkip::of(rust_item_is_test_only(node, code, ancestors));
        }
        // An outer attribute is a *sibling* of the item it marks, not a
        // child, so pruning the item never reached it and Rust's `Loc`
        // catch-all credited its start row to PLOC — one phantom row per
        // pruned item, and a file that is nothing but a `#[cfg(test)]
        // mod` still reading `ploc 1` (#1431). Answering here, before the
        // row is ever recorded, is what makes the shared-row case behave:
        // a row the attribute happens to share with retained code is
        // still inserted by that code, and `Stats::settle_excluded_rows`
        // keeps it (#1417). Retracting the row afterwards could not tell
        // the two apart.
        //
        // The verdict is the whole run's, not this row's, so it is
        // reported as reaching the rest of the run: the walker then
        // asks once per run instead of once per row, which is what
        // keeps a deep file carrying one long run out of `O(run^2)`
        // (#1446).
        if node.kind_id() == Rust::AttributeItem {
            return rust_attribute_run_verdict(node, code, ancestors);
        }
        SubtreeSkip::RETAIN
    }
}
