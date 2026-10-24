//! `Cyclomatic` implementation for C#.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cyclomatic for CsharpCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use crate::roles::csharp::{case_arm, guard, short_circuit, ternary};
        use Csharp::*;

        let kind = node.kind_id();
        // `&&` / `||`, the `ShortCircuit` row's payers, the
        // `conditional_expression` the `Ternary` row owns, and the two
        // guard clauses the `Guard` row owns — `when_clause` and
        // `catch_filter_clause`, each a decision the construct it guards
        // does not already pay for (#1422), so a guarded discard scores
        // two: the arm and the guard that makes it conditional (#1695).
        // `??` / `??=` stay below until their `NullSafe` row.
        if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors)
            || ternary::owns_kind(kind, node, code, ancestors)
            || guard::owns_kind(kind, node, code, ancestors)
        {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
            return;
        }
        // Standard-only: a switch arm, read at the `CaseArm` row's leaf —
        // each `case` label of a statement arm (gated by the row to the
        // arm, since `goto case 2;` spells the same token, #1450) and
        // each expression arm but the bare discard `_ =>`, C#'s
        // `default:` (#282, lesson 11); `default:` is excluded on the row.
        if case_arm::pays_at_leaf_kind(kind, node, code, ancestors) {
            stats.cyclomatic += 1.;
            return;
        }
        match kind.into() {
            // Modified-only: the switch statement and switch expression
            // containers each collapse to one decision point.
            SwitchStatement | SwitchExpression => {
                stats.cyclomatic_modified += 1.;
            }
            // Both standard and modified. The guard clauses are the
            // `Guard` row's, read above: `When` (119) is also a
            // `_reserved_identifier` at this pin, so `int when = 1;` emits
            // it under an `identifier` and the token would score a
            // decision per mention of the variable (`bca dump`, not
            // inferred) — which is why the row's owners are the clause
            // nodes and the token is excluded.
            IfStatement
            | ForStatement
            | ForeachStatement
            | WhileStatement
            | DoStatement
            | CatchClause
            | ConditionalAccessExpression
            | QMARKQMARK
            | QMARKQMARKEQ => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
