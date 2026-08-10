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
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use Irules::*;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            // Defensive guard for parity with sibling impls; iRules' dedicated
            // `Elseif` node means `is_else_if` is never true for an `If`.
            If if !Self::is_else_if(node, ancestors) => {
                increase_nesting(stats, &mut nesting);
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
                increase_nesting(stats, &mut nesting);
            }
            // `catch` is a conditional error handler; its body only runs when
            // the guarded command errors.
            Catch => {
                increase_nesting(stats, &mut nesting);
            }
            // `try` handlers are conditional error paths too: +1 plus current
            // nesting, and the handler body nests (issue #1266). The grammar
            // wraps each in a dedicated node (unlike Tcl's flat tokens), and
            // emits them only inside `try` — they are catch clauses, not the
            // `when`-style event handlers they were once mistaken for. `Try`
            // itself and `Finally` are unconditional and stay free.
            OnHandler | TrapHandler => {
                increase_nesting(stats, &mut nesting);
            }
            // iRules `switch` is a dedicated node (unlike Tcl): +1 plus current
            // nesting, with the `default` arm free, matching the C-family
            // `SwitchStatement` cognitive handling (lesson 11).
            Switch => {
                increase_nesting(stats, &mut nesting);
            }
            // Boolean sequences inside expressions: both symbolic (`&&`/`||`)
            // and the iRules keyword forms (`and`/`or`).
            BinopExpr => {
                compute_booleans_with(node, stats, |id| {
                    matches!(id.into(), AMPAMP | PIPEPIPE | And | Or)
                });
            }
            // The two function-space kinds reset nesting and bump the
            // function depth (see the `IrulesCode` Checker impl).
            Procedure | WhenEvent => {
                enter_function_boundary(&mut nesting, node, ancestors, &[Procedure, WhenEvent]);
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
