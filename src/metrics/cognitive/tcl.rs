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
use crate::lang_helpers::tcl_switch_arm_list;

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
        use crate::roles::tcl::{func_opener, short_circuit, ternary};
        use Tcl::*;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        let kind = node.kind_id();

        match kind.into() {
            // Guard kept for defensive consistency with sibling impls; Tcl's dedicated
            // Elseif node means this guard is always true in practice.
            If if !Self::is_else_if(node, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // elseif adds +1 without increasing nesting for its own children.
            Elseif => {
                increment_branch_extension(stats);
            }
            Else => {
                increment_by_one(stats);
            }
            While | Foreach => {
                increase_nesting(stats, &mut nesting);
            }
            // The `ternary_expr` the `Ternary` row owns (#1695).
            _ if ternary::owns_kind(kind, node, code, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // `catch` is a conditional error handler; only executes when the body errors.
            Catch => {
                increase_nesting(stats, &mut nesting);
            }
            // `try` itself is free; each `on error` handler is a conditional
            // error path: +1 plus current nesting, nesting only the handler
            // body (issue #1266). The `try` body and `finally` run
            // unconditionally and stay at the inherited level, matching the
            // C-family split of a free `try` and a charged `CatchClause`.
            // The grammar exposes no handler wrapper node, so the body's
            // slot is seeded here directly — the walker's
            // `propagate_nesting_to_children` uses `or_insert`, so a slot
            // written by the parent survives propagation (the #421
            // comprehension-clause pattern).
            Try => {
                for body in tcl_try_handler_bodies(node) {
                    let mut handler_nesting = nesting;
                    increase_nesting(stats, &mut handler_nesting);
                    nesting_map.insert(body.id(), handler_nesting);
                }
            }
            // Tcl `switch` and `for` are generic `command`s, not dedicated
            // kinds, so they would otherwise fall through to `_` (issues
            // #467, #1264). The leading word is resolved once here and
            // dispatched on; the two constructs used to re-resolve it
            // independently.
            //
            // `for` has a second, unrepairable consequence: with no grammar
            // rule there is no `expr` slot, so the loop condition
            // (`{$i < $n}`) stays an opaque `braced_word` and its comparison
            // never surfaces as a condition token for ABC — a grammar
            // limitation, not a metric-layer bug.
            // role-bypass: allow(CaseArm) — the command-name dispatch for `switch` / `for` /
            // `try`, which the row's switch-arm owner is also a kind of
            Command => match tcl_command_name(node, code) {
                // A switch-like structure: +1 plus current nesting, with the
                // `default` arm free, matching C-family `SwitchStatement`
                // cognitive handling (lesson 11). The arm list must be the
                // supported brace-list form, the same scoping cyclomatic
                // applies; cognitive needs no arm count.
                Some("switch") if tcl_switch_arm_list(node).is_some() => {
                    increase_nesting(stats, &mut nesting);
                }
                // A loop adds +1 plus current nesting and nests its body,
                // matching `While`/`Foreach` above and the dedicated iRules
                // `For`.
                Some("for") => {
                    increase_nesting(stats, &mut nesting);
                }
                _ => {}
            },
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
