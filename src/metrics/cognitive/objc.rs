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
        use crate::roles::objc::{closure, func_opener, short_circuit, ternary};
        use Objc::*;

        // Macro expansion is not tracked; macros are treated as opaque tokens.
        let mut nesting = get_nesting_from_map(node, nesting_map);

        let kind = node.kind_id();

        match kind.into() {
            IfStatement if !Self::is_else_if(node, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // `@catch` (the `catch_clause` node) nests like a C++ catch.
            // Fast enumeration folds into `for_statement`, so `ForStatement`
            // already covers it.
            ForStatement | WhileStatement | DoStatement | SwitchStatement | CatchClause => {
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
            // A `binary_expression` (either alias) the `ShortCircuit`
            // row owns (#1695).
            _ if short_circuit::owns_kind(kind, node, code, ancestors) => {
                compute_booleans_from(node, stats, short_circuit::PAYERS);
            }
            // ObjC blocks `^{ … }` are the language's closures — the
            // `Closure` row — and charge the lambda channel.
            _ if closure::owns(node, code, ancestors) => {
                nesting.lambda += 1;
            }
            // Functions and `@implementation` methods — the `FuncOpener`
            // row's callables — are definition boundaries: reset structural
            // nesting and bump the function-depth surcharge when nested
            // inside another (#696), the row's callable kinds the stops.
            _ if func_opener::is_callable(node, code, ancestors) => {
                enter_function_boundary(&mut nesting, node, ancestors, func_opener::CALLABLE);
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
