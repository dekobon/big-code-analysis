//! `Cognitive` implementation for Bash.
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

impl Cognitive for BashCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use Bash::*;

        let (mut nesting, mut depth, lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            // `WhileStatement` covers both `while` and `until`; `ForStatement`
            // covers both `for` and `select`. `CStyleForStatement` is the
            // `for ((…))` arithmetic form. `ElifClause` is a dedicated node,
            // not a nested `if`, so no `is_else_if` check is needed.
            IfStatement | WhileStatement | ForStatement | CStyleForStatement | CaseStatement => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            ElifClause | ElseClause => {
                increment_branch_extension(stats);
            }
            // `&&` / `||` appear in two places: as direct children of
            // `Bash::List` (command level: `cmd && cmd`) and as direct
            // children of `Bash::BinaryExpression3` (inside `[[ … ]]`,
            // `(( … ))`, c-style `for ((…))` conditions, and
            // parenthesized sub-expressions). Verified empirically
            // against tree-sitter-bash 0.25.1 — the other four
            // `BinaryExpression*` enum variants never wrap `&&` / `||`.
            List | BinaryExpression3 => {
                compute_booleans(node, stats, AMPAMP, PIPEPIPE);
            }
            FunctionDefinition => {
                nesting = 0;
                increment_function_depth(&mut depth, node, &[FunctionDefinition]);
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}
