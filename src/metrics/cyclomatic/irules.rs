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
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use crate::roles::irules::{case_arm, short_circuit, ternary};
        let kind = node.kind_id();
        // Unlike Tcl, iRules has a dedicated `switch` / `switch_arm` node:
        // each arm the `CaseArm` row owns — every one but `default` — is
        // a decision point in standard CCN, read at its own node, while
        // modified CCN collapses the whole `switch` to one container
        // decision.
        if case_arm::owns_kind(kind, node, code, ancestors) {
            stats.cyclomatic += 1.;
            return;
        }
        if kind == Irules::Switch as u16 {
            stats.cyclomatic_modified += 1.;
            return;
        }
        // The short-circuit operators, symbolic and the iRules keyword
        // forms (`and` / `or`) — the `ShortCircuit` row's payers — and
        // the `ternary_expr` the `Ternary` row owns (#1695).
        if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors)
            || ternary::owns_kind(kind, node, code, ancestors)
        {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
            return;
        }
        match kind.into() {
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
            // `try` handlers: the grammar wraps each `on error` / `trap`
            // group in a dedicated node (unlike Tcl's flat tokens), so each
            // is one decision point here (issue #1266), matching `Catch`
            // and the C-family `CatchClause`. `Try` itself and `Finally`
            // are unconditional and stay free.
            | Irules::OnHandler
            | Irules::TrapHandler => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
