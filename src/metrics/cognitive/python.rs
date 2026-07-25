//! `Cognitive` implementation for Python.
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

/// Precompute the nesting of each clause in a Python comprehension or
/// generator expression, writing each clause's nesting into its own slot
/// in `nesting_map`, and return the extra nesting the comprehension's
/// *element* position sits at.
///
/// `for_in_clause` and `if_clause` are SIBLINGS under the comprehension
/// node, not parent/child, and each clause's nesting depends on how many
/// `for` clauses precede it. The comprehension node — visited before any
/// of its clauses in pre-order — runs this single pass so the result is
/// independent of sibling traversal order; that is the #421 fix (the
/// original #417 sibling write-back was never seen by a comprehension
/// sitting in the *element* position, which pre-order visits before the
/// outer clauses run, so it under-counted).
///
/// Each clause sits at the comprehension's inherited `nesting` plus the
/// number of `for` clauses strictly before it. The element executes
/// inside the body opened by the *last* clause, so it sits `for_count`
/// levels deep (a trailing `for` has already advanced the count) plus one
/// more when the last clause is an `if`.
fn python_comprehension_clause_nesting(
    node: &Node,
    nesting: usize,
    depth: usize,
    lambda: usize,
    nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
) -> usize {
    use Python::*;
    let mut for_count = 0;
    let mut last_clause_is_if = false;
    for child in node.children() {
        let kind = child.kind_id();
        if kind == ForInClause as u16 {
            nesting_map.insert(child.id(), (nesting + for_count, depth, lambda));
            for_count += 1;
            last_clause_is_if = false;
        } else if kind == IfClause as u16 {
            nesting_map.insert(child.id(), (nesting + for_count, depth, lambda));
            last_clause_is_if = true;
        }
    }
    for_count + usize::from(last_clause_is_if)
}

/// Apply the structural increment and boolean-sequence accounting a
/// Python `boolean_operator` node contributes.
///
/// Only the *outermost* boolean operator in a chain pays the structural
/// cost: if walking ancestors (stopping at a `lambda` boundary) finds
/// another `boolean_operator` first, this node is nested inside one
/// already counted, so the `== 0` guard skips it. The outermost operator
/// then adds one structural unit per enclosing control construct
/// (`expression_list`, `if`/`for`/`while`) up to the nearest lambda.
fn python_apply_boolean_operator(node: &Node, stats: &mut Stats) {
    use Python::*;
    if node.count_specific_ancestors::<PythonCode>(
        |node| node.kind_id() == BooleanOperator,
        python_is_lambda,
    ) == 0
    {
        stats.structural += node.count_specific_ancestors::<PythonCode>(python_is_lambda, |node| {
            matches!(
                node.kind_id().into(),
                ExpressionList | IfStatement | ForStatement | WhileStatement
            )
        });
    }
    compute_booleans(node, stats, And, Or);
}

impl Cognitive for PythonCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Python::*;

        // Get nesting of the parent
        let (mut nesting, mut depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            // `else: if x:` chains surface as an `if_statement` wrapped
            // in an `else_clause`; `Self::is_else_if` flags that shape
            // so the nesting increment lands only on the outer chain
            // (matching the `elif_clause` accounting one arm below).
            IfStatement if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            ForStatement | WhileStatement | ConditionalExpression | MatchStatement => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // A comprehension / generator expression is a loop with an
            // optional filter, so it carries cognitive load just like the
            // explicit `for`/`if` form it desugars to (#417). Cyclomatic
            // already counts the `for`/`if` keyword tokens inside these
            // clauses; without these arms `[x for x in xs if x > 0]` scored
            // cognitive 0 while the equivalent explicit loop scored 3.
            //
            // `for_in_clause` and `if_clause` are SIBLINGS under the
            // comprehension node, not parent/child, and each clause's nesting
            // depends on how many `for` clauses precede it. Rather than have
            // each clause re-scan its siblings (O(N^2)) or write its nesting
            // back onto the shared parent for later siblings to read, the
            // comprehension node — visited before any of its clauses in
            // pre-order — precomputes every clause's nesting in one pass and
            // stashes it in that clause's own map slot, which the
            // `ForInClause | IfClause` arm reads back. Computing it here, on
            // the ancestor pre-order reaches first, makes the result
            // independent of sibling traversal order; that is the #421 fix
            // (the original #417 sibling write-back was never seen by a
            // comprehension sitting in the *element* position, which pre-order
            // visits before the outer clauses run, so it under-counted).
            //
            // The same pass accumulates the element's own nesting onto the
            // comprehension node's slot, so a nested comprehension in element
            // position inherits the full outer loop+filter depth.
            ListComprehension
            | DictionaryComprehension
            | SetComprehension
            | GeneratorExpression => {
                nesting +=
                    python_comprehension_clause_nesting(node, nesting, depth, lambda, nesting_map);
            }
            ForInClause | IfClause => {
                // `nesting` already holds this clause's own value: the
                // comprehension (visited first in pre-order) precomputed it
                // into this clause's map slot, and since #1062 a node reads
                // its own slot, so the read at the top of `compute` picks it
                // up. Before #1062 a node read its *parent's* slot, so this
                // arm had to re-read the slot explicitly; that override is
                // now a no-op and has been removed.
                stats.nesting = nesting + depth + lambda;
                increment(stats);
                stats.boolean_seq.reset();
            }
            ElifClause => {
                // No nesting increment for them because their cost has already
                // been paid by the if construct
                increment_branch_extension(stats);
            }
            ElseClause => {
                // No nesting increment for it because its cost has already
                // been paid by the if construct. A `finally` clause, by
                // contrast, is structured cleanup that always runs and adds
                // 0 per the SonarSource Cognitive Complexity spec (#416) —
                // so `FinallyClause` deliberately falls through to `_ => {}`,
                // matching the Java sibling which has no finally arm.
                increment_by_one(stats);
            }
            ExceptClause => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            ExpressionList | ExpressionStatement | Tuple => {
                stats.boolean_seq.reset();
            }
            BooleanOperator => python_apply_boolean_operator(node, stats),
            // `Lambda` (196) is the emitted lambda; `Lambda2` (197) is the
            // hidden alias `python_is_lambda` also accepts. A match arm
            // cannot route through the predicate, so the alias set is
            // spelled out here and kept in sync with it (#422; the
            // drift guard in checker.rs flags a bump that emits Lambda2).
            Lambda | Lambda2 => {
                // Increase lambda nesting
                lambda += 1;
            }
            FunctionDefinition => {
                // Increase depth function nesting if needed
                increment_function_depth(&mut depth, node, &[FunctionDefinition]);
            }
            _ => {}
        }
        // Add node to nesting map
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}
