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
        _ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Ruby as R;

        match node.kind_id().into() {
            // Standard-only: each `when` arm of an ordinary `case … when`.
            R::When => {
                stats.cyclomatic += 1.;
            }
            // Standard-only: a `case … in` pattern-match arm, but only when
            // it is a real decision — the bare wildcard `in _` (no guard)
            // is the default arm and adds nothing, matching Rust's `_`
            // `MatchArm` and Python's `case _:` filters (#977).
            R::InClause if crate::metrics::npa::ruby_in_clause_counts(node, code) => {
                stats.cyclomatic += 1.;
            }
            // Modified-only: each case container collapses its arms.
            R::Case | R::CaseMatch => {
                stats.cyclomatic_modified += 1.;
            }
            // Both standard and modified.
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
            | R::RescueModifier3
            | R::Conditional
            | R::AMPAMP
            | R::PIPEPIPE
            | R::And
            | R::Or
            // Safe-navigation `&.` (`AMPDOT`) is short-circuit — it
            // skips the method call when the receiver is nil — so each
            // occurrence is one decision point, mirroring the
            // Kotlin/PHP/JS/C# treatment of `?.` (issues #281, #452).
            // The grammar emits the `&.` token once per operator inside
            // a `call` node, so matching the token counts each textual
            // `&.` exactly once, including in chains (`a&.b&.c` is +2),
            // paralleling Kotlin's `QMARKDOT` token approach.
            | R::AMPDOT => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
