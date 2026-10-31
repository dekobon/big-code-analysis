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
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        // bca: suppress(cyclomatic, halstead) — exhaustive grammar dispatch
        // One arm per construct the grammar can hand us, each a row read
        // or a kind the rows do not own: the count is the construct list,
        // not branching a reader must hold, and the effort is the table's
        // vocabulary (#1695, #1697).
        use crate::roles::irules::{
            branch, catch, else_if, func_opener, r#loop, short_circuit, switch, ternary,
        };
        use Irules::*;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        let kind = node.kind_id();

        match kind.into() {
            // An `if` the `Branch` row owns opens a level. iRules' `ElseIf`
            // row owns the dedicated `elseif` node, never an `if`, so the
            // two rows are disjoint and no exclusion is needed here (#1697).
            _ if branch::owns_kind(kind, node, code, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // The `elseif` the `ElseIf` row pays at — its leaf, the clause
            // itself — extends the branch: +1 without increasing nesting.
            _ if else_if::pays_at_leaf_kind(kind, node, code, ancestors) => {
                increment_branch_extension(stats);
            }
            Else => {
                increment_by_one(stats);
            }
            // Loops and ternary. `DictFor` iterates; `dict update`/`dict with`
            // do not and are excluded.
            // Every loop the `Loop` row owns (#1697).
            _ if r#loop::owns_kind(kind, node, code, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // The `ternary_expr` the `Ternary` row owns (#1695).
            _ if ternary::owns_kind(kind, node, code, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // `catch`, and a `try`'s `on error` / `trap` handler groups — the
            // `Catch` row's owners — are conditional error paths: +1 plus
            // current nesting, and the handler body nests (#1266); `Try`
            // itself and `Finally` are unconditional and stay free. And the
            // `switch` the `Switch` row owns, a dedicated node unlike Tcl's:
            // +1 plus current nesting, the `default` arm free (lesson 11).
            _ if catch::owns_kind(kind, node, code, ancestors)
                || switch::owns_kind(kind, node, code, ancestors) =>
            {
                increase_nesting(stats, &mut nesting);
            }
            // Boolean sequences inside expressions: both symbolic (`&&`/`||`)
            // and the iRules keyword forms (`and`/`or`), each keyword keyed
            // to the symbol it aliases so a mixed spelling stays one
            // sequence (#1530).
            // A `binop_expr` the `ShortCircuit` row owns (#1695); which of
            // its payers share a key is this consumer's reading.
            // role-bypass: allow(ShortCircuit) — the sequence key the row's payers share, not
            // membership: the row is read on the arm's guard
            _ if short_circuit::owns_kind(kind, node, code, ancestors) => {
                compute_booleans_keyed(node, stats, |id| match id.into() {
                    AMPAMP | And => Some(AMPAMP as u16),
                    PIPEPIPE | Or => Some(PIPEPIPE as u16),
                    _ => None,
                });
            }
            // The `FuncOpener` row's callable owners — a `proc` and an
            // event handler — reset structural nesting and bump the
            // function-depth surcharge when nested in another (#696); the
            // stop list is the row's callable kinds.
            _ if func_opener::is_callable_kind(kind, node, code, ancestors) => {
                enter_function_boundary(&mut nesting, node, ancestors, func_opener::CALLABLE);
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
