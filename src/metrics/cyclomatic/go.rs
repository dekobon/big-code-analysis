//! `Cyclomatic` implementation for Go.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cyclomatic for GoCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        // Aliased because `Go::Go` (the `go` keyword variant) collides with
        // the bare enum name in pattern position under `use Go::*;`.
        use crate::roles::go::{case_arm, short_circuit};
        use Go as G;

        let kind = node.kind_id();
        // `&&` / `||`, the `ShortCircuit` row's payers (#1695).
        if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors) {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
            return;
        }
        match kind.into() {
            // Standard-only: each switch / select arm the `CaseArm` row
            // owns; the `default_case` is excluded there (lesson 11).
            _ if case_arm::owns_kind(kind, node, code, ancestors) => {
                stats.cyclomatic += 1.;
            }
            // Modified-only: each distinct switch/select container collapses
            // all its arms into one decision point.
            G::ExpressionSwitchStatement | G::TypeSwitchStatement | G::SelectStatement => {
                stats.cyclomatic_modified += 1.;
            }
            // Both standard and modified.
            G::IfStatement | G::ForStatement => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
