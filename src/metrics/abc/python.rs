#![allow(
    clippy::enum_glob_use,
    clippy::too_many_lines,
    clippy::wildcard_imports
)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::{Abc, Stats, count_boolean_slot, wrapped_operand};
use crate::*;

// Fitzpatrick's ABC rules adapted for Python.
//
// - Assignments: every `Assignment` node that contains an explicit `=`
//   token (plain assignment, walrus `:=` lives in `NamedExpression`,
//   handled separately), plus every `AugmentedAssignment` (`+=`,
//   `-=`, …) and every `NamedExpression` (walrus). Bare type-only
//   annotations like `x: int` also parse as `Assignment` but have no
//   `=` child — these are excluded so a class-level type annotation
//   does not inflate the assignment count.
// - Branches: every `Call` node. Python's "object construction" is
//   syntactically a `Call` (`Foo()` parses as `call`), so the same
//   arm covers it without a separate `New`-style case.
// - Conditions: comparison operators (`ComparisonOperator` wraps
//   `<`, `>`, `==`, `!=`, `is`, `is not`, `in`, `not in`, etc. as a
//   single node), `ConditionalExpression` (ternary `a if c else b`),
//   the unary `NotOperator` (paper's "unary conditional expression",
//   Rule 7 / Figure 4), and the explicit arms of control flow:
//   `ElifClause`, `ElseClause`, `ExceptClause`, `FinallyClause`,
//   `CaseClause`. The predicate of an `if` / `elif` / `while`, a
//   ternary condition and a `case` guard are each a boolean slot that
//   pays one condition unless an arm inside it already did (see
//   `python_count_condition`).
//
//   `BooleanOperator` (Python's `and` / `or` wrapper) is
//   deliberately NOT counted, and `NotOperator` is kept as the
//   paper's "unary conditional expression". See the module-level
//   `Stats` doc-comment for the cross-language `&&` / `||` policy
//   (issue #395, walker tracked in #403).

// One step of the value peel: the expression a wrapper evaluates to, or
// `None` for anything that is not a wrapper. A parenthesis holds its
// only operand — read by role, not at child(1) after the `(` token,
// where a comment sits in `if (  # c` (#1455) — and a walrus
// `(n := g())` evaluates to its `value`, so `if (y := x > 1):` tests
// the comparison its own arm already counts. The walrus was a terminal
// kind until #1526, which charged that slot a second time.
//
// `not` is deliberately not peeled: the `NotOperator` arm counts it, so
// `python_condition_scores_itself` stops on it instead.
fn python_wrapper_operand<'a>(node: &Node<'a>) -> Option<(Node<'a>, bool)> {
    match node.kind_id().into() {
        Python::ParenthesizedExpression => wrapped_operand(node),
        Python::NamedExpression => node.child_by_field_name("value"),
        _ => None,
    }
    .map(|operand| (operand, false))
}

// Whether another arm of `compute` already charges `expr` (already
// peeled) as a condition: a comparison, a `not`, a ternary (each its own
// arm) or an `and` / `or` chain, whose walker scores each operand
// through `python_count_condition` and so always carries a condition of
// its own.
fn python_condition_scores_itself(expr: &Node) -> bool {
    use Python::*;

    matches!(
        expr.kind_id().into(),
        ComparisonOperator | NotOperator | BooleanOperator | ConditionalExpression
    )
}

// Scores one boolean slot: an `if` / `elif` / `while` predicate, a
// ternary condition, a `case` guard, or an operand of an `and` / `or`
// chain (see `count_boolean_slot`). Python is truthy-valued, so every
// expression the grammar can put there — `-x`, `x + 1`, a lambda — is a
// decision, and scored 0 while the slot paid only for a fixed list of
// terminal kinds (#1526).
fn python_count_condition(condition: &Node, conditions: &mut f64) {
    count_boolean_slot(
        condition,
        python_wrapper_operand,
        python_condition_scores_itself,
        conditions,
    );
}

// Each operand of an `and` / `or` chain is a boolean slot (Fitzpatrick
// Rule 9, #403). tree-sitter-python parses `a and b or c` as a
// left-nested chain of `boolean_operator`s, so an operand that is itself
// a chain is paid by its own operator's visit. Read by field: a comment
// beside the operator is a named child too, and must not pay.
fn python_count_chain_operands(chain: &Node, conditions: &mut f64) {
    for field in ["left", "right"] {
        if let Some(operand) = chain.child_by_field_name(field) {
            python_count_condition(&operand, conditions);
        }
    }
}

// Phase-2B (issue #1161): the condition slot of a Python conditional
// expression, `<consequence> if <condition> else <alternative>`. Before
// this, `a if c() else b` reported 1 — the `ConditionalExpression` node
// alone — where every C-family language reports 2 for the equivalent
// `c() ? a : b`.
//
// tree-sitter-python's `conditional_expression` is a bare
// `seq(expression, 'if', expression, 'else', expression)` carrying no
// grammar fields at all, so the slot cannot be named the way Ruby's and
// the C family's can. It is located by ROLE rather than by a bare index
// (`.claude/rules/grammar-dispatch.md` item 3): the first child after
// the `if` keyword that is not a comment.
//
// Both halves of that are load-bearing, and neither is theoretical.
// Comments are tree-sitter `extras`, so they land as direct children of
// `conditional_expression` and shift every index after them — `child(2)`
// reads the `if` token itself for `a\n# why\nif b else c`. They can also
// sit between the keyword and the condition, where they would be the
// anchor's next child, which is why the scan skips them rather than
// taking the first. The C family is immune to both only because it can
// ask for the slot by name.
//
// Only the condition is routed. Python's branch operands are already
// counted by the top-level `NotOperator` / `ComparisonOperator` arms — a
// different mechanism from the C-family walkers — so a wholesale
// `cpp_walk_ternary` copy would double-count: `(b) if a else (c)` would
// score its two parenthesised operands, which the identical
// unparenthesised `b if a else c` correctly scores at zero.
fn python_count_ternary_condition(node: &Node, conditions: &mut f64) {
    use Python::*;
    if let Some(condition) = node
        .children()
        .skip_while(|child| child.kind_id() != If as u16)
        .skip(1)
        .find(|child| child.kind_id() != Comment as u16)
    {
        python_count_condition(&condition, conditions);
    }
}

// The `case … if g:` guard of a `case_clause`, modelled as a condition
// slot exactly like the `if` / `while` slots (#1454, transferring
// #1422's C# rule). Before this, a guard scored whatever operator
// happened to sit inside it: `case n if n > 5:` counted one via the
// `comparison_operator` arm while `case n if is_even(n):` and
// `case _ if b:` counted zero, so three semantically identical guards
// produced two different numbers. As a slot every spelling contributes
// exactly one, and a compound guard (`case n if a > 1 and b:`) keeps its
// sub-structure rather than collapsing to one.
//
// By grammar FIELD, not index (`.claude/rules/grammar-dispatch.md` §3):
// `case_clause` names its guard `guard`, which is what keeps a
// comprehension's `if_clause` — the same kind, in a wholly different
// role — out of this slot. The clause itself carries no field for its
// expression (it is `seq('if', expression)`), so the operand is its
// first named child that is not an `extra`: a comment may precede it
// (`case n if  # why\n b:`), and would pay as a slot of its own.
//
// No double count (§5): cyclomatic reaches this guard through the `If`
// *keyword token* inside the `if_clause`, which no ABC arm matches.
fn python_count_case_guard(case_clause: &Node, conditions: &mut f64) {
    if let Some(operand) = case_clause
        .child_by_field_name("guard")
        .and_then(|guard| wrapped_operand(&guard))
    {
        python_count_condition(&operand, conditions);
    }
}

fn python_count_slot(slot: Option<Node>, conditions: &mut f64) {
    if let Some(condition) = slot {
        python_count_condition(&condition, conditions);
    }
}

impl Abc for PythonCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Python::*;

        match node.kind_id().into() {
            // Plain `=` assignment. tree-sitter-python emits an
            // `Assignment` node for both `x = 1` (LHS, `=`, RHS) and
            // bare annotations `x: int` (LHS, `:`, type, *no* `=`).
            // Filtering on the presence of an `EQ` child keeps the
            // annotation-only case out of the count.
            Assignment if node.first_child(|id| id == EQ).is_some() => {
                stats.assignments += 1.;
            }
            // Augmented assignment (`+=`, `-=`, `*=`, …) always counts;
            // walrus `name := expr` is a PEP-572 `NamedExpression` and
            // also binds a value, so it counts as one assignment under
            // Fitzpatrick's rule.
            AugmentedAssignment | NamedExpression => {
                stats.assignments += 1.;
            }
            // Every call — function call, method call, type
            // construction — is one branch. Python parses `Foo()` as
            // `Call`, so object construction folds into this arm.
            Call => {
                stats.branches += 1.;
            }
            // `x < y`, `a == b`, `c is None`, `n in xs`, `m not in xs`
            // all parse as a single `ComparisonOperator` node — one
            // node, one condition, regardless of how many comparison
            // operators are chained.
            ComparisonOperator | ElseClause | ExceptClause | FinallyClause | NotOperator => {
                // `NotOperator` is Python's unary `not`. Counting it
                // mirrors Java's `!x` / C#'s `!x` Abc condition rule
                // and closes the parity gap noted in #214 — without
                // it, `if not flag:` reports 0 conditions while
                // `if !flag` in Java reports 1. Nested combos like
                // `not (x > 0)` count both the unary and the
                // comparison once each (one logical "is-negation",
                // one logical "comparison"), matching Java's
                // `!(x > 0)`.
                stats.conditions += 1.;
            }
            // A non-wildcard `case` arm contributes one condition,
            // matching Rust's bare-`_` MatchArm filter and Java/C#'s
            // `default:` rule. The bare wildcard is detected by: (a)
            // `case_pattern` is `_`, AND (b) no `if_clause` sibling
            // on the `case_clause` — `case _ if g:` carries a guard
            // and still counts. The shared classifier lives in
            // `super::npa` next to `pattern_is_bare_underscore`.
            // The guard is a further condition slot — see
            // `python_count_case_guard`. It is counted inside this arm
            // rather than from an `IfClause` arm of its own because a
            // comprehension filter (`[x for x in xs if g]`) is the same
            // `if_clause` kind, and the `guard` field reaches only the
            // `case` one. A guarded clause always satisfies the gate
            // (`python_case_clause_counts` returns `true` on sight of
            // an `if_clause`), so no guard is lost to it.
            CaseClause if super::npa::python_case_clause_counts(node, UNDERSCORE as u16) => {
                stats.conditions += 1.;
                python_count_case_guard(node, &mut stats.conditions);
            }
            // Fitzpatrick Rule 9 walker: each operand of an `and` /
            // `or` chain is one condition (issue #403). The `And` /
            // `Or` keyword tokens live inside a `boolean_operator`
            // wrapper which the walker iterates as the parent list.
            And | Or => {
                if let Some(chain) = ancestors
                    .parent(node)
                    .filter(|p| p.kind_id() == BooleanOperator)
                {
                    python_count_chain_operands(&chain, &mut stats.conditions);
                }
            }
            // An `elif` is Java's `else if`: the `else` (+1, Rule 5) and
            // an `if` predicate slot, as Ruby's `elsif` is. It paid only
            // the first, so `elif b` scored one below `else if (b)` while
            // `elif x > 0` matched it through the comparison arm (#1526).
            ElifClause => {
                stats.conditions += 1.;
                python_count_slot(node.child_by_field_name("condition"), &mut stats.conditions);
            }
            // Phase-2B (issue #403): `if` / `while` condition slot.
            // Python has no paren wrap around if-conditions, so the
            // condition has to be classified directly. NotOperator,
            // ComparisonOperator, BooleanOperator children each have
            // their own dispatcher arms and are not re-counted here.
            // `for` has no condition slot; ReturnStatement /
            // ArgumentList do not need walker arms because every
            // unary-conditional content node (NotOperator,
            // ComparisonOperator) already has its own top-level arm.
            //
            // Read by grammar field (grammar-dispatch §3). Unlike the
            // other languages' slots (#1455) a fixed child(1) gave the
            // same answer: no valid Python puts a comment between `if`
            // and its condition, so no test can tell the two apart.
            IfStatement | WhileStatement => {
                python_count_slot(node.child_by_field_name("condition"), &mut stats.conditions);
            }
            // `a if c() else b` — the conditional expression node itself
            // is one condition, as the `?` token is in every C-family
            // grammar; its condition slot is a further Fitzpatrick Rule 9
            // unary condition (issue #1161). The branch operands are
            // deliberately not walked — see
            // `python_count_ternary_condition`.
            ConditionalExpression => {
                stats.conditions += 1.;
                python_count_ternary_condition(node, &mut stats.conditions);
            }
            _ => {}
        }
    }
}
