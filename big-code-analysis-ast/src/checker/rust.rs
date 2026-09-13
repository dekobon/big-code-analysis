//! `Checker` implementation for Rust.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for RustCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Rust::LineComment || node.kind_id() == Rust::BlockComment
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

    #[inline]
    fn is_primitive(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Rust::PrimitiveType
                | Rust::PrimitiveType2
                | Rust::PrimitiveType3
                | Rust::PrimitiveType4
                | Rust::PrimitiveType5
                | Rust::PrimitiveType6
                | Rust::PrimitiveType7
                | Rust::PrimitiveType8
                | Rust::PrimitiveType9
                | Rust::PrimitiveType10
                | Rust::PrimitiveType11
                | Rust::PrimitiveType12
                | Rust::PrimitiveType13
                | Rust::PrimitiveType14
                | Rust::PrimitiveType15
                | Rust::PrimitiveType16
                | Rust::PrimitiveType17
        )
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
