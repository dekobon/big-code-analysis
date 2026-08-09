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
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use Bash::*;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            // `WhileStatement` covers both `while` and `until`; `ForStatement`
            // covers both `for` and `select`. `CStyleForStatement` is the
            // `for ((…))` arithmetic form. `ElifClause` is a dedicated node,
            // not a nested `if`, so no `is_else_if` check is needed.
            // `TernaryExpression` is the arithmetic ternary inside
            // `(( … ))` / `$(( … ))`, Bash's only ternary form. It nests
            // like the C-family `ConditionalExpression`, so a ternary
            // inside a ternary charges the inner one at +2 (#1268). The
            // pinned grammar emits only kind 223; the
            // `TernaryExpression2` alias is listed defensively per
            // grammar-dispatch §1 and pinned unreachable by
            // `bash_ternary_expression_alias_is_unreachable` in the
            // cyclomatic test module.
            IfStatement | WhileStatement | ForStatement | CStyleForStatement | CaseStatement
            | TernaryExpression | TernaryExpression2 => {
                increase_nesting(stats, &mut nesting);
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
                enter_function_boundary(&mut nesting, node, ancestors, &[FunctionDefinition]);
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
