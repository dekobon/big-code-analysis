//! `Cyclomatic` implementation for Perl.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cyclomatic for PerlCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use crate::roles::perl::{case_arm, short_circuit, ternary};
        use Perl as P;

        let kind = node.kind_id();
        // `&&` / `||` / `or`, and an `and` that is the operator rather
        // than an auto-quoted key (#1539): the `ShortCircuit` row's
        // payers, the `ternary_expression` the `Ternary` row owns, and
        // the `when` statement the `CaseArm` row owns (#1695). The
        // defined-or `//` and the compound forms stay below until their
        // `NullSafe` row.
        if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors)
            || ternary::owns_kind(kind, node, code, ancestors)
            || case_arm::owns_kind(kind, node, code, ancestors)
        {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
            return;
        }
        match kind.into() {
            P::IfStatement
            | P::UnlessStatement
            | P::ElsifClause
            | P::WhileStatement
            | P::UntilStatement
            | P::ForStatement1
            | P::ForStatement2
            | P::IfSimpleStatement
            | P::UnlessSimpleStatement
            | P::WhileSimpleStatement
            | P::UntilSimpleStatement
            | P::ForSimpleStatement
            | P::SLASHSLASH
            // Compound short-circuit assignments `&&=`, `||=`, `//=`
            // are semantically `x = x op y` and each carries one short-
            // circuit decision edge, parallel to the JS-family fix in
            // #248 (issue #249).
            | P::AMPAMPEQ
            | P::PIPEPIPEEQ
            | P::SLASHSLASHEQ => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
