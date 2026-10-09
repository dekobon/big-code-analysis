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

use super::{
    Abc, Stats, count_boolean_slot, count_each_operand, count_field_operands,
    count_negated_operand, last_operand, wrapped_operand,
};
use crate::*;

// One step of the value peel (see `PeelStep`). A parenthesis holds one
// operand, read by role rather than at child(1) after the `(` or the
// operator token, where a comment may sit (`if ! /*c*/ b`, #1455) —
// neither wrapper names a field. A block evaluates to its last
// expression, so `if { x > 1 } {}` tests the comparison its own arm
// already counts. `!` is the one wrapper that proves its operand
// boolean; the other unary operators (`-x`, `*p`) yield a value and stop
// the peel.
fn rust_wrapper_operand<'a>(node: &Node<'a>) -> Option<(Node<'a>, bool)> {
    use Rust::*;

    match node.kind_id().into() {
        ParenthesizedExpression => wrapped_operand(node).map(|o| (o, false)),
        Block | Block2 => last_operand(node).map(|o| (o, false)),
        UnaryExpression if node.child(0).is_some_and(|c| c.kind_id() == BANG as u16) => {
            wrapped_operand(node).map(|o| (o, true))
        }
        _ => None,
    }
}

// Whether an arm of `compute` already charges `expr` (already peeled) as
// a condition: a comparison (the comparison-token arms), a `let`
// condition (its own arm), or an `&&` / `||` chain — a
// `binary_expression`, or the `let_chain` a Rust 2024 chain with a `let`
// in it becomes — whose operands each pay through `rust_count_condition`.
fn rust_condition_scores_itself(expr: &Node) -> bool {
    use Rust::*;

    match expr.kind_id().into() {
        LetCondition | LetChain | LetChain2 => true,
        BinaryExpression => expr.child_by_field_name("operator").is_some_and(|op| {
            matches!(
                op.kind_id().into(),
                EQEQ | BANGEQ | LT | GT | LTEQ | GTEQ | AMPAMP | PIPEPIPE
            )
        }),
        _ => false,
    }
}

// Scores one boolean slot — an `if` / `while` condition, a match guard,
// an operand of an `&&` / `||` chain (see `count_boolean_slot`). A Rust
// slot only admits a `bool`, and the terminal-kind list it used to pay
// for missed valid ones: `if *flag`, `if a & b`, `if a ^ b`, `if result?`,
// `if match x { … }` and `if { a }` each scored 0 conditions against a
// cyclomatic decision of 1 (#1526). Asking whether the decision is
// already paid for covers them all. It also pays for an ill-typed
// `if -x`, which no valid program can tell apart (grammar-dispatch §6).
fn rust_count_condition(condition: &Node, conditions: &mut f64) {
    count_boolean_slot(
        condition,
        rust_wrapper_operand,
        rust_condition_scores_itself,
        conditions,
    );
}

// An operand outside a boolean slot — a `return` value, a call argument
// — scores only when a `!` proves it boolean (see
// `count_negated_operand`): `return !x` scores one, `return x` none,
// matching Java's policy (`java_return_without_conditions`).
fn rust_count_negated(operand: &Node, conditions: &mut f64) {
    count_negated_operand(
        operand,
        rust_wrapper_operand,
        rust_condition_scores_itself,
        conditions,
    );
}

fn rust_count_slot(slot: Option<Node>, conditions: &mut f64) {
    if let Some(condition) = slot {
        rust_count_condition(&condition, conditions);
    }
}

// The conditions a `match_arm` contributes: the arm itself, plus its
// guard.
//
// The arm counts unless its pattern is a bare `_` — the C / Java
// `default:` equivalent, filtered here exactly as cyclomatic filters it.
//
// The guard is a condition slot, modelled like the `if` / `while` slots
// (#1454, transferring #1422's C# rule). Before this, a guard scored
// whatever operator happened to sit inside it: `n if n > 5` counted one
// via the comparison-token arm while `n if is_even(n)` and `_ if b`
// counted zero, so three semantically identical guards produced two
// different numbers. As a slot every spelling contributes exactly one,
// and a compound guard (`n if a > 1 && b`) keeps its sub-structure
// rather than collapsing to one.
//
// By grammar FIELD, not index (`.claude/rules/grammar-dispatch.md` §3):
// `match_pattern` names its guard `condition`, so a comment between the
// pattern and the `if` cannot shift the read the way it does for the
// positional sibling slots. The field is absent on an unguarded arm,
// which is what keeps the control at its old value.
//
// No double count (§5): cyclomatic reaches this guard through the `If`
// *keyword token* inside `match_pattern`, which no ABC arm matches, and
// the field's two non-expression types — `let_condition` (`n if let
// Some(v) = o`) and `let_chain` — are already owned by the
// `LetCondition` token arm and by the `&&` walker respectively, so
// `rust_condition_scores_itself` stops the slot paying for them again.
fn rust_count_match_arm(node: &Node, conditions: &mut f64) {
    let Some(pattern) = node.child_by_field_name("pattern") else {
        // `pattern` is a required field, so this is unreachable at the
        // pinned grammar; counting the arm keeps the pre-#1454
        // `is_some_and` polarity if error recovery ever produces one.
        *conditions += 1.;
        return;
    };
    if !super::npa::pattern_is_bare_underscore(&pattern, Rust::UNDERSCORE as u16) {
        *conditions += 1.;
    }
    rust_count_slot(pattern.child_by_field_name("condition"), conditions);
}

// Fitzpatrick Rule 7 (#403): each operand of an `&&` / `||` chain is a
// boolean slot. `a && b && c` is a left-nested chain, so an operand that
// is itself a chain is paid by its own operator's visit and the walk
// stays O(operands). A `binary_expression` names its operands; a Rust
// 2024 `let_chain` (`if a && let Some(x) = b`) names none and is flat,
// so its operands are every child that is neither an `&&` token nor an
// `extra`, and only its first `&&` walks them: each later one would pay
// every operand again (`if let Some(_) = o && b && c` scored 5 against
// 3). Its `let` operands score themselves.
fn rust_count_chain_operands(token: &Node, chain: &Node, conditions: &mut f64) {
    if chain.kind_id() == Rust::BinaryExpression {
        count_field_operands(chain, rust_count_condition, conditions);
    } else if matches!(chain.kind_id().into(), Rust::LetChain | Rust::LetChain2)
        && chain
            .children()
            .find(|child| child.kind_id() == Rust::AMPAMP)
            .is_some_and(|first| first.id() == token.id())
    {
        count_each_operand(chain, rust_count_condition, conditions);
    }
}

impl Abc for RustCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Rust::*;

        match node.kind_id().into() {
            // Plain `x = expr` (assignment_expression) and augmented
            // forms `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`,
            // `<<=`, `>>=` (compound_assignment_expr) both bind a
            // value; each counts as one assignment. Rust grammar
            // isolates both in distinct named nodes, so there is no
            // risk of double-counting the contained `EQ` token here.
            AssignmentExpression | CompoundAssignmentExpr => {
                stats.assignments += 1.;
            }
            // `let x = expr;` and `let mut x = expr;` both carry an
            // explicit `=` initializer — the `value` field is present
            // on the `let_declaration` only when the initializer
            // exists. Per Fitzpatrick (1997), every `=` operator
            // increments A; the JS impl already counts `let x = 5;`
            // (and excludes `const`). We follow the literal reading
            // for Rust too and count both `let x = ...;` and
            // `let mut x = ...;` — distinguishing the `mut` form
            // would diverge from the JS rule (which does not
            // distinguish `let` from `var`) and complicates the
            // implementation without changing the cross-language
            // story. Bare `let x;` (no initializer) leaves the
            // `value` field unset and correctly stays out.
            LetDeclaration if node.child_by_field_name("value").is_some() => {
                stats.assignments += 1.;
            }
            // Every call expression — including method calls
            // (`a.b.c()` parses as `call_expression` whose callee is a
            // `field_expression`) — plus every `try_expression` (the
            // `?` operator, a short-circuit return on Result / Option)
            // contributes one branch. Macro invocations parse as
            // `macro_invocation`, NOT `call_expression`, so they are
            // intentionally NOT counted as branches.
            CallExpression | TryExpression => {
                stats.branches += 1.;
            }
            // Comparison operators emitted as token children of a
            // `binary_expression`, `if let` / `while let` conditions,
            // and the `else` keyword each count as one condition.
            // `let_condition` covers both `if let` and `while let`
            // (Rust's grammar uses the same node for both); inside a
            // `let_chain` each `let_condition` counts separately.
            // Java counts the `Else` token directly; Rust's grammar
            // exposes the same token and we follow that lead.
            LTEQ | GTEQ | EQEQ | BANGEQ | LetCondition | Else => {
                stats.conditions += 1.;
            }
            // `<` / `>` doubles as type-argument delimiter; the
            // `BinaryExpression` parent check disambiguates without
            // needing to inspect siblings.
            LT | GT
                if ancestors
                    .parent(node)
                    .is_some_and(|p| matches!(p.kind_id().into(), BinaryExpression)) =>
            {
                stats.conditions += 1.;
            }
            // Every non-wildcard `match_arm` is one condition. A bare
            // `_ => ...` arm is the C / Java `default:` equivalent and
            // is excluded — mirrors the cyclomatic treatment and
            // Kotlin's `when` / Java's `case` rules. Patterns like
            // `Some(_)`, `(_, x)`, or `_ if guard` are not bare
            // wildcards and still count. The check scans only NAMED
            // children of `match_pattern` so anonymous tokens like a
            // leading `|` (allowed in or-patterns: `| _ => ...`) do
            // not throw off the detection. A guard (`_ if g`) adds a
            // second named child to `match_pattern` and so escapes
            // the bare-wildcard filter.
            //
            // The arm's guard is a further condition slot — see
            // `rust_count_match_arm`.
            MatchArm | MatchArm2 => rust_count_match_arm(node, &mut stats.conditions),
            // Fitzpatrick Rule 7: each operand of a `&&` / `||` chain
            // is one condition. The walker iterates immediate children
            // of the parent `binary_expression`; the per-`&&` / per-`||`
            // trigger keeps left-associative chains (`a && b && c`) at
            // O(operands) total work since the inner operator's pass
            // counts the inner pair and the outer operator's pass
            // counts only the new outer operand. See issue #403.
            AMPAMP | PIPEPIPE => {
                if let Some(chain) = ancestors.parent(node) {
                    rust_count_chain_operands(node, &chain, &mut stats.conditions);
                }
            }
            // Phase-2B (issue #403): Fitzpatrick Rule 6 / 7 condition
            // slots. Read by grammar field: a fixed child(1) landed on a
            // comment (`if /*c*/ b`) and on the label of `'a: while b`
            // (#1455). Rust has no `do_statement`, no ternary, and no
            // for-condition slot.
            IfExpression | WhileExpression => {
                rust_count_slot(node.child_by_field_name("condition"), &mut stats.conditions);
            }
            // `return value;` — the value is the only operand, which
            // scores only behind a negation.
            ReturnExpression => {
                if let Some(value) = wrapped_operand(node) {
                    rust_count_negated(&value, &mut stats.conditions);
                }
            }
            // Method-argument walker: `m(!a, !b)` contributes one
            // condition per negated argument.
            // Each argument of a call is a negated operand (`m(!a, !b)`).
            Arguments => count_each_operand(node, rust_count_negated, &mut stats.conditions),
            _ => {}
        }
    }
}
