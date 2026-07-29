//! `Cyclomatic` implementation for iRules.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cyclomatic for IrulesCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        _ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        // Unlike Tcl, iRules has a dedicated `switch`/`switch_arm` node: each
        // non-`default` arm is a decision point in standard CCN, while modified
        // CCN collapses the whole `switch` to a single container decision. The
        // arms are counted at the `switch` node so the `switch_arm` children
        // are not also matched below (which would double-count).
        if let Some(arms) = crate::metrics::cognitive::irules_switch_decision_arms(node, code) {
            stats.cyclomatic += arms as f64;
            stats.cyclomatic_modified += 1.;
            return;
        }
        match node.kind_id().into() {
            // Branches and loops. `DictFor` is an iterating construct; the
            // non-looping `dict update` / `dict with` are intentionally
            // excluded. `Else` carries no condition and is not a branch.
            Irules::If
            | Irules::Elseif
            | Irules::For
            | Irules::Foreach
            | Irules::While
            | Irules::DictFor
            | Irules::Catch
            | Irules::TernaryExpr
            // Short-circuit logical operators, both symbolic and the iRules
            // keyword forms (`and`/`or`) — Tcl has no keyword forms, so these
            // two arms are iRules-specific.
            | Irules::AMPAMP
            | Irules::PIPEPIPE
            | Irules::And
            | Irules::Or => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
