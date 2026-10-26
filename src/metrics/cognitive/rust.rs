//! `Cognitive` implementation for Rust.
#![allow(
    clippy::enum_glob_use,
    clippy::match_same_arms,
    clippy::needless_pass_by_value,
    clippy::wildcard_imports
)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cognitive for RustCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use crate::roles::rust::{closure, func_opener, short_circuit};
        use Rust::*;
        // Macro expansion is not tracked; macros are treated as opaque tokens.
        let mut nesting = get_nesting_from_map(node, nesting_map);

        let kind = node.kind_id();

        match kind.into() {
            IfExpression if !Self::is_else_if(node, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            ForExpression | WhileExpression | LoopExpression | MatchExpression => {
                increase_nesting(stats, &mut nesting);
            }
            // `let PAT = e else { … };` is an `if let` that diverges
            // instead of nesting the happy path, so it pays what its
            // `if let` twin pays: a structural increment plus the current
            // nesting, and a level for everything under it — the `else`
            // block included, as the twin's body is (#1548). Keyed on the
            // `alternative` field, as cyclomatic does (#1542).
            LetDeclaration if node.child_by_field_name("alternative").is_some() => {
                increase_nesting(stats, &mut nesting);
            }
            // `Else` here is the `else` keyword token, which the grammar
            // also emits for the `else` of an `else if` — so this arm
            // covers both. A `let`-`else`'s token is a direct child of the
            // `let_declaration`, which the arm above already charged.
            Else if !ancestors.parent_has_kind(node, LetDeclaration as u16) => {
                increment_by_one(stats);
            }
            BreakExpression | ContinueExpression => {
                if let Some(label_child) = node.child(1)
                    && let Label = label_child.kind_id().into()
                {
                    increment_by_one(stats);
                }
            }
            // A `binary_expression` or a let-chain the `ShortCircuit` row
            // owns (#1695). `LetChain` (the visible alias) and `LetChain2`
            // (the hidden `_let_chain` supertype) are Rust 2024
            // let-chains: `if let Some(x) = a && let Some(y) = b && cond`.
            // Their `&&` tokens are direct children of the chain node —
            // not wrapped in `BinaryExpression` — so without the row
            // owning them, let-chain `&&` is invisible to the
            // boolean-sequence counter (issue #396).
            _ if short_circuit::owns_kind(kind, node, code, ancestors) => {
                compute_booleans_from(node, stats, short_circuit::PAYERS);
            }
            // A named function — the `FuncOpener` row's callable owners —
            // resets structural nesting and bumps the function-depth
            // surcharge when nested in another (#696); the stop list is
            // the row's callable kinds.
            _ if func_opener::is_callable_kind(kind, node, code, ancestors) => {
                enter_function_boundary(&mut nesting, node, ancestors, func_opener::CALLABLE);
            }
            // A closure — the `Closure` row — charges the lambda channel
            // and does not reset nesting.
            _ if closure::owns_kind(kind, node, code, ancestors) => {
                nesting.lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
