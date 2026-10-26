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
        use crate::roles::lua::{closure, func_opener, short_circuit};
        use Lua::*;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        let kind = node.kind_id();

        match kind.into() {
            // `is_else_if` returns true for `ElseifStatement`, but Lua's
            // grammar makes that node a child field of `IfStatement` rather
            // than a nested `if_statement`, so the guard is defensive only.
            IfStatement if !Self::is_else_if(node, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // `elseif` adds +1 at the same nesting level as the parent `if`,
            // matching how Tcl/Bash handle their dedicated elseif/elif nodes.
            ElseifStatement => {
                increment_branch_extension(stats);
            }
            ForStatement | WhileStatement | RepeatStatement => {
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
