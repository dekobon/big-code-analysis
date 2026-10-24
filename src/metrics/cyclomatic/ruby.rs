//! `Cyclomatic` implementation for Ruby.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cyclomatic for RubyCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Ruby as R;

        use crate::roles::ruby::{case_arm, guard, null_safe, short_circuit, ternary};
        let kind = node.kind_id();
        // `&&` / `||` / `and` / `or`, the `ShortCircuit` row's payers,
        // the `conditional` the `Ternary` row owns, and an `in` arm's
        // `if` / `unless` guard the `Guard` row owns — a decision the
        // arm it guards does not already pay (#1454; `Guard` the hidden
        // `_guard` supertype, lesson 34), and the safe navigation's `&.`,
        // the `NullSafe` row's payer, once per token (#452) (#1695).
        if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors)
            || ternary::owns_kind(kind, node, code, ancestors)
            || guard::owns_kind(kind, node, code, ancestors)
            || null_safe::pays_at_leaf_kind(kind, node, code, ancestors)
        {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
            return;
        }
        match kind.into() {
            // Standard-only: each arm the `CaseArm` row owns — a `when`
            // of `case … when`, and an `in` of `case … in` unless it is
            // the bare wildcard `in _` with no guard, the default arm
            // (#977), which the row leaves out as Rust's `_` and
            // Python's `case _:` are.
            _ if case_arm::owns_kind(kind, node, code, ancestors) => {
                stats.cyclomatic += 1.;
            }
            // Modified-only: each case container collapses its arms.
            R::Case | R::CaseMatch => {
                stats.cyclomatic_modified += 1.;
            }
            // Both standard and modified. The guards are the `Guard`
            // row's, read above: no double count (§5), since the `if` /
            // `unless` *keyword tokens* inside a guard are anonymous
            // tokens distinct from `R::If` (239) / `R::Unless` (240),
            // the statement nodes — measured, not assumed.
            R::If
            | R::Unless
            | R::Elsif
            | R::IfModifier
            | R::UnlessModifier
            | R::While
            | R::Until
            | R::For
            | R::WhileModifier
            | R::UntilModifier
            | R::Rescue
            | R::RescueModifier
            | R::RescueModifier2
            | R::RescueModifier3 => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
