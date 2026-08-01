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
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use Rust::*;
        // Macro expansion is not tracked; macros are treated as opaque tokens.
        let mut nesting = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            IfExpression if !Self::is_else_if(node, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            ForExpression | WhileExpression | LoopExpression | MatchExpression => {
                increase_nesting(stats, &mut nesting);
            }
            Else /*else-if also */ => {
                increment_by_one(stats);
            }
            BreakExpression | ContinueExpression => {
                if let Some(label_child) = node.child(1)
                    && let Label = label_child.kind_id().into()
                {
                    increment_by_one(stats);
                }
            }
            // `LetChain` (the visible alias) and `LetChain2` (the hidden
            // `_let_chain` supertype) are Rust 2024 let-chains:
            // `if let Some(x) = a && let Some(y) = b && cond`. Their `&&`
            // tokens are direct children of the chain node — not wrapped
            // in `BinaryExpression` — so without dispatching them through
            // `compute_booleans` here, let-chain `&&` is invisible to the
            // boolean-sequence counter (issue #396). Cyclomatic already
            // counts the same tokens via the AMPAMP keyword arm.
            BinaryExpression | LetChain | LetChain2 => {
                compute_booleans(node, stats, AMPAMP, PIPEPIPE);
            }
            FunctionItem => {
                enter_function_boundary(&mut nesting, node, ancestors, &[FunctionItem]);
            }
            ClosureExpression => {
                nesting.lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
