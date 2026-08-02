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

use super::{Abc, Stats};
use crate::macros::python_bool_terminal_kinds;
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
//   `CaseClause`. We do not separately count the `if` / `while`
//   keyword: the condition expression itself is already covered by
//   `ComparisonOperator`. This matches the token-level approach
//   used for PHP / Bash.
//
//   `BooleanOperator` (Python's `and` / `or` wrapper) is
//   deliberately NOT counted, and `NotOperator` is kept as the
//   paper's "unary conditional expression". See the module-level
//   `Stats` doc-comment for the cross-language `&&` / `||` policy
//   (issue #395, walker tracked in #403).
// Python ABC unary-conditional walker (Fitzpatrick Rule 9 / Figure 4;
// issue #403). Python's `a and b` parses as `boolean_operator` (NOT
// `binary_operator`), so the walker triggers on the `And` / `Or`
// keyword tokens and iterates the immediate children of the parent
// `boolean_operator`. Terminal-bool kinds: `Identifier`, `True`,
// `False`, `Call`, `Attribute` (`obj.attr`), and `Subscript` (`xs[i]`).
//
// Unlike the C-family walkers, this one deliberately does NOT recurse
// into `NotOperator`, `ComparisonOperator`, or nested `BooleanOperator`
// children — each of those is counted by its own top-level dispatcher
// arm. Re-counting them inside the walker would inflate the metric for
// any `not x and y` / `x == 0 and y` shape. `ParenthesizedExpression`
// is still unwrapped to catch bare-identifier operands like
// `if (a) and b:` that the dispatcher would otherwise miss.
fn python_inspect_container(container_node: &Node, parent: &Node, conditions: &mut f64) {
    use Python::*;

    let mut node = *container_node;
    let mut node_kind = node.kind_id().into();
    let has_boolean_content = matches!(
        parent.kind_id().into(),
        BooleanOperator | IfStatement | WhileStatement | ConditionalExpression
    );

    loop {
        if !matches!(node_kind, ParenthesizedExpression) {
            break;
        }

        let Some(child) = node.child(1) else { break };
        node = child;
        node_kind = node.kind_id().into();

        if matches!(node_kind, python_bool_terminal_kinds!()) {
            if has_boolean_content {
                *conditions += 1.;
            }
            break;
        }
    }
}

// Phase-2B (issue #403): Python `if` / `while` condition slot.
// Python has no paren wrap around if-conditions and no top-level
// terminal arm, so the condition has to be classified directly:
//   - Identifier / True / False / Call / Attribute / Subscript at
//     the top level counts once (Rule 6: bare-boolean condition).
//   - ParenthesizedExpression unwraps via `python_inspect_container`.
//   - NotOperator / ComparisonOperator / BooleanOperator are
//     skipped: each is counted by its own top-level dispatcher arm
//     (the `Or`/`And` keyword walker, the `NotOperator` arm, and
//     the `ComparisonOperator` arm at lines `~1334-1390`).
fn python_count_condition(condition: &Node, parent: &Node, conditions: &mut f64) {
    use Python::*;
    let kind = condition.kind_id().into();
    if matches!(kind, python_bool_terminal_kinds!()) {
        *conditions += 1.;
    } else if matches!(kind, ParenthesizedExpression) {
        python_inspect_container(condition, parent, conditions);
    }
}

// Phase-2B (issue #1161): the condition slot of a Python conditional
// expression, `<consequence> if <condition> else <alternative>`. Before
// this, `a if c() else b` reported 1 — the `ConditionalExpression` node
// alone — where every C-family language reports 2 for the equivalent
// `c() ? a : b`, and `python_inspect_container`'s `ConditionalExpression`
// boolean-context seed was unreachable because no call site ever passed
// that parent.
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
        python_count_condition(&condition, node, conditions);
    }
}

fn python_inspect_child(node: &Node, idx: usize, conditions: &mut f64) {
    if let Some(child) = node.child(idx) {
        python_count_condition(&child, node, conditions);
    }
}

fn python_count_unary_conditions(list_node: &Node, conditions: &mut f64) {
    use Python::*;

    let list_kind = list_node.kind_id().into();
    let mut cursor = list_node.cursor();

    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();
            let node_kind = node.kind_id().into();

            if matches!(node_kind, python_bool_terminal_kinds!())
                && matches!(list_kind, BooleanOperator)
            {
                *conditions += 1.;
            } else if matches!(node_kind, ParenthesizedExpression) {
                python_inspect_container(&node, list_node, conditions);
            }
            // NotOperator / ComparisonOperator / nested BooleanOperator
            // children are intentionally not walked here — each has
            // its own top-level dispatcher arm and counting them
            // again would double-count.

            if !cursor.goto_next_sibling() {
                break;
            }
        }
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
            ComparisonOperator | ElifClause | ElseClause | ExceptClause | FinallyClause
            | NotOperator => {
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
            CaseClause if super::npa::python_case_clause_counts(node, UNDERSCORE as u16) => {
                stats.conditions += 1.;
            }
            // Fitzpatrick Rule 9 walker: each operand of an `and` /
            // `or` chain is one condition (issue #403). The `And` /
            // `Or` keyword tokens live inside a `boolean_operator`
            // wrapper which the walker iterates as the parent list.
            And | Or => {
                if let Some(parent) = ancestors.parent(node) {
                    python_count_unary_conditions(&parent, &mut stats.conditions);
                }
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
            IfStatement | WhileStatement => {
                python_inspect_child(node, 1, &mut stats.conditions);
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
