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
use crate::macros::rust_bool_terminal_kinds;
use crate::*;

// Rust ABC unary-conditional walker (Fitzpatrick Rule 7 / Listing 2).
//
// On every `&&` / `||` token, we walk the parent `binary_expression`
// and count each non-comparison operand as one condition. Identifier,
// boolean literal, call, field-expression, and index-expression
// operands count directly. Operands wrapped in `(…)` or `!…` route
// through `rust_inspect_container`, which unwraps the wrapper chain
// until it lands on a terminal — then counts. Operands that are
// themselves nested `binary_expression`s (left-associative
// `(a && b) && c`) are not counted at the outer site; the inner
// `&&` token's own walker pass picks them up.
//
// The list-kind guard inside the count helper prevents an Identifier
// or BooleanLiteral that happens to be an immediate child of a non-
// binary parent from contributing — only direct operands of a
// `binary_expression` count as unary conditions per Rule 7. See issue
// #403.
fn rust_inspect_container(container_node: &Node, parent: &Node, conditions: &mut f64) {
    use Rust::*;

    let mut node = *container_node;
    let mut node_kind = node.kind_id().into();
    // `MatchPattern` joined this list with #1454: a match guard
    // (`n if g =>`) is a boolean slot exactly as an `if` condition is,
    // so a parenthesised guard operand (`n if (b)`) counts where the
    // bare `n if b` already did.
    let mut has_boolean_content = matches!(
        parent.kind_id().into(),
        BinaryExpression | IfExpression | WhileExpression | LetChain | LetChain2 | MatchPattern
    );

    loop {
        let is_parens = matches!(node_kind, ParenthesizedExpression);
        let is_not = matches!(node_kind, UnaryExpression)
            && node.child(0).is_some_and(|c| c.kind_id() == BANG as u16);

        if !is_parens && !is_not {
            break;
        }
        if !has_boolean_content && is_not {
            has_boolean_content = true;
        }

        let Some(child) = node.child(1) else { break };
        node = child;
        node_kind = node.kind_id().into();

        if matches!(node_kind, rust_bool_terminal_kinds!()) {
            if has_boolean_content {
                *conditions += 1.;
            }
            break;
        }
    }
}

// Phase-2B helpers (issue #403): classify a condition slot directly.
// Used for the `if (cond)` / `while (cond)` / `return value` arms —
// Fitzpatrick's Rule 6 / 7 ("unary conditional expression"). If the
// condition itself is a terminal-bool kind (`if true {}`, `if a {}`),
// it counts as one condition; if wrapped in `(...)` or `!...`,
// `rust_inspect_container` unwraps until a terminal is found. Mirrors
// the `java_count_condition` / `java_inspect_child` helper pair used
// by `java_walk_ternary` / `java_walk_for_statement`.
fn rust_count_condition(condition: &Node, parent: &Node, conditions: &mut f64) {
    use Rust::*;
    let kind = condition.kind_id().into();
    if matches!(kind, rust_bool_terminal_kinds!()) {
        *conditions += 1.;
    } else if matches!(kind, ParenthesizedExpression | UnaryExpression) {
        rust_inspect_container(condition, parent, conditions);
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
// different numbers. As a slot every spelling contributes exactly one —
// a call / field / index / bare identifier through
// `rust_bool_terminal_kinds!()`, a comparison through the token arm
// that already owns it — and a compound guard (`n if a > 1 && b`) keeps
// its sub-structure rather than collapsing to one.
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
// `LetCondition` token arm and by the `&&` walker respectively. Neither
// is a `rust_bool_terminal_kinds!()` member, so routing them through
// the slot adds nothing.
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
    if let Some(guard) = pattern.child_by_field_name("condition") {
        rust_count_condition(&guard, &pattern, conditions);
    }
}

fn rust_inspect_child(node: &Node, idx: usize, conditions: &mut f64) {
    if let Some(child) = node.child(idx) {
        rust_count_condition(&child, node, conditions);
    }
}

fn rust_count_unary_conditions(list_node: &Node, conditions: &mut f64) {
    use Rust::*;

    let list_kind = list_node.kind_id().into();
    let mut cursor = list_node.cursor();

    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();
            let node_kind = node.kind_id().into();

            // Allow `LetChain` (and its hidden-rule alias `LetChain2`)
            // alongside `BinaryExpression` as a known-boolean list
            // parent: a Rust 2024 let-chain `if a && let Some(x) = b`
            // makes `&&`'s parent the `LetChain` wrapper, not a
            // `BinaryExpression`. Without this, bare-identifier
            // operands inside a let-chain fall through and never
            // contribute to the condition count, while their
            // semantically equivalent `BinaryExpression` siblings do.
            if matches!(node_kind, rust_bool_terminal_kinds!())
                && matches!(list_kind, BinaryExpression | LetChain | LetChain2)
            {
                *conditions += 1.;
            } else if node.is_named() {
                rust_inspect_container(&node, list_node, conditions);
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
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
                if let Some(parent) = ancestors.parent(node) {
                    rust_count_unary_conditions(&parent, &mut stats.conditions);
                }
            }
            // Phase-2B (issue #403): Fitzpatrick Rule 6 / 7 condition
            // slots. `if true {}` / `if !a {}` count their condition
            // once via `rust_count_condition` (terminal-at-top or
            // paren / unary unwrap). The condition sits at child(1)
            // for `if_expression` and `while_expression` (child(0)
            // is the keyword). Rust has no `do_statement`, no
            // ternary, and no for-condition slot.
            IfExpression | WhileExpression => {
                rust_inspect_child(node, 1, &mut stats.conditions);
            }
            // `return value;` — `value` sits at child(1). Use the
            // bare `inspect_container` path (no top-level terminal
            // count) so that `return x` reports zero conditions
            // while `return !x` reports one. Matches Java's policy
            // (`java_return_without_conditions`): a bare identifier
            // in the return slot is not a unary conditional.
            ReturnExpression => {
                if let Some(value) = node.child(1) {
                    rust_inspect_container(&value, node, &mut stats.conditions);
                }
            }
            // Method-argument walker: `m(!a, !b)` contributes one
            // condition per unary-conditional argument.
            Arguments => {
                rust_count_unary_conditions(node, &mut stats.conditions);
            }
            _ => {}
        }
    }
}
