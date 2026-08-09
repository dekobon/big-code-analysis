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
use crate::macros::ruby_bool_terminal_kinds;
use crate::*;

// Ruby ABC rules follow the Fitzpatrick paper's spirit, adapted to
// tree-sitter-ruby:
// - Assignments: `assignment` (plain `=`) and `operator_assignment`
//   (`+=`, `-=`, `||=`, `&&=`, …). Tree-sitter wraps both forms in a
//   dedicated node, so we count one assignment per node and avoid
//   double-counting the inner `=` / augmented token.
// - Branches: every Ruby method invocation kind (`Call` / `Call2` /
//   `Call3` / `Call4`) plus `super` and `yield`. `yield` is grammar-
//   level a "block invocation" but ABC's branch bucket is "message
//   pass / function call", so it belongs here. `attr_*` macros are
//   `Call3` nodes and are counted as branches like any other call.
// - Conditions: comparison and equality operator tokens emitted inside
//   `binary` (`==`, `!=`, `===`, `<`, `>`, `<=`, `>=`, `<=>`,
//   `=~`, `!~`), plus the control-flow arms that the Fitzpatrick rules
//   list — the named clause nodes `Else` / `Elsif` / `When` and the
//   `?` ternary marker, plus `Rescue` (the rescue clause) and rescue
//   modifiers. `if` / `unless` themselves are not counted (the head
//   condition appears as the inner comparison); the `Then` clause is
//   an implicit grammar wrapper around every `if` / `elsif` body and
//   is NOT counted as a separate arm.
// Ruby ABC unary-conditional walker (Fitzpatrick Rule 9; issue #557).
// tree-sitter-ruby parses `a && b || c` as a left-nested chain of
// `binary` nodes carrying `&&` / `||` / `and` / `or` operator tokens
// (the `binary` kind is aliased `Binary`..`Binary3` per lesson #2, so
// every alias must be matched). Negation surfaces as `unary`
// (`Unary`..`Unary5`) whose child(0) is the `!` token; the condition
// slot may be wrapped in `parenthesized_statements`. Both are unwrapped
// by `ruby_inspect_container`.
fn ruby_inspect_container(container_node: &Node, parent: &Node, conditions: &mut f64) {
    // bca: suppress(cognitive) — wrapper-peeling state machine, clearest whole
    // See `cpp_inspect_container` for the shared rationale: one loop peels
    // `(...)` / `!...` layers while carrying a single boolean-context flag.
    use Ruby::*;

    let mut node = *container_node;
    let mut node_kind = node.kind_id().into();
    let parent_kind = parent.kind_id().into();
    // A ternary seeds boolean context for its condition slot alone: the
    // two branch operands are type-free, so an unnegated branch must
    // contribute nothing — see `ruby_walk_ternary` (#1161).
    //
    // The slot is identified by grammar FIELD, not by the neighbouring
    // `?` / `:` token. The token form had two weaknesses this avoids: a
    // comment between the token and the operand is the previous sibling
    // instead, which flips the seed on for a branch slot; and the test
    // inverts on failure, so Ruby's second `:` id (`COLON2`, unreachable
    // at tree-sitter-ruby 0.23.1 but one grammar bump away) would
    // silently turn every parenthesised alternative into a condition.
    // Both were live across the C family, PHP, Perl and the JS family
    // until #1181 moved them all onto this form; the cross-language
    // regression test is `ternary_comment_invariance` in `abc.rs`.
    let mut has_boolean_content = matches!(
        parent_kind,
        Binary | Binary2 | Binary3 | If | Unless | While | Until
    ) || (matches!(parent_kind, Conditional)
        && parent
            .child_by_field_name("condition")
            .is_some_and(|condition| condition.id() == node.id()));

    loop {
        let is_parens = matches!(node_kind, ParenthesizedStatements);
        // Both spellings of the same negation. `not` and `!` differ in
        // precedence but not in meaning, and ABC counts the negation,
        // not the parse — testing `BANG` alone scored `if not b` as 0
        // where `if !b` scores 1, and a `not` ternary as 2 where the `!`
        // form scores 4 (#1182). Read through the grammar's `operator`
        // field rather than child(0), matching the ternary slots (#1181).
        let is_not = matches!(node_kind, Unary | Unary2 | Unary3 | Unary4 | Unary5)
            && node
                .child_by_field_name("operator")
                .is_some_and(|op| matches!(op.kind_id().into(), BANG | Not));

        if !is_parens && !is_not {
            break;
        }
        if !has_boolean_content && is_not {
            has_boolean_content = true;
        }

        // A `!` unary stores its operand at child index 1 (after the `!`
        // token). `parenthesized_statements` wraps its body in named
        // children; descend through the first named child carrying the
        // expression.
        let next = if is_not {
            node.child(1)
        } else {
            node.children().find(Node::is_named)
        };
        let Some(child) = next else { break };
        node = child;
        node_kind = node.kind_id().into();

        if matches!(node_kind, ruby_bool_terminal_kinds!()) {
            if has_boolean_content {
                *conditions += 1.;
            }
            break;
        }
    }
}

// Counts each non-comparison operand of a Ruby `&&` / `||` chain once.
// Comparison operands are nested `binary` nodes (absent from
// `ruby_bool_terminal_kinds!()`) and so contribute nothing.
fn ruby_count_unary_conditions(list_node: &Node, conditions: &mut f64) {
    use Ruby::*;

    let list_kind = list_node.kind_id().into();
    let mut cursor = list_node.cursor();

    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();
            let node_kind = node.kind_id().into();

            if matches!(node_kind, ruby_bool_terminal_kinds!())
                && matches!(list_kind, Binary | Binary2 | Binary3)
            {
                *conditions += 1.;
            } else if node.is_named() {
                ruby_inspect_container(&node, list_node, conditions);
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

// Count the bare-predicate condition of a Ruby `if`/`unless`/`while`/
// `until` (block or modifier form) as one condition — Fitzpatrick's
// "unary conditional expression" (Rule 6 / 7), mirroring Rust's
// `rust_count_condition`. tree-sitter-ruby exposes the predicate via the
// `condition` field for every form (block `if cond ... end` and modifier
// `body if cond`), so the field lookup is position-independent. A bare
// terminal (`if flag`) counts directly; a comparison / boolean chain
// (`if a == b`, `if a && b`) is a nested `binary` node already counted by
// the comparison-operator and `&&`/`||` walker arms, so it adds nothing
// here. A parenthesised or negated predicate (`if (flag)`, `if !flag`) is
// unwrapped by `ruby_inspect_container`. Without this arm, idiomatic Ruby
// bare predicates reported 0 ABC conditions while Ruby's own cyclomatic
// counted them, breaking the conditions >= decisions invariant
// (#469/#473/#456); issue #696.
fn ruby_count_condition(condition: &Node, parent: &Node, conditions: &mut f64) {
    use Ruby::*;
    let kind = condition.kind_id().into();
    if matches!(kind, ruby_bool_terminal_kinds!()) {
        *conditions += 1.;
    } else if matches!(
        kind,
        ParenthesizedStatements | Unary | Unary2 | Unary3 | Unary4 | Unary5
    ) {
        ruby_inspect_container(condition, parent, conditions);
    }
}

// Phase-2B (issues #403 / #1102 / #1161): a Ruby ternary's condition and
// its two branch operands are each a Fitzpatrick Rule 9 unary condition,
// exactly as `cpp_walk_ternary` counts them for the C family. Without
// this, Ruby scored `a ? !b : !c` as 1 — the `?` token alone — against
// Java's 4, and `ruby_inspect_container`'s `Conditional` boolean-context
// seed was unreachable.
//
// Slots are addressed by grammar field, per
// `.claude/rules/grammar-dispatch.md` item 3.
//
// The condition slot reuses `ruby_count_condition`, so a ternary's
// predicate is classified by exactly the code that classifies an
// `if` / `unless` / `while` / `until` predicate, and the two cannot
// drift.
//
// Both branch slots route through `ruby_inspect_container` rather than
// testing for the `Unary` kind here: `-b` and `!b` are the SAME node
// kind (`unary`), distinguished only by child(0) being `-` rather than
// `!`. Keying on the kind takes the control case `(a > 0) ? b : -b`
// from 2 to 3.
fn ruby_walk_ternary(node: &Node, conditions: &mut f64) {
    if let Some(condition) = node.child_by_field_name("condition") {
        ruby_count_condition(&condition, node, conditions);
    }
    // Branch operands carry no terminal check: an unnegated branch is
    // type-free and contributes nothing, which is what keeps
    // `(a > 0) ? b : -b` at 2 (the `?` and the `>`).
    for field in ["consequence", "alternative"] {
        if let Some(branch) = node.child_by_field_name(field) {
            ruby_inspect_container(&branch, node, conditions);
        }
    }
}

impl Abc for RubyCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Ruby::*;

        match node.kind_id().into() {
            Assignment | Assignment2 | OperatorAssignment | OperatorAssignment2 => {
                stats.assignments += 1.;
            }
            // The bare predicate of every `if`/`unless`/`while`/`until`
            // form (block and modifier) is one unary condition. The
            // `condition` field locates the predicate position-
            // independently across all eight node kinds (#696).
            If | Unless | While | Until | IfModifier | UnlessModifier | WhileModifier
            | UntilModifier => {
                if let Some(cond) = node.child_by_field_name("condition") {
                    ruby_count_condition(&cond, node, &mut stats.conditions);
                }
            }
            Call | Call2 | Call3 | Call4 | Super | Yield | Yield2 => {
                stats.branches += 1.;
            }
            EQEQ | BANGEQ | EQEQEQ | LTEQ | GTEQ | LTEQGT | EQTILDE | BANGTILDE | Else | Elsif
            | When | QMARK | Rescue | RescueModifier | RescueModifier2 | RescueModifier3 => {
                stats.conditions += 1.;
            }
            // `<` and `>` are comparisons only inside a `binary` node. The
            // same `<` token also spells a class's superclass clause
            // (`class Foo < Bar`, parent `superclass`) and an operator
            // method's name (`def <(other)`, parent `operator`), neither of
            // which is a condition — the positive `binary` guard is the
            // polarity Rust / Go / C / C++ already use, and stays correct
            // for any future non-comparison `<` (#1280). `a < b` emits the
            // `Binary2` alias; the two siblings are listed defensively per
            // grammar-dispatch §1. `<<` is the distinct `LTLT` token and
            // heredoc openers are their own tokens, so neither is affected.
            LT | GT
                if ancestors
                    .parent(node)
                    .is_some_and(|p| matches!(p.kind_id().into(), Binary | Binary2 | Binary3)) =>
            {
                stats.conditions += 1.;
            }
            // A `case … in` pattern-match arm is a branch condition exactly
            // when it counts toward cyclomatic — a non-wildcard pattern or
            // a guarded arm. The bare `in _` default arm is filtered out,
            // keeping ABC and cyclomatic in lockstep on the same construct
            // and matching the Python `case_clause` policy (#977).
            InClause if crate::metrics::npa::ruby_in_clause_counts(node, code) => {
                stats.conditions += 1.;
            }
            // Fitzpatrick Rule 9 walker: each non-comparison operand of a
            // `&&` / `||` / `and` / `or` chain is one condition (issue
            // #557). The short-circuit operators are not counted directly
            // (cross-language policy, #395); the keyword forms `and` / `or`
            // get the same treatment as `&&` / `||`.
            AMPAMP | PIPEPIPE | And | Or => {
                if let Some(parent) = ancestors.parent(node) {
                    ruby_count_unary_conditions(&parent, &mut stats.conditions);
                }
            }
            // `a ? !b : !c` — the ternary's own `?` token is already
            // counted by the token arm above; this walks the three
            // operand slots (issue #1161).
            Conditional => {
                ruby_walk_ternary(node, &mut stats.conditions);
            }
            _ => {}
        }
    }
}
