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

use super::{Abc, Stats, last_operand};
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
//   `=~`, `!~`) — the `binary` parent is a gate, not a description, since
//   every one of those tokens also names an operator method in a `def`
//   and `<` also spells a superclass clause (#1280) — plus the
//   control-flow arms that the Fitzpatrick rules
//   list — the named clause nodes `Else` / `Elsif` / `When` (see
//   `ruby_count_when` for a subject-less `case`) and the
//   `?` ternary marker, plus `Rescue` (the rescue clause) and rescue
//   modifiers. `if` / `unless` themselves are not counted (the head
//   condition appears as the inner comparison); the `Then` clause is
//   an implicit grammar wrapper around every `if` / `elsif` body and
//   is NOT counted as a separate arm.

// One step of the `(...)` / negation peel: the operand a wrapper wraps,
// and whether the wrapper itself proves that operand boolean. `None`
// for anything that is not a wrapper this peel descends, which is also
// the answer `ruby_count_condition` asks for — it routes exactly the
// kinds this function accepts, so the two cannot disagree (#1470; the
// Kotlin, Groovy and C# instances of that disagreement were #1459,
// #1466 and #1463).
//
// Both spellings of the same negation. `not` and `!` differ in
// precedence but not in meaning, and ABC counts the negation, not the
// parse — testing `BANG` alone scored `if not b` as 0 where `if !b`
// scores 1, and a `not` ternary as 2 where the `!` form scores 4
// (#1182). The operator is read through the grammar's `operator` field
// rather than child(0), matching the ternary slots (#1181). The other
// `unary` operators (`-b`, `~b`, `defined?`) are arithmetic or
// introspection, never a boolean slot's operand, so the peel declines.
//
// A negation names its `operand`; `parenthesized_statements` wraps a
// statement sequence and evaluates to its *last* statement (`(x; y)` is
// `y`), so the peel reads its `last_operand`, as Perl's peel does for
// a list. Both are read by role because a comment may sit before the
// operand — `(# c` or `! # c` — and a positional or first-named-child
// read handed the comment to the slot (#1455).
fn ruby_wrapper_operand<'a>(node: &Node<'a>) -> Option<(Node<'a>, bool)> {
    use Ruby::*;

    match node.kind_id().into() {
        ParenthesizedStatements => Some((last_operand(node)?, false)),
        Unary | Unary2 | Unary3 | Unary4 | Unary5 => {
            match node.child_by_field_name("operator")?.kind_id().into() {
                BANG | Not => Some((node.child_by_field_name("operand")?, true)),
                _ => None,
            }
        }
        _ => None,
    }
}

// Ruby ABC unary-conditional walker (Fitzpatrick Rule 9; issue #557).
// tree-sitter-ruby parses `a && b || c` as a left-nested chain of
// `binary` nodes carrying `&&` / `||` / `and` / `or` operator tokens
// (the `binary` kind is aliased `Binary`..`Binary3` per lesson #2, so
// every alias must be matched). Negation surfaces as `unary`
// (`Unary`..`Unary5`); the condition slot may be wrapped in
// `parenthesized_statements`. Both are unwrapped one layer at a time by
// `ruby_wrapper_operand`.
fn ruby_inspect_container(container_node: &Node, parent: &Node, conditions: &mut f64) {
    use Ruby::*;

    let mut node = *container_node;
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
    // The three guard kinds joined this list with #1454: a `case … in`
    // arm's `if` / `unless` guard is a boolean slot exactly as an `if`
    // predicate is, so a parenthesised guard operand (`in [x] if (b)`)
    // counts where the bare `in [x] if b` already did. `Guard` is the
    // hidden `_guard` supertype, listed defensively (§2).
    let mut has_boolean_content = matches!(
        parent_kind,
        Binary | Binary2 | Binary3 | If | Unless | While | Until | Guard | IfGuard | UnlessGuard
    ) || (matches!(parent_kind, Conditional)
        && parent
            .child_by_field_name("condition")
            .is_some_and(|condition| condition.id() == node.id()));

    while let Some((operand, proves_boolean)) = ruby_wrapper_operand(&node) {
        has_boolean_content |= proves_boolean;
        node = operand;

        if matches!(node.kind_id().into(), ruby_bool_terminal_kinds!()) {
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
    if matches!(condition.kind_id().into(), ruby_bool_terminal_kinds!()) {
        *conditions += 1.;
    } else if ruby_wrapper_operand(condition).is_some() {
        // Asking the peel itself which kinds it unwraps, rather than
        // restating the list here (#1470). A restated list that gained
        // a kind the peel lacked would read as covering a shape the
        // peel then dropped on the floor (`.claude/rules/
        // grammar-dispatch.md` §7) — the Kotlin and Groovy defects of
        // #1459 / #1466.
        ruby_inspect_container(condition, parent, conditions);
    }
}

// The comparison / equality tokens that are one condition each inside a
// `binary`. Shared by the token arm in `compute` and by
// `ruby_condition_scores_itself`, which must agree with that arm on
// exactly which operators it charges.
macro_rules! ruby_comparison_kinds {
    () => {
        Ruby::EQEQ
            | Ruby::BANGEQ
            | Ruby::EQEQEQ
            | Ruby::LT
            | Ruby::GT
            | Ruby::LTEQ
            | Ruby::GTEQ
            | Ruby::LTEQGT
            | Ruby::EQTILDE
            | Ruby::BANGTILDE
    };
}

// Whether another arm of `compute` already charges `expr` as a condition,
// looking through `(…)` and `!` / `not` layers, which add no decision of
// their own. True for a comparison (the token arm), an `&&` / `||` /
// `and` / `or` chain (the Rule 9 walker), a one-line pattern test (the
// `TestPattern` arm) and a ternary (the `?` arm plus `ruby_walk_ternary`)
// — exactly the expressions the `if` predicate slot leaves to those arms.
//
// It answers "is the decision already paid for", not "is this a boolean
// slot", which is why a subject-less `when` asks this rather than routing
// through `ruby_count_condition`. That slot scores zero for any shape
// outside `ruby_bool_terminal_kinds!()` — `Foo::Bar`, `self`, `-x`,
// `defined?(x)`, `(y = x)`, `*xs`, `1..2` — so delegating to it would
// have moved each of those `when`s from 1 to 0 against an unchanged
// cyclomatic decision (measured for #1453). The `if` forms of those
// shapes do score 0 today; that is the slot's gap (#1520), not this
// arm's to copy.
fn ruby_condition_scores_itself(expr: &Node) -> bool {
    use Ruby::*;

    let mut node = *expr;
    loop {
        match node.kind_id().into() {
            Binary | Binary2 | Binary3 => {
                return node.child_by_field_name("operator").is_some_and(|op| {
                    let op = op.kind_id().into();
                    matches!(op, ruby_comparison_kinds!())
                        || matches!(op, AMPAMP | PIPEPIPE | And | Or)
                });
            }
            TestPattern | Conditional => return true,
            _ => {}
        }
        // The same wrappers the condition peel descends, by construction.
        let Some((operand, _)) = ruby_wrapper_operand(&node) else {
            return false;
        };
        node = operand;
    }
}

// Scores one `when` clause (#1453, transferring #1421's Kotlin rule).
// Both `case` shapes contribute the single decision cyclomatic counts per
// clause, but they pay for it in different places.
//
// A subject-ful clause (`case x; when 1`) lists a *pattern* matched with
// `pattern === x`: that comparison is written nowhere in the source, so
// the clause itself is the condition.
//
// A subject-less clause (`case; when x > 5`) lists an ordinary boolean
// expression, evaluated exactly as an `if` predicate. When that
// expression is a comparison, chain, pattern test or ternary, its own arm
// has already counted it, and the clause's former blanket +1 counted the
// same decision twice: `case; when x > 5 then 1; else 0; end` scored 3
// against its `if` analogue's 2. Otherwise — a bare `when b`, `(b)`,
// `!b`, `x.even?` — nothing else sees it and the clause still pays.
//
// A clause may list several patterns (`when a, b`): an implicit `||`, so
// each pattern is one operand of that chain and is scored as Rule 9
// scores an `||` operand — once, unless its own arm already counted it.
// `when a, b` / `when a, x > 1` / `when x > 1, x < -1` therefore each
// read 2, exactly their `if a || b` analogues, and the order the patterns
// are written in cannot matter. Cyclomatic scores the clause as one
// decision, so these sit above `conditions == cyclomatic - 1`, as the
// `||` analogues do.
//
// A `when` whose parent is not a `case` occurs only under error
// recovery; it keeps the per-clause count rather than guessing.
fn ruby_count_when<'a>(when: &Node<'a>, ancestors: Ancestors<'a, '_>, conditions: &mut f64) {
    let subject_less = ancestors.parent(when).is_some_and(|case| {
        case.kind_id() == Ruby::Case && case.child_by_field_name("value").is_none()
    });
    if !subject_less {
        *conditions += 1.;
        return;
    }
    // `pattern` wraps exactly one expression, so an extra (a comment) can
    // sit beside it under the `when` but never inside it.
    for pattern in when.children().filter(|c| c.kind_id() == Ruby::Pattern) {
        if pattern
            .child(0)
            .is_some_and(|expr| !ruby_condition_scores_itself(&expr))
        {
            *conditions += 1.;
        }
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
            //
            // The two `case … in` guard kinds share the arm: `if_guard`
            // and `unless_guard` each expose their predicate through the
            // same `condition` field, so a guard is a condition slot
            // classified by exactly the code that classifies an `if`
            // predicate (#1454, transferring #1422's C# rule). Before
            // this, a guard scored whatever operator happened to sit
            // inside it: `in [x] if x > 2` counted one via the
            // comparison-token arm while `in [x] if x.even?` and
            // `in [x] if b` counted zero, so three semantically
            // identical guards produced two different numbers. As a slot
            // every spelling contributes exactly one — a call /
            // identifier / ivar / element reference through
            // `ruby_bool_terminal_kinds!()`, a comparison through the
            // token arm that already owns it — and a compound guard
            // keeps its sub-structure rather than collapsing to one.
            //
            // `Guard` is the hidden `_guard` supertype (§2, lesson #34);
            // it is listed for the same defensive reason
            // `ruby_in_clause_counts` lists it.
            //
            // No double count (§5): the `if` / `unless` *keyword tokens*
            // a guard contains are anonymous tokens distinct from the
            // `If` / `Unless` statement kinds this arm matches, so a
            // guard reaches the slot exactly once.
            If | Unless | While | Until | IfModifier | UnlessModifier | WhileModifier
            | UntilModifier | Guard | IfGuard | UnlessGuard => {
                if let Some(cond) = node.child_by_field_name("condition") {
                    ruby_count_condition(&cond, node, &mut stats.conditions);
                }
            }
            Call | Call2 | Call3 | Call4 | Super | Yield | Yield2 => {
                stats.branches += 1.;
            }
            // A comparison / equality token is a condition only inside a
            // `binary` node. Every one of them doubles as a *method name*
            // in a `def` (`def <(other)`, `def ==(other)`, …), where the
            // grammar parents it under `operator`; `<` additionally spells
            // a class's superclass clause (`class Foo < Bar`, parent
            // `superclass`). Neither is a condition — the positive
            // `binary` guard is the polarity Rust / Go / C / C++ already
            // use, and stays correct for any future non-comparison
            // spelling (#1280). `a < b` emits the `Binary2` alias; the two
            // siblings are listed defensively per grammar-dispatch §1.
            // `<<` is the distinct `LTLT` token and heredoc openers are
            // their own tokens, so neither is affected.
            ruby_comparison_kinds!()
                if ancestors
                    .parent(node)
                    .is_some_and(|p| matches!(p.kind_id().into(), Binary | Binary2 | Binary3)) =>
            {
                stats.conditions += 1.;
            }
            // Ruby 3.0's one-line pattern test (`a in Integer`) joins
            // them in #1461, scored by use rather than by slot. It sat in
            // `ruby_bool_terminal_kinds!()`, which counts only inside a
            // boolean slot, so `b = a in Integer` scored zero where the
            // `b = a == 1` beside it scored one — every comparison above
            // is a token arm and `in` was not. Fitzpatrick Rule 5 scores
            // a relational operator wherever it is written.
            //
            // Matched as the node rather than as the `in` token, which
            // the language also spells in `for x in xs` and in the
            // `in_clause` of a `case`/`in`. Those are separate
            // productions (`bca dump`), so the `InClause` arm below
            // never sees a `test_pattern` and exactly one arm fires per
            // test (§5).
            //
            // It shares the arm rather than sitting beside it because
            // the arm's meaning is "this node is a condition, with
            // nothing to gate on" — as true of a production as of a
            // token.
            Else | Elsif | QMARK | Rescue | RescueModifier | RescueModifier2 | RescueModifier3
            | TestPattern => {
                stats.conditions += 1.;
            }
            // A subject-less `case` can already have paid for its clause
            // through the clause's own operators (#1453).
            When => ruby_count_when(node, ancestors, &mut stats.conditions),
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
