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
// same, as `=~`, which shares `=`'s precedence). The two `-o` then sit
// in sibling subtrees, neither inside the other's span, and the run of
// one operation scored two sequences. A connective's expression is an
// operand of such a comparison only in that mis-nested tree, so the
// sequence it belongs to ends where the outermost comparison of the run
// does; where no comparison holds the connective, its own node's end.
// The mis-nested tree has lost precedence, so a mixed run is scored in
// textual order: `[[ "$a" = 1 || "$b" = 2 && "$c" = 3 || "$d" = 4 ]]`
// scores three sequences where the `==` twin, nested
// `(a || (b && c)) || d`, scores two. `[ … ]` gives every `-x` test one
// precedence, so a mixed `-a` / `-o` run over `-eq` tests is scored in
// textual order too, though a run of one connective there nests down a
// single spine and needs nothing from this.
const RUN_COMPARISONS: [u16; 4] = [
    Bash::EQ as u16,
    Bash::EQEQ as u16,
    Bash::BANGEQ as u16,
    Bash::EQTILDE as u16,
];

/// The comparison runs the walk is inside, innermost last. A run member
/// is a `BinaryExpression3` whose operator is one of
/// [`RUN_COMPARISONS`]; each records its span and the end of the
/// outermost member of its run, inherited when its parent is a member
/// too.
///
/// The walk is pre-order, so a member is visited before everything under
/// it, and one whose span does not hold the node being visited is
/// finished. Climbing from every `BinaryExpression3` to the top of its run
/// instead made a long `[ … -o … ]` or `[[ … = … && … ]]` test quadratic.
/// A stack rather than one slot, because a run can sit in a parenthesised
/// operand of another and the outer run resumes after it:
/// `[[ $a = 1 || ( $x = 5 && $y = 6 ) || $b = 2 || $c = 3 ]]` keeps
/// `$c = 3` in the `||` sequence `$b = 2` opened. Walk state: never
/// serialized or merged.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ComparisonRuns(Vec<RunMember>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RunMember {
    id: usize,
    start: usize,
    end: usize,
    run_end: usize,
}

impl ComparisonRuns {
    /// The byte a boolean sequence under `node`, a `BinaryExpression3`
    /// whose parent is `parent`, extends to: the end of the run `parent`
    /// is a member of, else `node`'s own end. Records `node` when it is a
    /// member itself.
    fn sequence_end(&mut self, node: &Node, parent: Option<Node>) -> usize {
        let (start, end) = (node.start_byte(), node.end_byte());
        while self
            .0
            .last()
            .is_some_and(|member| start < member.start || member.end < end)
        {
            self.0.pop();
        }
        let run_end = match (parent, self.0.last()) {
            (Some(parent), Some(member)) if member.id == parent.id() => member.run_end,
            _ => end,
        };
        if node.wraps_any(&RUN_COMPARISONS) {
            self.0.push(RunMember {
                id: node.id(),
                start,
                end,
                run_end,
            });
        }
        run_end
    }
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
                let end = stats.bash_runs.sequence_end(node, ancestors.parent(node));
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
