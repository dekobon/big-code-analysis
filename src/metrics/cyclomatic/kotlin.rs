//! `Cyclomatic` implementation for Kotlin.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cyclomatic for KotlinCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Kotlin::*;

        match node.kind_id().into() {
            // Standard-only: individual when entries (arms), except the
            // `else -> …` arm which is Kotlin's analogue of `default:`
            // and must NOT contribute to standard CCN (issue #282 /
            // lesson 11). tree-sitter-kotlin-ng attaches a `condition`
            // field to every case-style entry; the else arm has no
            // `condition` field.
            WhenEntry if !kotlin_when_entry_is_else(node) => {
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
            IfExpression | ForStatement | WhileStatement | DoWhileStatement | CatchBlock
            | AMPAMP | PIPEPIPE | QMARKCOLON | QMARKDOT => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
