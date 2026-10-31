//! `Cognitive` implementation for Objective-C.
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

impl Cognitive for ObjcCode {
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
        use crate::roles::objc::{
            branch, catch, closure, else_if, func_opener, r#loop, short_circuit, switch, ternary,
        };
        use Objc::*;

        // Macro expansion is not tracked; macros are treated as opaque tokens.
        let mut nesting = get_nesting_from_map(node, nesting_map);

        let kind = node.kind_id();

        match kind.into() {
            // An `if_statement` the `Branch` row owns opens a level —
            // unless the `ElseIf` row recognises it as the `if` of an
            // `else if` (its parent is the `else_clause`), where the
            // `Else` arm below pays the +1 and no level opens (#1697).
            _ if branch::owns_kind(kind, node, code, ancestors)
                && !else_if::owns_kind(kind, node, code, ancestors) =>
            {
                increase_nesting(stats, &mut nesting);
            }
            // Every loop, switch and `@catch` the rows own — fast
            // enumeration folds into `for_statement`, so the `Loop` row
            // already covers it — and the `conditional_expression` the
            // `Ternary` row owns (#1695).
            _ if r#loop::owns_kind(kind, node, code, ancestors)
                || switch::owns_kind(kind, node, code, ancestors)
                || catch::owns_kind(kind, node, code, ancestors)
                || ternary::owns_kind(kind, node, code, ancestors) =>
            {
                increase_nesting(stats, &mut nesting);
            }
            // `Else` here is the `else` keyword token, which the grammar
            // also emits for the `else` of an `else if` — so this arm
            // covers both.
            GotoStatement | Else => {
                increment_by_one(stats);
            }
            // A `binary_expression` (either alias) the `ShortCircuit`
            // row owns (#1695).
            _ if short_circuit::owns_kind(kind, node, code, ancestors) => {
                compute_booleans_from(node, stats, short_circuit::PAYERS);
            }
            // ObjC blocks `^{ … }` are the language's closures — the
            // `Closure` row — and charge the lambda channel.
            _ if closure::owns_kind(kind, node, code, ancestors) => {
                nesting.lambda += 1;
            }
            // Functions and `@implementation` methods — the `FuncOpener`
            // row's callables — are definition boundaries: reset structural
            // nesting and bump the function-depth surcharge when nested
            // inside another (#696), the row's callable kinds the stops.
            _ if func_opener::is_callable_kind(kind, node, code, ancestors) => {
                enter_function_boundary(&mut nesting, node, ancestors, func_opener::CALLABLE);
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
