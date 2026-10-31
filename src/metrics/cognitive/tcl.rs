//! `Cognitive` implementation for Tcl.
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
use crate::lang_helpers::tcl::tcl_try_handler_bodies;

impl Cognitive for TclCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        // bca: suppress(cyclomatic, halstead) — exhaustive kind dispatch table
        // One arm per grammar kind, like `CppCode::compute`: the count is
        // the number of node kinds the grammar can hand us, not branching
        // a reader must hold, and the effort is the table's vocabulary;
        // the row reads (#1695) are arms like the kind arms they replaced.
        use crate::roles::tcl::{
            branch, catch, else_if, func_opener, r#loop, short_circuit, switch, ternary,
        };
        use Tcl::*;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        let kind = node.kind_id();

        match kind.into() {
            // The `if` the `Branch` row owns. No `is_else_if` exclusion:
            // the `ElseIf` row owns the dedicated `elseif` node, never an
            // `if`, so the two rows are disjoint by construction.
            _ if branch::owns_kind(kind, node, code, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // The `elseif` clause the `ElseIf` row owns adds +1 without
            // increasing nesting for its own children.
            _ if else_if::owns_kind(kind, node, code, ancestors) => {
                increment_branch_extension(stats);
            }
            Else => {
                increment_by_one(stats);
            }
            // The `Loop` row: `while` / `foreach`, and the `for` the
            // grammar spells as a generic `command` told by its leading
            // word (#1264), matching the dedicated iRules `For`. `for` has
            // a second, unrepairable consequence: with no grammar rule
            // there is no `expr` slot, so the loop condition (`{$i < $n}`)
            // stays an opaque `braced_word` and its comparison never
            // surfaces as a condition token for ABC — a grammar
            // limitation, not a metric-layer bug.
            _ if r#loop::owns_kind(kind, node, code, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // The `ternary_expr` the `Ternary` row owns (#1695).
            _ if ternary::owns_kind(kind, node, code, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // The brace-list `switch` the `Switch` row owns — a generic
            // `command` told by its leading word (#467): +1 plus current
            // nesting, with the `default` arm free, matching C-family
            // `SwitchStatement` cognitive handling (lesson 11). The row
            // declines the unsupported split form, the same scoping
            // cyclomatic applies; cognitive needs no arm count.
            _ if switch::owns_kind(kind, node, code, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // The `Catch` row. `catch` is a conditional error handler and
            // only executes when the body errors: +nesting at the node. A
            // `try` itself is free; each `on error` handler is a
            // conditional error path: +1 plus current nesting, nesting
            // only the handler body (#1266). The `try` body and `finally`
            // run unconditionally and stay at the inherited level,
            // matching the C-family split of a free `try` and a charged
            // `CatchClause`. The grammar exposes no handler wrapper node,
            // so the body's slot is seeded here directly — the walker's
            // `propagate_nesting_to_children` uses `or_insert`, so a slot
            // written by the parent survives propagation (the #421
            // comprehension-clause pattern). The row owns a `try` only
            // when it has a handler.
            _ if catch::owns_kind(kind, node, code, ancestors) => {
                // role-bypass: allow(Catch) — which of the row's two owners
                // nests per handler body rather than at the node
                if kind == Try as u16 {
                    for body in tcl_try_handler_bodies(node) {
                        let mut handler_nesting = nesting;
                        increase_nesting(stats, &mut handler_nesting);
                        nesting_map.insert(body.id(), handler_nesting);
                    }
                } else {
                    increase_nesting(stats, &mut nesting);
                }
            }
            // A `binop_expr` the `ShortCircuit` row owns (#1695).
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
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
