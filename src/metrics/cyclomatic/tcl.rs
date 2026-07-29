//! `Cyclomatic` implementation for Tcl.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cyclomatic for TclCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        _ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        // Tcl `switch` is a generic `command`, not a dedicated kind, so it is
        // matched out-of-band before the kind dispatch (issue #467). Mirroring
        // the C-family convention (see `impl_cyclomatic_c_family`): each
        // non-`default` arm is a decision point in standard CCN, while modified
        // CCN collapses the whole construct to a single container decision.
        if let Some(arms) = crate::metrics::cognitive::tcl_switch_decision_arms(node, code) {
            stats.cyclomatic += arms as f64;
            stats.cyclomatic_modified += 1.;
            return;
        }
        match node.kind_id().into() {
            Tcl::If
            | Tcl::Elseif
            | Tcl::Foreach
            | Tcl::While
            | Tcl::Catch
            | Tcl::TernaryExpr
            | Tcl::AMPAMP
            | Tcl::PIPEPIPE => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
