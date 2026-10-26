//! `Cognitive` implementation for C.
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

impl Cognitive for CCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use crate::roles::c::{func_opener, short_circuit, ternary};
        use C::*;

        // Macro expansion is not tracked; macros are treated as opaque tokens.
        let mut nesting = get_nesting_from_map(node, nesting_map);

        let kind = node.kind_id();

        match kind.into() {
            IfStatement if !Self::is_else_if(node, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            ForStatement | WhileStatement | DoStatement | SwitchStatement => {
                increase_nesting(stats, &mut nesting);
            }
            // The `conditional_expression` the `Ternary` row owns (#1695).
            _ if ternary::owns_kind(kind, node, code, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // `Else` here is the `else` keyword token, which the grammar
            // also emits for the `else` of an `else if` — so this arm
            // covers both.
            GotoStatement | Else => {
                increment_by_one(stats);
            }
            // A `binary_expression` the `ShortCircuit` row owns (#1695).
            _ if short_circuit::owns_kind(kind, node, code, ancestors) => {
                compute_booleans_from(node, stats, short_circuit::PAYERS);
            }
            // A named function — the `FuncOpener` row's callable owners —
            // resets structural nesting and bumps the function-depth
            // surcharge when nested in another, matching Rust and the
            // sibling families; without it a definition inside a control
            // construct inherited the enclosing nesting and a nested one
            // missed the SonarSource B-nesting amplification (#696). The
            // stop list is the row's callable kinds.
            _ if func_opener::is_callable_kind(kind, node, code, ancestors) => {
                enter_function_boundary(&mut nesting, node, ancestors, func_opener::CALLABLE);
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
