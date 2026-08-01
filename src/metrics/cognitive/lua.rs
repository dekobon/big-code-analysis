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
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use Lua::*;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
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
            BinaryExpression => {
                compute_booleans(node, stats, And, Or);
            }
            FunctionDeclaration | FunctionDeclaration2 | FunctionDeclaration3 => {
                enter_function_boundary(
                    &mut nesting,
                    node,
                    ancestors,
                    &[
                        FunctionDeclaration,
                        FunctionDeclaration2,
                        FunctionDeclaration3,
                    ],
                );
            }
            FunctionDefinition => {
                nesting.lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
