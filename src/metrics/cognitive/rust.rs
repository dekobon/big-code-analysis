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
        // bca: suppress(cyclomatic, halstead) — exhaustive grammar dispatch
        // One arm per construct the grammar can hand us, each a row read
        // or a kind the rows do not own: the count is the construct list,
        // not branching a reader must hold, and the effort is the table's
        // vocabulary (#1695, #1697).
        use crate::roles::rust::{
            branch, closure, else_if, func_opener, r#loop, short_circuit, switch,
        };
        use Rust::*;
        // Macro expansion is not tracked; macros are treated as opaque tokens.
        let mut nesting = get_nesting_from_map(node, nesting_map);

        let kind = node.kind_id();

        match kind.into() {
            // Every branch the `Branch` row owns opens a level — the `if`
            // expression, and `let PAT = e else { … };`, the row's gated
            // owner: an `if let` that diverges instead of nesting the happy
            // path, so it pays what its twin pays, a level for everything
            // under it, the `else` block included (#1548, #1542) — unless
            // the `ElseIf` row recognises the `if` as the continuation of
            // an `else if` chain (its parent is the `else_clause`), where
            // the `Else` arm below pays the +1 and no level opens (#1697).
            _ if branch::owns_kind(kind, node, code, ancestors)
                && !else_if::owns_kind(kind, node, code, ancestors) =>
            {
                increase_nesting(stats, &mut nesting);
            }
            // Every loop the `Loop` row owns and the `match` the `Switch`
            // row owns.
            _ if r#loop::owns_kind(kind, node, code, ancestors)
                || switch::owns_kind(kind, node, code, ancestors) =>
            {
                increase_nesting(stats, &mut nesting);
            }
            // `Else` here is the `else` keyword token, which the grammar
            // also emits for the `else` of an `else if` — so this arm
            // covers both. A `let`-`else`'s token is a direct child of the
            // `let_declaration`, which the arm above already charged.
            // role-bypass: allow(Branch) — which construct the `else` belongs
            // to, read off the parent; not whether the parent is a branch
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
