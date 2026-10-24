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
    Abc, Stats, count_boolean_slot, count_field_operands, count_negated_operand, last_operand,
    rows_own,
};
use crate::roles::ruby::{case_arm, comparison, guard, short_circuit, ternary};
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
fn ruby_count_condition(condition: &Node, code: &[u8], conditions: &mut f64) {
    count_boolean_slot(
        condition,
        ruby_wrapper_operand,
        |expr| ruby_condition_scores_itself(expr, code),
        conditions,
    );
}

// Whether another arm of `compute` already charges `expr` (already
// peeled through the wrappers `ruby_wrapper_operand` descends, which add
// no decision of their own) as a condition. True for a comparison (the
// `Comparison` row's owner, paid by its token), a one-line pattern test
// (the `TestPattern` arm), a ternary (the `?` arm plus
// `ruby_walk_ternary`) and an `&&` / `||` / `and` / `or` chain (the
// `ShortCircuit` row's owner): the chain walker scores each operand
// through `ruby_count_condition`, so every operand either pays there or
// is one of these, and a chain always carries at least one condition of
// its own. Neither owner gate reads ancestors, so the peeled descendant
// is asked with none.
fn ruby_condition_scores_itself(expr: &Node, code: &[u8]) -> bool {
    use Ruby::*;

    rows_own!(expr, code; comparison, short_circuit, ternary)
        || expr.kind_id() == TestPattern as u16
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
fn ruby_count_when<'a>(
    when: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    conditions: &mut f64,
) {
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
            ruby_count_condition(&expr, code, conditions);
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
fn ruby_walk_ternary(node: &Node, code: &[u8], conditions: &mut f64) {
    if let Some(condition) = node.child_by_field_name("condition") {
        ruby_count_condition(&condition, code, conditions);
    }
    for field in ["consequence", "alternative"] {
        if let Some(branch) = node.child_by_field_name(field) {
            count_negated_operand(
                &branch,
                ruby_wrapper_operand,
                |expr| ruby_condition_scores_itself(expr, code),
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
        // bca: suppress(cyclomatic, halstead) — exhaustive kind dispatch table
        // One arm per grammar kind, like `CppCode::compute`: the count is
        // the number of node kinds the Ruby grammar can hand us, not
        // branching a reader must hold, and the effort is the table's
        // vocabulary. The `Elsif` slot arm took it past
        // the limit; each arm is independent and there is no semantic
        // boundary to split this lookup on.
        use Ruby::*;

        let kind = node.kind_id();
        match kind.into() {
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
            // The guards are the `Guard` row's, read in the arm after this
            // one (`Guard` the hidden `_guard` supertype, listed there per
            // lesson 34). No double count (§5): the `if` / `unless`
            // *keyword tokens* a guard contains are anonymous tokens
            // distinct from the `If` / `Unless` statement kinds this arm
            // matches, so a guard reaches the slot exactly once.
            If | Unless | While | Until | IfModifier | UnlessModifier | WhileModifier
            | UntilModifier => {
                if let Some(cond) = node.child_by_field_name("condition") {
                    ruby_count_condition(&cond, code, &mut stats.conditions);
                }
            }
            // An `in` arm's guard, the `Guard` row's owner: its condition
            // is a slot as the predicates above are.
            _ if guard::owns_kind(kind, node, code, ancestors) => {
                if let Some(cond) = node.child_by_field_name("condition") {
                    ruby_count_condition(&cond, code, &mut stats.conditions);
                }
            }
            // A safe-navigation call is the `NullSafe` row's for its `&.`,
            // which ABC does not score; it is a branch as every call is.
            // role-bypass: allow(NullSafe) — the branch reading of every call, which the row's
            // safe-navigation owner is one of
            Call | Call2 | Call3 | Call4 | Super | Yield | Yield2 => {
                stats.branches += 1.;
            }
            // The ten comparison tokens, the `Comparison` row's payers
            // (`roles/ruby.rs`), each gated to a `binary` parent: every
            // one doubles as a method name in a `def`, and `<` as a
            // superclass clause (#1280) — the derivation is on the row.
            _ if comparison::pays_at_leaf_kind(kind, node, code, ancestors) => {
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
            Else | Rescue | RescueModifier | RescueModifier2 | RescueModifier3 | TestPattern => {
                stats.conditions += 1.;
            }
            // A ternary's `?`, the `Ternary` row's payer: the grammar
            // spells the bare token nowhere else.
            _ if ternary::pays_at_leaf_kind(kind, node, code, ancestors) => {
                stats.conditions += 1.;
            }
            // An `elsif` is Java's `else if`: the `else` (+1, Rule 5) and
            // an `if` predicate slot. It paid only the first, so `elsif b`
            // scored one below `else if (b)` while `elsif x > 0` matched it
            // through the comparison arm.
            Elsif => {
                stats.conditions += 1.;
                if let Some(cond) = node.child_by_field_name("condition") {
                    ruby_count_condition(&cond, code, &mut stats.conditions);
                }
            }
            // Each arm the `CaseArm` row owns is a branch condition
            // exactly when it counts toward cyclomatic, which reads the
            // same row: a `when` arm, whose subject-less `case` can
            // already have paid for the clause through its own operators
            // (#1453), and an `in` arm with a non-wildcard pattern or a
            // guard — the bare `in _` default arm is left out, as the
            // Python `case_clause` policy leaves its twin (#977).
            // role-bypass: allow(CaseArm) — which owner shape the arm is: a `when` walks its
            // patterns, an `in` pays at the node
            _ if case_arm::owns_kind(kind, node, code, ancestors) => {
                if kind == When as u16 {
                    ruby_count_when(node, code, ancestors, &mut stats.conditions);
                } else {
                    stats.conditions += 1.;
                }
            }
            // Fitzpatrick Rule 9 walker: each operand of a `&&` / `||` /
            // `and` / `or` chain is a boolean slot (issue #557, #1529).
            // The short-circuit operators are not counted directly
            // (cross-language policy, #395); the keyword forms `and` / `or`
            // get the same treatment as `&&` / `||`. Under error recovery
            // the token's parent can be an `ERROR` node (`f(&&)`), which
            // has no operands to score. tree-sitter-ruby parses
            // `a && b || c` as a left-nested chain of `binary` nodes
            // (aliased `Binary`..`Binary3`, lesson #2), so an operand that
            // is itself a chain is paid by its own operator's visit.
            _ if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors) => {
                if let Some(chain) = ancestors
                    .parent(node)
                    .filter(|p| short_circuit::owns(p, code, Ancestors::unknown()))
                {
                    count_field_operands(
                        &chain,
                        |operand, conds| ruby_count_condition(operand, code, conds),
                        &mut stats.conditions,
                    );
                }
            }
            // `a ? !b : !c` — the `conditional` the `Ternary` row owns:
            // its `?` is already paid by the row's arm above; this walks
            // the three operand slots (issue #1161).
            _ if ternary::owns_kind(kind, node, code, ancestors) => {
                ruby_walk_ternary(node, code, &mut stats.conditions);
            }
            _ => {}
        }
    }
}
