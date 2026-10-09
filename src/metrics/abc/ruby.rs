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

use super::{Abc, Stats, count_boolean_slot, count_negated_operand, last_operand};
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
//   modifiers. An `if` / `unless` / `while` / `until` predicate pays
//   one condition unless another arm already charged it — see
//   `ruby_count_condition`; the `Then` clause is an implicit grammar
//   wrapper around every `if` / `elsif` body and is NOT counted as a
//   separate arm.

// One step of the value peel: the operand a wrapper evaluates to,
// and whether the wrapper itself proves that operand boolean. `None`
// for anything that is not a wrapper this peel descends. The chain
// walker and the slot both descend through this one function, so what
// the slot decides is already paid for and what the walker actually
// counts cannot disagree (#1470; the Kotlin, Groovy
// and C# instances of that disagreement were #1459, #1466 and #1463).
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
//
// `begin … end` evaluates to its last statement exactly as `(…)` does,
// and an assignment to its `right` value (#1520). Both are peeled so
// `ruby_condition_scores_itself` sees the comparison inside
// `if begin a > 1 end` / `if (y = a > 1)` — without that, the slot
// would pay a second time for a decision the comparison token already
// counted. A `begin` whose last child is a `rescue` / `else` / `ensure`
// clause peels to that clause, which is no wrapper, so the peel stops.
fn ruby_wrapper_operand<'a>(node: &Node<'a>) -> Option<(Node<'a>, bool)> {
    use Ruby::*;

    match node.kind_id().into() {
        ParenthesizedStatements | Begin => last_operand(node).map(|o| (o, false)),
        Assignment | Assignment2 => node.child_by_field_name("right").map(|o| (o, false)),
        Unary | Unary2 | Unary3 | Unary4 | Unary5 => node
            .child_by_field_name("operator")
            .filter(|op| matches!(op.kind_id().into(), BANG | Not))
            .and_then(|_| node.child_by_field_name("operand"))
            .map(|o| (o, true)),
        _ => None,
    }
}

// Fitzpatrick Rule 9 walker (#557): each operand of an `&&` / `||` /
// `and` / `or` chain is a boolean slot, scored by `ruby_count_condition`.
// tree-sitter-ruby parses `a && b || c` as a left-nested chain of
// `binary` nodes (aliased `Binary`..`Binary3`, lesson #2), so an operand
// that is itself a chain is paid by its own operator's visit.
//
// The operands are read by field: a comment between operand and operator
// is a named child of the `binary` too, and must not pay.
fn ruby_count_chain_operands(chain: &Node, conditions: &mut f64) {
    for field in ["left", "right"] {
        if let Some(operand) = chain.child_by_field_name(field) {
            ruby_count_condition(&operand, conditions);
        }
    }
}

// Scores one boolean slot — the predicate of every `if` / `unless` /
// `while` / `until` form (block and modifier), a `case … in` guard, a
// ternary condition, each pattern of a subject-less `when`, and each
// operand of an `&&` / `||` chain — as Fitzpatrick's "unary conditional
// expression" (Rule 6 / 7 / 9): one condition, unless another arm
// already charged the same decision (see `count_boolean_slot`).
//
// The slot once paid only for a predicate that peeled down to a fixed
// list of terminal kinds, so `if Foo::Bar`, `if self`, `if -x`,
// `if defined?(x)`, `if (y = x)`, `if x + 1` and `g if begin b end` each
// scored 0 against a cyclomatic decision of 1, while the same expression
// as a subject-less `when` scored 1 (#1520); the chain walker kept that
// list until #1529, so `a && self` scored one below `a && b`. #1526
// moved the rule into `count_boolean_slot` and ported it to every
// sibling.
fn ruby_count_condition(condition: &Node, conditions: &mut f64) {
    count_boolean_slot(
        condition,
        ruby_wrapper_operand,
        ruby_condition_scores_itself,
        conditions,
    );
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

// Whether another arm of `compute` already charges `expr` (already
// peeled through the wrappers `ruby_wrapper_operand` descends, which add
// no decision of their own) as a condition. True for a comparison (the
// token arm), a
// one-line pattern test (the `TestPattern` arm), a ternary (the `?` arm
// plus `ruby_walk_ternary`) and an `&&` / `||` / `and` / `or` chain:
// the chain walker scores each operand through `ruby_count_condition`,
// so every operand either pays there or is one of these, and a chain
// always carries at least one condition of its own.
fn ruby_condition_scores_itself(expr: &Node) -> bool {
    use Ruby::*;

    match expr.kind_id().into() {
        Binary | Binary2 | Binary3 => expr.child_by_field_name("operator").is_some_and(|op| {
            let kind: Ruby = op.kind_id().into();
            matches!(kind, ruby_comparison_kinds!()) || matches!(kind, AMPAMP | PIPEPIPE | And | Or)
        }),
        TestPattern | Conditional => true,
        _ => false,
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
        if let Some(expr) = pattern.child(0) {
            ruby_count_condition(&expr, conditions);
        }
    }
}

// Phase-2B (issues #403 / #1102 / #1161): a Ruby ternary's condition and
// its two branch operands are each a Fitzpatrick Rule 9 unary condition,
// exactly as `cpp_walk_ternary` counts them for the C family. Without
// this, Ruby scored `a ? !b : !c` as 1 — the `?` token alone — against
// Java's 4.
//
// Slots are addressed by grammar field, per
// `.claude/rules/grammar-dispatch.md` item 3.
//
// The condition slot reuses `ruby_count_condition`, so a ternary's
// predicate is classified by exactly the code that classifies an
// `if` / `unless` / `while` / `until` predicate, and the two cannot
// drift.
//
// A branch slot is type-free: it is a boolean slot only when the peel
// proves it one — a `!` / `not` layer. Keying on the `unary` kind instead
// would also take `-b` (the same kind, a different operator), moving the
// control case `(a > 0) ? b : -b` from 2 to 3.
fn ruby_walk_ternary(node: &Node, conditions: &mut f64) {
    if let Some(condition) = node.child_by_field_name("condition") {
        ruby_count_condition(&condition, conditions);
    }
    for field in ["consequence", "alternative"] {
        if let Some(branch) = node.child_by_field_name(field) {
            count_negated_operand(
                &branch,
                ruby_wrapper_operand,
                ruby_condition_scores_itself,
                conditions,
            );
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
        // bca: suppress(cyclomatic) — exhaustive kind dispatch table
        // One arm per grammar kind, like `CppCode::compute`: the count is
        // the number of node kinds the Ruby grammar can hand us, not
        // branching a reader must hold. The `Elsif` slot arm took it past
        // the limit; each arm is independent and there is no semantic
        // boundary to split this lookup on.
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
            // every spelling contributes exactly one — the slot's own
            // one for anything no other arm counts, or a comparison
            // through the token arm that already owns it — and a
            // compound guard keeps its sub-structure rather than
            // collapsing to one.
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
                    ruby_count_condition(&cond, &mut stats.conditions);
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
            // the walker's terminal-kind list, which counted only inside
            // a boolean slot, so `b = a in Integer` scored zero where the
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
            Else | QMARK | Rescue | RescueModifier | RescueModifier2 | RescueModifier3
            | TestPattern => {
                stats.conditions += 1.;
            }
            // An `elsif` is Java's `else if`: the `else` (+1, Rule 5) and
            // an `if` predicate slot. It paid only the first, so `elsif b`
            // scored one below `else if (b)` while `elsif x > 0` matched it
            // through the comparison arm.
            Elsif => {
                stats.conditions += 1.;
                if let Some(cond) = node.child_by_field_name("condition") {
                    ruby_count_condition(&cond, &mut stats.conditions);
                }
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
            // Fitzpatrick Rule 9 walker: each operand of a `&&` / `||` /
            // `and` / `or` chain is a boolean slot (issue #557, #1529).
            // The short-circuit operators are not counted directly
            // (cross-language policy, #395); the keyword forms `and` / `or`
            // get the same treatment as `&&` / `||`. Under error recovery
            // the token's parent can be an `ERROR` node (`f(&&)`), which
            // has no operands to score.
            AMPAMP | PIPEPIPE | And | Or => {
                if let Some(chain) = ancestors
                    .parent(node)
                    .filter(|p| matches!(p.kind_id().into(), Binary | Binary2 | Binary3))
                {
                    ruby_count_chain_operands(&chain, &mut stats.conditions);
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
