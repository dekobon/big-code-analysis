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
        _code: &'a [u8],
        _ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Perl as P;

        match node.kind_id().into() {
            P::IfStatement
            | P::UnlessStatement
            | P::ElsifClause
            | P::WhileStatement
            | P::UntilStatement
            | P::ForStatement1
            | P::ForStatement2
            | P::WhenSimpleStatement
            | P::IfSimpleStatement
            | P::UnlessSimpleStatement
            | P::WhileSimpleStatement
            | P::UntilSimpleStatement
            | P::ForSimpleStatement
            | P::AMPAMP
            | P::PIPEPIPE
            | P::SLASHSLASH
            // Compound short-circuit assignments `&&=`, `||=`, `//=`
            // are semantically `x = x op y` and each carries one short-
            // circuit decision edge, parallel to the JS-family fix in
            // #248 (issue #249).
            | P::AMPAMPEQ
            | P::PIPEPIPEEQ
            | P::SLASHSLASHEQ
            | P::And
            | P::Or
            | P::TernaryExpression => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
