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
use crate::lang_helpers::bash::bash_test_connective;

// FIXME(#1536 upstream): tree-sitter-bash 0.25.1 lets a test's
// comparison swallow the connectives around it, so
// `[ "$a" = 1 -o "$b" = 2 -o "$c" = 3 ]` parses as
// `"$a" = (1 -o "$b") = (2 -o "$c") = 3` (and `[[ … = … || … ]]` the
// same). The two `-o` then sit in sibling subtrees, neither inside the
// other's span, and the run of one operation scored two sequences. A
// connective's expression is an operand of a comparison only in that
// mis-nested tree, so the sequence it belongs to ends where the
// outermost such comparison does. A correct tree (`==` inside `[[ … ]]`,
// `-eq`) never puts a connective under a comparison, so this returns
// `node`'s own end there. The mis-nested tree has lost precedence, so a
// mixed run is scored in textual order: `[[ "$a" = 1 || "$b" = 2 &&
// "$c" = 3 || "$d" = 4 ]]` scores three sequences where the `==` twin,
// nested `(a || (b && c)) || d`, scores two.
fn bash_sequence_end(node: &Node, ancestors: Ancestors) -> usize {
    ancestors
        .iter(node)
        .map(|(ancestor, _)| ancestor)
        .take_while(|ancestor| {
            ancestor.kind_id() == Bash::BinaryExpression3 as u16
                && ancestor.wraps_any(&[Bash::EQ as u16, Bash::EQEQ as u16, Bash::BANGEQ as u16])
        })
        .last()
        .map_or(node.end_byte(), |outermost| outermost.end_byte())
}

impl Cognitive for BashCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
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
            // Inside `[ … ]` the same expression joins two tests with
            // `-a` / `-o`, keyed to the symbol it stands for so the
            // three spellings score alike (#1536).
            List => {
                compute_booleans(node, stats, AMPAMP, PIPEPIPE);
            }
            BinaryExpression3 => {
                let end = bash_sequence_end(node, ancestors);
                compute_booleans_by_node(node, end, stats, |child| match child.kind_id().into() {
                    AMPAMP | PIPEPIPE => Some(child.kind_id()),
                    _ => bash_test_connective(child, node, code).map(|op| op as u16),
                });
            }
            FunctionDefinition => {
                enter_function_boundary(&mut nesting, node, ancestors, &[FunctionDefinition]);
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
