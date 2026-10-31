//! `Cyclomatic` implementation for Tcl.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;
use crate::lang_helpers::tcl::tcl_try_handler_bodies;

impl Cyclomatic for TclCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        // bca: suppress(halstead) — one row read per construct; the effort
        // is the rows' vocabulary, not work a reader must hold
        use crate::roles::tcl::{
            branch, case_arm, catch, else_if, r#loop, short_circuit, switch, ternary,
        };
        let kind = node.kind_id();
        // One decision in both tiers: `&&` / `||`, the `ShortCircuit`
        // row's payers; the `ternary_expr` the `Ternary` row owns
        // (#1695); the `if` the `Branch` row owns; and the `elseif`
        // clause, which the `ElseIf` row marks as paying for itself — the
        // grammar gives it a node, not a nested `if` (#1697).
        if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors)
            || ternary::owns_kind(kind, node, code, ancestors)
            || branch::owns_kind(kind, node, code, ancestors)
            || else_if::pays_at_leaf_kind(kind, node, code, ancestors)
        {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
            return;
        }
        // The `Catch` row: `catch` is one decision; a `try` is free itself
        // and pays one per `on error` handler in both tiers (#1266),
        // matching the C-family `CatchClause` and the iRules `OnHandler`
        // — modified CCN collapses only switch-like arm lists, never
        // catch handlers. `finally` is unconditional cleanup and stays
        // free. The row owns a `try` only when it has a handler, so the
        // handler count below is never zero — and at this pin never more
        // than one either: the grammar admits a single `on error` and no
        // `trap`, so a second handler parses to `ERROR`, and a
        // perturbation to a flat `1.` is caught by no parseable fixture.
        // The count is kept for the bump that adds them.
        if catch::owns_kind(kind, node, code, ancestors) {
            // role-bypass: allow(Catch) — which of the row's two owners
            // pays per handler group rather than once
            let decisions = if kind == Tcl::Try as u16 {
                tcl_try_handler_bodies(node).count() as f64
            } else {
                1.
            };
            stats.cyclomatic += decisions;
            stats.cyclomatic_modified += decisions;
            return;
        }
        // No `return` here, deliberately. Standard-only: each arm of a
        // brace-list `switch` the `CaseArm` row owns — a nested `command`
        // told by its position and its parent's leading word, the
        // `default` arm left out (lesson 11), and the unsupported split
        // form owning nothing (#467). An arm is a command too, so the
        // `Loop` and `Switch` reads below still see its leading word: an
        // arm whose pattern spells `for` or `switch` pays as that command
        // as well, as it always has — a reading the grammar cannot tell
        // from the command, left as is.
        if case_arm::owns_kind(kind, node, code, ancestors) {
            stats.cyclomatic += 1.;
        }
        // One loop decision in both tiers: the `Loop` row's `foreach` /
        // `while`, and the `for` the grammar spells as a generic
        // `command` told by its leading word (#1264), matching the
        // dedicated iRules `For`.
        if r#loop::owns_kind(kind, node, code, ancestors) {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
        }
        // Modified-only: the brace-list `switch` the `Switch` row owns
        // collapses to one container decision (its arms are read above,
        // each at its own node). The split form has no arm list, so the
        // row declines it and it stays uncounted in both tiers (#467).
        if switch::owns_kind(kind, node, code, ancestors) {
            stats.cyclomatic_modified += 1.;
        }
    }
}
