//! `Cognitive` implementation for iRules.
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

impl Cognitive for IrulesCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use Irules::*;

        let (mut nesting, mut depth, lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            // Defensive guard for parity with sibling impls; iRules' dedicated
            // `Elseif` node means `is_else_if` is never true for an `If`.
            If if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // `elseif` extends the chain: +1 without increasing nesting.
            Elseif => {
                increment_branch_extension(stats);
            }
            Else => {
                increment_by_one(stats);
            }
            // Loops and ternary. `DictFor` iterates; `dict update`/`dict with`
            // do not and are excluded.
            For | Foreach | While | DictFor | TernaryExpr => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // `catch` is a conditional error handler; its body only runs when
            // the guarded command errors.
            Catch => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // iRules `switch` is a dedicated node (unlike Tcl): +1 plus current
            // nesting, with the `default` arm free, matching the C-family
            // `SwitchStatement` cognitive handling (lesson 11).
            Switch => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // Boolean sequences inside expressions: both symbolic (`&&`/`||`)
            // and the iRules keyword forms (`and`/`or`).
            BinopExpr => {
                compute_booleans_with(node, stats, |id| {
                    matches!(id.into(), AMPAMP | PIPEPIPE | And | Or)
                });
            }
            // All four function-space kinds reset nesting and bump the
            // function depth (see the `IrulesCode` Checker impl).
            Procedure | WhenEvent | OnHandler | TrapHandler => {
                nesting = 0;
                increment_function_depth(
                    &mut depth,
                    node,
                    &[Procedure, WhenEvent, OnHandler, TrapHandler],
                );
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}
