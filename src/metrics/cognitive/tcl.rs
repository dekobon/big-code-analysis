//! `Cognitive` implementation for Tcl.
#![allow(
    clippy::enum_glob_use,
    clippy::match_same_arms,
    clippy::needless_pass_by_value,
    clippy::wildcard_imports
)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cognitive for TclCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use Tcl::*;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            // Guard kept for defensive consistency with sibling impls; Tcl's dedicated
            // Elseif node means this guard is always true in practice.
            If if !Self::is_else_if(node, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // elseif adds +1 without increasing nesting for its own children.
            Elseif => {
                increment_branch_extension(stats);
            }
            Else => {
                increment_by_one(stats);
            }
            While | Foreach | TernaryExpr => {
                increase_nesting(stats, &mut nesting);
            }
            // `catch` is a conditional error handler; only executes when the body errors.
            Catch => {
                increase_nesting(stats, &mut nesting);
            }
            // Tcl `switch` is a generic `command`, not a dedicated kind, so it
            // would otherwise fall through to `_` (issue #467). It is a
            // switch-like structure: +1 plus current nesting, with the
            // `default` arm free, matching C-family `SwitchStatement` cognitive
            // handling (lesson 11). `tcl_switch_decision_arms` returns `Some`
            // only for a leading-word `switch` command.
            Command if tcl_switch_decision_arms(node, code).is_some() => {
                increase_nesting(stats, &mut nesting);
            }
            BinopExpr => {
                compute_booleans(node, stats, AMPAMP, PIPEPIPE);
            }
            Procedure => {
                enter_function_boundary(&mut nesting, node, ancestors, &[Procedure]);
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
