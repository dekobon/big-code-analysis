//! `Cyclomatic` implementation for PHP.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cyclomatic for PhpCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Php::*;

        use crate::roles::php::short_circuit;
        let kind = node.kind_id();
        // `&&` / `||` / `and` / `or`, the `ShortCircuit` row's payers
        // (#1695); `??` / `??=` / `?->` stay below until their `NullSafe`
        // row.
        if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors) {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
            return;
        }
        match kind.into() {
            // Standard-only: individual case arms in switch/match.
            CaseStatement | MatchConditionalExpression => {
                stats.cyclomatic += 1.;
            }
            // Modified-only: each switch/match container collapses to one
            // decision point.
            SwitchStatement | MatchExpression => {
                stats.cyclomatic_modified += 1.;
            }
            // Both standard and modified. `xor` is absent: it evaluates
            // both operands, so like `^` it is a value, not a decision
            // (#1536).
            IfStatement
            | ElseIfClause
            | ElseIfClause2
            | ForStatement
            | ForeachStatement
            | WhileStatement
            | DoStatement
            | ConditionalExpression
            | CatchClause
            | QMARKQMARK
            | QMARKQMARKEQ
            // Nullsafe operator `?->` (`QMARKDASHGT`) is short-circuit — it
            // skips the member access/call when the LHS is null — so each
            // occurrence is one decision point, mirroring the JS/C#
            // treatment of `?.` (issues #281, #436). Matching the `?->`
            // token (`QMARKDASHGT`, id 129) counts each operator exactly
            // once across BOTH `nullsafe_member_access_expression`
            // (property access) and `nullsafe_member_call_expression`
            // (method call), and in chains (`$a?->b?->c` is +2). Matching
            // either node kind instead would miss the other form and could
            // double-count nested access/call; the token is the single
            // granularity that fires once per textual `?->`, paralleling
            // TS/TSX's `QMARKDOT` token approach.
            | QMARKDASHGT => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
