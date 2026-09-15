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
            //
            // `IfGuard` / `UnlessGuard` are the two guard spellings of a
            // `case … in` pattern arm (`in [x] if x > 2`), and each is a
            // decision the arm does not already pay for (#1454,
            // transferring #1422's C# rule): a guarded arm fails two
            // ways — the pattern does not match, or it matches and the
            // guard is false. Unlike `InClause`, the guard is not
            // collapsed by the `case` container, so it counts toward
            // modified as well, exactly as C#'s `when_clause` does.
            //
            // `Guard` (210) is the hidden `_guard` supertype the parser
            // never emits; it is listed defensively beside the two
            // concrete kinds, as `ruby_in_clause_counts` already lists
            // it (grammar-dispatch §2, lesson #34). Its hidden status is
            // pinned by an `ast_has_kind_id` assertion in the
            // `ruby_in_clause_guard_*` tests.
            //
            // No double count (§5): the `if` / `unless` *keyword tokens*
            // inside a guard are anonymous tokens distinct from
            // `R::If` (239) / `R::Unless` (240), which are the statement
            // nodes — measured, not assumed: before this arm a guarded
            // `in` arm scored exactly what its unguarded control did.
            R::Guard
            | R::IfGuard
            | R::UnlessGuard
            | R::If
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
