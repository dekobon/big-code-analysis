//! `Cyclomatic` implementation for Kotlin.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cyclomatic for KotlinCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use crate::roles::kotlin::{case_arm, null_safe, short_circuit};
        use Kotlin::*;

        let kind = node.kind_id();
        // `&&` / `||`, the `ShortCircuit` row's payers, and the elvis `?:`
        // and safe navigation's `?.`, the `NullSafe` row's — each a
        // short-circuit decision once per token (#281, #436) (#1695).
        if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors)
            || null_safe::pays_at_leaf_kind(kind, node, code, ancestors)
        {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
            return;
        }
        match kind.into() {
            // Standard-only: each `when` entry the `CaseArm` row owns —
            // every case-style arm, and not the `else -> …` fallback,
            // Kotlin's `default:` (#282, lesson 11), which the row tells
            // by the absence of a `condition` field.
            _ if case_arm::owns_kind(kind, node, code, ancestors) => {
                stats.cyclomatic += 1.;
            }
            // Modified-only: the when expression container.
            WhenExpression => {
                stats.cyclomatic_modified += 1.;
            }
            // Both standard and modified.
            //
            // Kotlin's Elvis operator `?:` (`QMARKCOLON`) is a short-circuit
            // nullish operator analogous to JS `??` and each occurrence is a
            // distinct decision point, mirroring `&&` / `||`.
            //
            // Safe-navigation `?.` (`QMARKDOT`) is short-circuit too — it
            // skips the member access when the LHS is null — so each
            // occurrence is one decision point, mirroring the JS/C#
            // treatment of `?.` (issues #281, #436). The grammar emits the
            // `?.` token (`QMARKDOT`, id 140) once per operator inside a
            // `navigation_expression`, so matching the token counts each
            // textual `?.` exactly once, including in chains (`a?.b?.c` is
            // +2), and parallels TS/TSX which also match the `QMARKDOT`
            // token rather than the wrapper node.
            IfExpression | ForStatement | WhileStatement | DoWhileStatement | CatchBlock => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
