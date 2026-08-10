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

impl Cognitive for TclCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use Tcl::*;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
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
            While | Foreach | TernaryExpr => {
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
            BinopExpr => {
                compute_booleans(node, stats, AMPAMP, PIPEPIPE);
            }
            Procedure => {
                enter_function_boundary(&mut nesting, node, ancestors, &[Procedure]);
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
