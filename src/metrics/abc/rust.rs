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
    count_negated_operand, last_operand, rows_own, wrapped_operand,
};
use crate::roles::rust::{case_arm, comparison, guard, short_circuit};
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
        AwaitExpression => {
            // Only an awaited async block yields its body; a bare one is a future.
            let mut operand = wrapped_operand(node)?;
            while let Some(inner) = rust_transparent_operand(&operand) {
                operand = inner;
            }
            if operand.kind_id() == AsyncBlock as u16 {
                operand = wrapped_operand(&operand)?;
            }
            Some((operand, false))
        }
        UnaryExpression if node.child(0).is_some_and(|c| c.kind_id() == BANG as u16) => {
            wrapped_operand(node).map(|o| (o, true))
        }
        _ => rust_transparent_operand(node).map(|o| (o, false)),
    }
}

fn rust_transparent_operand<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    use Rust::*;

    match node.kind_id().into() {
        ParenthesizedExpression | UnsafeBlock => wrapped_operand(node),
        ConstBlock => node.child_by_field_name("body"),
        TypeCastExpression => node.child_by_field_name("value"),
        Block | Block2 => last_operand(node),
        ExpressionStatement if !node.is_child(SEMI as u16) => wrapped_operand(node),
        _ => None,
    }
}

// Whether an arm of `compute` already charges `expr` (already peeled) as
// a condition: a comparison (the `Comparison` row's owner, paid by its
// token), a `let` condition (its own arm), or an `&&` / `||` chain —
// the `ShortCircuit` row's owner: a `binary_expression`, or the
// `let_chain` a Rust 2024 chain with a `let` in it becomes — whose
// operands each pay through `rust_count_condition`. Neither owner gate
// reads ancestors, so the peeled descendant is asked with none.
fn rust_condition_scores_itself(expr: &Node, code: &[u8]) -> bool {
    rows_own!(expr, code; comparison, short_circuit) || expr.kind_id() == Rust::LetCondition as u16
}

// Scores one boolean slot — an `if` / `while` condition, a match guard,
// an operand of an `&&` / `||` chain (see `count_boolean_slot`). A Rust
// slot only admits a `bool`, and the terminal-kind list it used to pay
// for missed valid ones: `if *flag`, `if a & b`, `if a ^ b`, `if result?`,
// `if match x { … }` and `if { a }` each scored 0 conditions against a
// cyclomatic decision of 1 (#1526). Asking whether the decision is
// already paid for covers them all. It also pays for an ill-typed
// `if -x`, which no valid program can tell apart (grammar-dispatch §6).
fn rust_count_condition(condition: &Node, code: &[u8], conditions: &mut f64) {
    count_boolean_slot(
        condition,
        rust_wrapper_operand,
        |expr| rust_condition_scores_itself(expr, code),
        conditions,
    );
}

// An operand outside a boolean slot — a `return` value, a call argument
// — scores only when a `!` proves it boolean (see
// `count_negated_operand`): `return !x` scores one, `return x` none,
// matching Java's policy (`java_return_without_conditions`).
fn rust_count_negated(operand: &Node, code: &[u8], conditions: &mut f64) {
    count_negated_operand(
        operand,
        rust_wrapper_operand,
        |expr| rust_condition_scores_itself(expr, code),
        conditions,
    );
}

fn rust_count_slot(slot: Option<Node>, code: &[u8], conditions: &mut f64) {
    if let Some(condition) = slot {
        rust_count_condition(&condition, code, conditions);
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
fn rust_count_match_arm(node: &Node, code: &[u8], conditions: &mut f64) {
    if case_arm::owns(node, code, Ancestors::unknown()) {
        *conditions += 1.;
    }
    // The guard is the `Guard` row's owner, the pattern's `condition`
    // field; the pattern is required at the pinned grammar.
    if let Some(pattern) = node
        .child_by_field_name("pattern")
        .filter(|pattern| guard::owns(pattern, code, Ancestors::unknown()))
    {
        rust_count_slot(pattern.child_by_field_name("condition"), code, conditions);
    }
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
//
// The chain is the token's parent, and only one the `ShortCircuit` row
// owns walks: the row pays the token ungated, so a `&&` inside a macro's
// `token_tree` (`m!(a && b)`) reaches here with a parent that owns no
// chain and scores nothing, as it always did. Which owner shape it is
// decides how its operands are read, and the let-chain's first `&&` is
// the one the row's projection lands on.
fn rust_count_chain_operands(token: &Node, chain: &Node, code: &[u8], conditions: &mut f64) {
    if !short_circuit::owns(chain, code, Ancestors::unknown()) {
        return;
    }
    let count = |operand: &Node, conds: &mut f64| rust_count_condition(operand, code, conds);
    // role-bypass: allow(Comparison, ShortCircuit) — traversal: which shape the owning chain
    // has, reached from the `ShortCircuit` payer's parent
    if chain.kind_id() == Rust::BinaryExpression {
        count_field_operands(chain, count, conditions);
    } else if short_circuit::project(chain).is_some_and(|first| first.id() == token.id()) {
        count_each_operand(chain, count, conditions);
    }
}

impl Abc for RustCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Rust::*;

        let kind = node.kind_id();
        match kind.into() {
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
            LetCondition | Else => {
                stats.conditions += 1.;
            }
            // The six comparison tokens, the `Comparison` row's payers
            // (`roles/rust.rs`): `<` / `>` double as type-argument
            // delimiters, and the row gates them to a `binary_expression`
            // parent.
            _ if comparison::pays_at_leaf_kind(kind, node, code, ancestors) => {
                stats.conditions += 1.;
            }
            // Every `match_arm` the `CaseArm` row owns is one condition;
            // a bare `_ => ...` arm is the C / Java `default:` equivalent
            // and the row leaves it out — mirrors the cyclomatic
            // treatment, which reads the same row, and Kotlin's `when` /
            // Java's `case` rules. The arm's guard is a further condition
            // slot — see `rust_count_match_arm`, which asks the row and
            // the `Guard` row of the arm itself, so a bare `_` arm with a
            // guard still reaches the slot.
            // role-bypass: allow(CaseArm) — the arm's two slots are read inside; the row is asked
            // there of the arm itself, bare or not
            MatchArm | MatchArm2 => rust_count_match_arm(node, code, &mut stats.conditions),
            // Fitzpatrick Rule 7: each operand of a `&&` / `||` chain
            // is one condition. The walker iterates immediate children
            // of the parent `binary_expression`; the per-`&&` / per-`||`
            // trigger keeps left-associative chains (`a && b && c`) at
            // O(operands) total work since the inner operator's pass
            // counts the inner pair and the outer operator's pass
            // counts only the new outer operand. See issue #403.
            _ if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors) => {
                if let Some(chain) = ancestors.parent(node) {
                    rust_count_chain_operands(node, &chain, code, &mut stats.conditions);
                }
            }
            // Phase-2B (issue #403): Fitzpatrick Rule 6 / 7 condition
            // slots. Read by grammar field: a fixed child(1) landed on a
            // comment (`if /*c*/ b`) and on the label of `'a: while b`
            // (#1455). Rust has no `do_statement`, no ternary, and no
            // for-condition slot.
            IfExpression | WhileExpression => {
                rust_count_slot(
                    node.child_by_field_name("condition"),
                    code,
                    &mut stats.conditions,
                );
            }
            // `return value;` — the value is the only operand, which
            // scores only behind a negation.
            ReturnExpression => {
                if let Some(value) = wrapped_operand(node) {
                    rust_count_negated(&value, code, &mut stats.conditions);
                }
            }
            // Method-argument walker: `m(!a, !b)` contributes one
            // condition per negated argument.
            // Each argument of a call is a negated operand (`m(!a, !b)`).
            Arguments => count_each_operand(
                node,
                |operand, conds| rust_count_negated(operand, code, conds),
                &mut stats.conditions,
            ),
            _ => {}
        }
    }
}
