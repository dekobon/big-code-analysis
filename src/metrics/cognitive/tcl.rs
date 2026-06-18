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
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Tcl::*;

        let (mut nesting, mut depth, lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            // Guard kept for defensive consistency with sibling impls; Tcl's dedicated
            // Elseif node means this guard is always true in practice.
            If if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // elseif adds +1 without increasing nesting for its own children.
            Elseif => {
                increment_branch_extension(stats);
            }
            Else => {
                increment_by_one(stats);
            }
            While | Foreach | TernaryExpr => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // `catch` is a conditional error handler; only executes when the body errors.
            Catch => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // Tcl `switch` is a generic `command`, not a dedicated kind, so it
            // would otherwise fall through to `_` (issue #467). It is a
            // switch-like structure: +1 plus current nesting, with the
            // `default` arm free, matching C-family `SwitchStatement` cognitive
            // handling (lesson 11). `tcl_switch_decision_arms` returns `Some`
            // only for a leading-word `switch` command.
            Command if tcl_switch_decision_arms(node, code).is_some() => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            BinopExpr => {
                compute_booleans(node, stats, AMPAMP, PIPEPIPE);
            }
            Procedure => {
                nesting = 0;
                increment_function_depth(&mut depth, node, &[Procedure]);
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}
