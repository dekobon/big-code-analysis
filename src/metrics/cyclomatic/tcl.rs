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
            // Tcl `switch` and `for` are generic `command`s with no dedicated
            // kind (issues #467, #1264), so the kind dispatch above never
            // sees them. The leading word is resolved once and dispatched on.
            Tcl::Command => match crate::metrics::cognitive::tcl_command_name(node, code) {
                // Mirroring the C-family convention (see
                // `impl_cyclomatic_c_family`): each non-`default` arm is a
                // decision point in standard CCN, while modified CCN
                // collapses the whole construct to a single container
                // decision. The unsupported split form yields `None` and
                // stays uncounted in both tiers.
                Some("switch") => {
                    if let Some(arms) =
                        crate::metrics::cognitive::tcl_switch_decision_arms(node, code)
                    {
                        stats.cyclomatic += arms as f64;
                        stats.cyclomatic_modified += 1.;
                    }
                }
                // One loop decision in both tiers, matching `Foreach`/`While`
                // above and the dedicated iRules `For`.
                Some("for") => {
                    stats.cyclomatic += 1.;
                    stats.cyclomatic_modified += 1.;
                }
                _ => {}
            },
            // `try` itself is free; each `on error` handler is one decision
            // point in both tiers (issue #1266), matching `Catch` above, the
            // C-family `CatchClause`, and the iRules `OnHandler` — modified
            // CCN collapses only switch-like arm lists, never catch
            // handlers. `finally` is unconditional cleanup and stays free.
            Tcl::Try => {
                let handlers =
                    crate::metrics::cognitive::tcl_try_handler_bodies(node).count() as f64;
                stats.cyclomatic += handlers;
                stats.cyclomatic_modified += handlers;
            }
            _ => {}
        }
    }
}
