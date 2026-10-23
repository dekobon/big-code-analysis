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
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Rust::LineComment || node.kind_id() == Rust::BlockComment
    }

    fn is_doc_comment(node: &Node, _code: &[u8]) -> bool {
        // tree-sitter-rust marks every doc form — `///`, `//!`, `/** */`,
        // `/*! */` — with a `doc_comment` child after its opener and
        // marker, and leaves `////` and `/**/` without one, so the bytes
        // are never consulted. A plain comment has at most its two
        // delimiters as children, so the count spares every one of them
        // the cursor the child scan allocates.
        Self::is_comment(node) && node.child_count() > 2 && node.is_child(Rust::DocComment as u16)
    }

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

    // What rustc's `unused_doc_comments` lint leaves alone: an item, an
    // associated or foreign item, a field or a variant. It lints a doc
    // comment on an extern block, a macro invocation, a statement, an
    // expression and a generic parameter, none of which is listed. A tuple
    // struct's field has no node of its own, so its first token stands in.
    fn is_doc_target<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
        if ancestors.parent_has_kind(node, Rust::OrderedFieldDeclarationList as u16) {
            return node.is_named() && !Self::is_comment(node);
        }
        matches!(
            node.kind_id().into(),
            Rust::ModItem
                | Rust::StructItem
                | Rust::UnionItem
                | Rust::EnumItem
                | Rust::EnumVariant
                | Rust::FieldDeclaration
                | Rust::ConstItem
                | Rust::StaticItem
                | Rust::TypeItem
                | Rust::FunctionItem
                | Rust::FunctionSignatureItem
                | Rust::ImplItem
                | Rust::TraitItem
                | Rust::AssociatedType
                | Rust::UseDeclaration
                | Rust::ExternCrateDeclaration
                | Rust::MacroDefinition
        )
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

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Rust::SourceFile
                | Rust::FunctionItem
                | Rust::ImplItem
                | Rust::TraitItem
                | Rust::ClosureExpression
        )
    }

    fn is_func<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        node.kind_id() == Rust::FunctionItem
    }

    fn is_closure<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        node.kind_id() == Rust::ClosureExpression
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Rust::CallExpression
    }

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

    impl_simple_is_string!(Rust, StringLiteral, RawStringLiteral);

    // Rust models `else if` as a nested `if_expression` (not an
    // `if_statement`) sitting directly inside the `else_clause`.
    impl_is_else_if_parent_clause!(Rust, IfExpression, ElseClause);

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
    /// `#[…]` siblings that mark it. The runtime guard
    /// in `spaces::metrics_with_options` only consults this hook
    /// when the caller opts in via `MetricsOptions::exclude_tests`,
    /// so the default `metrics()` entry point is unaffected.
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
