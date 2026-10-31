//! `Cognitive` implementation for Lua.
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

impl Cognitive for LuaCode {
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
        use crate::roles::lua::{branch, closure, else_if, func_opener, r#loop, short_circuit};
        use Lua::*;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        let kind = node.kind_id();

        match kind.into() {
            // An `if` the `Branch` row owns opens a level. Lua's `ElseIf` row
            // owns the dedicated `elseif_statement`, never an `if`, so the
            // two rows are disjoint and no exclusion is needed here (#1697).
            _ if branch::owns_kind(kind, node, code, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // `elseif` adds +1 at the same nesting level as the parent `if`,
            // matching how Tcl/Bash handle their dedicated elseif/elif nodes.
            // The `elseif` statement the `ElseIf` row pays at — its leaf,
            // the clause itself — extends the branch at the same level.
            _ if else_if::pays_at_leaf_kind(kind, node, code, ancestors) => {
                increment_branch_extension(stats);
            }
            // Every loop the `Loop` row owns (#1697).
            _ if r#loop::owns_kind(kind, node, code, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // `else` increments without nesting. Lua's `break` is always
            // unlabeled (the grammar has no labeled break, and no
            // `continue`), so per SonarSource Cognitive Complexity §B2 it
            // adds +0 — the enclosing loop's nesting already accounts for
            // it. Only `goto label` is a genuinely unstructured jump and
            // adds +1.
            ElseStatement | GotoStatement => {
                increment_by_one(stats);
            }
            // A `binary_expression` the `ShortCircuit` row owns (#1695).
            _ if short_circuit::owns_kind(kind, node, code, ancestors) => {
                compute_booleans_from(node, stats, short_circuit::PAYERS);
            }
            // A named function — the `FuncOpener` row's callable owners —
            // resets structural nesting and bumps the function-depth
            // surcharge when nested in another (#696); the stop list is
            // the row's callable kinds.
            _ if func_opener::is_callable_kind(kind, node, code, ancestors) => {
                enter_function_boundary(&mut nesting, node, ancestors, func_opener::CALLABLE);
            }
            // A closure — the `Closure` row — charges the lambda channel
            // and does not reset nesting.
            _ if closure::owns_kind(kind, node, code, ancestors) => {
                nesting.lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
