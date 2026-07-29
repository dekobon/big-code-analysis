//! `Cyclomatic` implementation for Go.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cyclomatic for GoCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        _ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        // Aliased because `Go::Go` (the `go` keyword variant) collides with
        // the bare enum name in pattern position under `use Go::*;`.
        use Go as G;

        match node.kind_id().into() {
            // Standard-only: individual case arms inside switch/select.
            G::ExpressionCase | G::TypeCase | G::CommunicationCase => {
                stats.cyclomatic += 1.;
            }
            // Modified-only: each distinct switch/select container collapses
            // all its arms into one decision point.
            G::ExpressionSwitchStatement | G::TypeSwitchStatement | G::SelectStatement => {
                stats.cyclomatic_modified += 1.;
            }
            // Both standard and modified.
            G::IfStatement | G::ForStatement | G::AMPAMP | G::PIPEPIPE => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
