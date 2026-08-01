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
/// Each clause sits at the comprehension's `inherited.conditional` plus
/// the number of `for` clauses strictly before it, and carries the
/// comprehension's `function_depth` / `lambda` through unchanged. The
/// element executes inside the body opened by the *last* clause, so it
/// sits `for_count` levels deep (a trailing `for` has already advanced
/// the count) plus one more when the last clause is an `if`.
fn python_comprehension_clause_nesting(
    node: &Node,
    inherited: Nesting,
    nesting_map: &mut NestingMap,
) -> usize {
    use Python::*;
    let mut for_count = 0;
    let mut last_clause_is_if = false;
    for child in node.children() {
        let kind = child.kind_id();
        let is_for = kind == ForInClause as u16;
        if !is_for && kind != IfClause as u16 {
            continue;
        }
        nesting_map.insert(
            child.id(),
            Nesting {
                conditional: inherited.conditional + for_count,
                ..inherited
            },
        );
        for_count += usize::from(is_for);
        last_clause_is_if = !is_for;
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
/// then adds one structural unit per enclosing `lambda`, walking upward
/// only as far as the nearest `expression_list` / `if` / `for` / `while`.
/// `count_specific_ancestors` takes `(ancestors, check, stop)` in that
/// order, so it is the lambdas that get counted and the control
/// constructs that end the walk — the reverse of what this comment
/// claimed before #1090.
///
/// The per-lambda surcharge is a deliberate Python-only deviation from
/// Campbell, reviewed and kept in #1150; the user-facing statement of
/// it, with the measured ladder, is the Python bullet under *Cognitive
/// Complexity → Per-language deviations* in
/// `big-code-analysis-book/src/metrics.md`.
fn python_apply_boolean_operator<'a>(
    node: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) {
    use Python::*;
    if node.count_specific_ancestors::<PythonCode>(
        ancestors,
        |node| node.kind_id() == BooleanOperator,
        python_is_lambda,
    ) == 0
    {
        stats.structural +=
            node.count_specific_ancestors::<PythonCode>(ancestors, python_is_lambda, |node| {
                // Only `ExpressionList` can change the count: a lambda
                // body is a single expression, so no lambda ever sits
                // *above* an `if`/`for`/`while` statement, and stopping
                // at one is indistinguishable from running to the module
                // root. The three statement kinds are kept as an explicit
                // statement-boundary set rather than a claim about
                // coverage. `ExpressionList` is the observable arm,
                // reached via a parenthesised `yield` or an f-string
                // interpolation — see
                // `python_boolean_in_expression_list_under_lambda`
                // (#1090).
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
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use Python::*;

        // Get nesting of the parent
        let mut nesting = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            // `else: if x:` chains surface as an `if_statement` wrapped
            // in an `else_clause`; `Self::is_else_if` flags that shape
            // so the nesting increment lands only on the outer chain
            // (matching the `elif_clause` accounting one arm below).
            IfStatement if !Self::is_else_if(node, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            ForStatement | WhileStatement | ConditionalExpression | MatchStatement => {
                increase_nesting(stats, &mut nesting);
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
                nesting.conditional +=
                    python_comprehension_clause_nesting(node, nesting, nesting_map);
            }
            ForInClause | IfClause => {
                // `nesting` already holds this clause's own value: the
                // comprehension (visited first in pre-order) precomputed it
                // into this clause's map slot, and since #1062 a node reads
                // its own slot, so the read at the top of `compute` picks it
                // up. Before #1062 a node read its *parent's* slot, so this
                // arm had to re-read the slot explicitly; that override is
                // now a no-op and has been removed.
                stats.nesting = nesting.total();
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
                increase_nesting(stats, &mut nesting);
            }
            ExpressionList | ExpressionStatement | Tuple => {
                stats.boolean_seq.reset();
            }
            BooleanOperator => python_apply_boolean_operator(node, ancestors, stats),
            // `Lambda` (196) is the emitted lambda; `Lambda2` (197) is the
            // hidden alias `python_is_lambda` also accepts. A match arm
            // cannot route through the predicate, so the alias set is
            // spelled out here and kept in sync with it (#422; the
            // drift guard in checker.rs flags a bump that emits Lambda2).
            Lambda | Lambda2 => {
                // A lambda amplifies the enclosing nesting rather than
                // replacing it, so it deliberately does NOT take the
                // function-boundary reset below: every sibling lambda arm
                // (Java `LambdaExpression`, JS `ArrowFunction`, Rust
                // `ClosureExpression`, …) leaves `conditional` alone, and
                // adding the reset here would break the cross-language
                // parity that reset exists to preserve (#1149).
                nesting.lambda += 1;
            }
            // At a (possibly nested) `def` boundary, reset structural
            // nesting to zero and bump the function-depth surcharge when
            // this definition is itself nested inside another, so a `def`
            // written inside an `if` is scored against its own depth rather
            // than the enclosing function's — matching Java, Rust, and
            // every other conforming family (#696, #1149). Python is the
            // one family that provably needs no `nesting.lambda = 0`
            // companion to go with it — a `def` is a statement and a
            // lambda body is a single expression, so no
            // `function_definition` can sit under a `lambda`. Elsewhere
            // that shape is legal (`let f = || { fn g() {} };`) and only
            // the JS macro currently carries the extra line.
            FunctionDefinition => {
                enter_function_boundary(&mut nesting, node, ancestors, &[FunctionDefinition]);
            }
            _ => {}
        }
        // Add node to nesting map
        nesting_map.insert(node.id(), nesting);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ParserTrait;

    // `Nesting`'s three fields are same-typed and every downstream
    // consumer folds them into one sum (`stats.nesting = conditional +
    // function_depth + lambda`), so transposing two of them inside
    // `python_comprehension_clause_nesting` leaves every cognitive score
    // in the suite unchanged. The only way to pin the assignment is to
    // drive the helper directly and assert each clause's map slot
    // field-by-field — which is what the `Nesting` parameter now makes
    // checkable at the call site too.
    #[test]
    fn python_comprehension_clauses_carry_inherited_depth_and_lambda() {
        // Both fixtures have three clauses and differ only in which kind
        // trails, which is what selects the `last_clause_is_if` term of
        // the returned element depth. A trailing `for` has already
        // advanced `for_count`, so the term contributes 0 and covering
        // only that shape leaves the term untested — the element depth
        // is the same with it deleted.
        let cases = [
            // `for a in xs` (0 preceding `for`s), `if a` (1),
            // `for b in ys` (1); trailing `for` → element at 2.
            ("[x for a in xs if a for b in ys]", [1, 2, 2], 2),
            // `for a in xs` (0), `for b in ys` (1), `if b` (2);
            // trailing `if` → one deeper than the 2 `for`s → 3.
            ("[x for a in xs for b in ys if b]", [1, 2, 3], 3),
        ];

        for (source, expected_conditionals, expected_element) in cases {
            let parser = PythonParser::new(
                source.as_bytes().to_vec(),
                std::path::Path::new("comprehension.py"),
                None,
            );
            let root = parser.root();
            let comprehension = *root
                .descendants_by_kind(&["list_comprehension"])
                .first()
                .expect("the fixture contains exactly one list comprehension");
            let clauses: Vec<_> = comprehension
                .children()
                .filter(|child| matches!(child.kind(), "for_in_clause" | "if_clause"))
                .collect();
            assert_eq!(clauses.len(), 3, "`{source}` must expose all three clauses");

            let inherited = Nesting {
                conditional: 1,
                function_depth: 2,
                lambda: 3,
            };
            let mut nesting_map = NestingMap::default();
            let element_nesting =
                python_comprehension_clause_nesting(&comprehension, inherited, &mut nesting_map);

            assert_eq!(
                element_nesting, expected_element,
                "wrong element depth for `{source}`"
            );
            for (clause, expected_conditional) in clauses.iter().zip(expected_conditionals) {
                assert_eq!(
                    nesting_map.get(&clause.id()),
                    Some(&Nesting {
                        conditional: expected_conditional,
                        function_depth: 2,
                        lambda: 3,
                    }),
                    "wrong nesting recorded for the `{}` clause of `{source}`",
                    clause.kind()
                );
            }
        }
    }
}
