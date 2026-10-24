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

        use crate::roles::php::{case_arm, null_safe, short_circuit, ternary};
        let kind = node.kind_id();
        // `&&` / `||` / `and` / `or`, the `ShortCircuit` row's payers,
        // the `conditional_expression` the `Ternary` row owns, and `??`
        // and `?->`, the `NullSafe` row's payers — each once per token,
        // the nullsafe operator across both nullsafe kinds and in a
        // chain (#436) (#1695). `??=` is excluded there as the consumers'
        // own and stays below.
        if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors)
            || ternary::owns_kind(kind, node, code, ancestors)
            || null_safe::pays_at_leaf_kind(kind, node, code, ancestors)
        {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
            return;
        }
        match kind.into() {
            // Standard-only: each switch / match arm the `CaseArm` row
            // owns; the `default` arms are excluded there (#473).
            _ if case_arm::owns_kind(kind, node, code, ancestors) => {
                stats.cyclomatic += 1.;
            }
            // Modified-only: each switch/match container collapses to one
            // decision point.
            SwitchStatement | MatchExpression => {
                stats.cyclomatic_modified += 1.;
            }
            // Both standard and modified. `xor` — the `EagerLogical`
            // row's — is absent: it evaluates both operands, so like `^`
            // it is a value, not a decision (#1536). `??=` is the compound
            // form the `NullSafe` row excludes as the consumers' own: one
            // decision edge, as the bare operator.
            // role-bypass: allow(NullSafe) — the compound `??=`, excluded on the row
            IfStatement | ElseIfClause | ElseIfClause2 | ForStatement | ForeachStatement
            | WhileStatement | DoStatement | CatchClause | QMARKQMARKEQ => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
