//! `Cyclomatic` implementation for Tcl.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;
use crate::lang_helpers::case_arms::tcl_switch_arm_list;

impl Cyclomatic for TclCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use crate::roles::tcl::{case_arm, short_circuit, ternary};
        let kind = node.kind_id();
        // `&&` / `||`, the `ShortCircuit` row's payers, and the
        // `ternary_expr` the `Ternary` row owns (#1695).
        if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors)
            || ternary::owns_kind(kind, node, code, ancestors)
        {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
            return;
        }
        // Standard-only: each arm of a brace-list `switch` the `CaseArm`
        // row owns — a nested `command` told by its position and its
        // parent's leading word, the `default` arm left out (lesson 11),
        // and the unsupported split form owning nothing (#467).
        if case_arm::owns_kind(kind, node, code, ancestors) {
            stats.cyclomatic += 1.;
            return;
        }
        match kind.into() {
            Tcl::If | Tcl::Elseif | Tcl::Foreach | Tcl::While | Tcl::Catch => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            // Tcl `switch` and `for` are generic `command`s with no dedicated
            // kind (issues #467, #1264), so the kind dispatch above never
            // sees them. The leading word is resolved once and dispatched on.
            // role-bypass: allow(CaseArm) — the command-name dispatch for `switch` / `for`, which
            // the row's switch-arm owner is also a kind of; the arms are read above
            Tcl::Command => match crate::lang_helpers::tcl::tcl_command_name(node, code) {
                // Modified-only: the brace-list `switch` collapses to one
                // container decision (its arms are read above, each at its
                // own node). The unsupported split form has no arm list
                // and stays uncounted in both tiers.
                Some("switch") => {
                    if tcl_switch_arm_list(node).is_some() {
                        stats.cyclomatic_modified += 1.;
                    }
                }
                // One loop decision in both tiers, matching `Foreach`/`While`
                // above and the dedicated iRules `For`.
                Some("for") => {
                    stats.cyclomatic += 1.;
                    stats.cyclomatic_modified += 1.;
                }
                _ => {}
            },
            // `try` itself is free; each `on error` handler is one decision
            // point in both tiers (issue #1266), matching `Catch` above, the
            // C-family `CatchClause`, and the iRules `OnHandler` — modified
            // CCN collapses only switch-like arm lists, never catch
            // handlers. `finally` is unconditional cleanup and stays free.
            Tcl::Try => {
                let handlers =
                    crate::metrics::cognitive::tcl_try_handler_bodies(node).count() as f64;
                stats.cyclomatic += handlers;
                stats.cyclomatic_modified += handlers;
            }
            _ => {}
        }
    }
}
