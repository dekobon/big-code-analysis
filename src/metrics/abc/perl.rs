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
use crate::macros::perl_bool_terminal_kinds;
use crate::*;

// Fitzpatrick's ABC rules adapted for Perl.
//
// - Assignments: every assignment operator token — plain `=` plus the
//   compound forms `+=`, `-=`, `*=`, `/=`, `%=`, `**=`, `.=`, `x=`,
//   `&=`, `|=`, `^=`, `<<=`, `>>=`, `&&=`, `||=`, `//=`, and the
//   bitstring forms `&.=`, `|.=`, `^.=`. Each token fires exactly
//   once per textual occurrence inside a `binary_expression`.
// - Branches: every call expression dispatch — `call_expression_with_*`
//   (bareword / spaced args / args-with-brackets / sub / variable /
//   recursive) plus `method_invocation`. The grammar nests an inner
//   `call_expression_with_bareword` (just the function name)
//   underneath the wrapper kinds carrying argument lists, so we only
//   count `CallExpressionWithBareword` when it stands on its own;
//   when its parent is another call form, the outer wrapper has
//   already contributed the branch.
// - Conditions: numeric and string comparison operators (`==`, `!=`,
//   `<`, `>`, `<=`, `>=`, `<=>`, `eq`, `ne`, `lt`, `gt`, `le`, `ge`,
//   `cmp`, `=~`, `!~`), the ternary operator (`TernaryExpression`),
//   and each `elsif` / `else` clause of an `if` / `unless`
//   statement. Bare predicates that have no comparison (e.g.
//   `if ($x)`) are not separately counted; we let the comparison
//   tokens carry the metric, mirroring the Bash / Python token-
//   level approach.
//
//   The short-circuit and low-precedence logical operators (`&&`,
//   `||`, `//`, `and`, `or`, `xor`) are deliberately NOT counted.
//   See the module-level `Stats` doc-comment for the cross-
//   language policy (Fitzpatrick rules mapped from Figure 2 for C,
//   the closest analogue since the paper does not define rules for
//   Perl; issue #395, walker tracked in #403).
// Perl ABC unary-conditional walker (Fitzpatrick Rule 9 mapped from
// Figure 2 for C — the closest analogue, since the paper does not
// define rules for Perl; issue #403). Logical-operator triggers cover
// both the high-precedence punctuation (`&&`, `||`, `//`) and the
// low-precedence keyword forms (`and`, `or`, `xor`). Terminal-bool
// kinds: `Identifier`, `Boolean`, `True`, `False`, the call-expression
// wrappers (every kind already counted as a branch), and the variable
// wrappers (`ScalarVariable`, `ArrayVariable`, `HashVariable` plus the
// access shapes).
fn perl_inspect_container(container_node: &Node, parent: &Node, conditions: &mut f64) {
    // bca: suppress(cognitive) — wrapper-peeling state machine, clearest whole
    // See `cpp_inspect_container` for the shared rationale: one loop peels
    // `(...)` / `!...` layers while carrying a single boolean-context flag.
    use Perl as P;

    let mut node = *container_node;
    let mut node_kind = node.kind_id().into();
    let parent_kind = parent.kind_id().into();
    let mut has_boolean_content = matches!(
        parent_kind,
        P::BinaryExpression
            | P::IfStatement
            | P::UnlessStatement
            | P::WhileStatement
            | P::UntilStatement
            | P::ForStatement1
    ) || (matches!(parent_kind, P::TernaryExpression)
        && parent
            .child_by_field_name("condition")
            .is_some_and(|condition| condition.id() == node.id()));

    loop {
        // `Array` is tree-sitter-perl's name for the `(...)` shape
        // used BOTH as the if/while/unless/until condition wrapper
        // AND as list literals `(1, 2, 3)` (and `(x, y)` operand
        // groupings). In Perl's scalar context — which every walker
        // call site here operates in — a list expression evaluates
        // to its LAST element, so descending via the last named
        // child gives the semantically correct operand for both
        // shapes: `($a)` → `$a`, `($x, $y)` → `$y`, `if ($a)` →
        // `$a`. `ParenthesizedArgument` (the other paren-wrap kind)
        // has only one inner expression, so child(1) and last-named
        // are equivalent.
        let is_parens = matches!(node_kind, P::ParenthesizedArgument | P::Array);
        // Both spellings of the same negation — see `ruby_inspect_container`
        // for the rationale; Perl has the identical gap (#1182). Read
        // through the grammar's `operator` field, whose type list is
        // `! + ++ - -- and not ~`. Do NOT match the hidden `_unary_not`
        // supertype (`P::UnaryNot`), which the parser never emits
        // (grammar-dispatch item 2).
        let is_not = matches!(node_kind, P::UnaryExpression)
            && node
                .child_by_field_name("operator")
                .is_some_and(|op| matches!(op.kind_id().into(), P::BANG | P::Not));

        if !is_parens && !is_not {
            break;
        }
        if !has_boolean_content && is_not {
            has_boolean_content = true;
        }

        // Descend through the wrapper to the value. Array uses
        // last-named-child (Perl scalar-context value); other
        // wrappers store their inner expression at child(1).
        let next = if matches!(node_kind, P::Array) {
            perl_last_named_child(&node)
        } else {
            node.child(1)
        };
        let Some(child) = next else { break };
        node = child;
        node_kind = node.kind_id().into();

        if matches!(node_kind, perl_bool_terminal_kinds!()) {
            if has_boolean_content {
                *conditions += 1.;
            }
            break;
        }
    }
}

// Phase-2B (issue #403): pass `node.child(idx)` through
// `perl_inspect_container`. Perl wraps `if (cond)` / `while (cond)` /
// `unless (cond)` / `until (cond)` conditions in a
// `parenthesized_argument`, so the paren unwrap handles the
// boolean-literal case.
fn perl_inspect_child(node: &Node, idx: usize, conditions: &mut f64) {
    if let Some(child) = node.child(idx) {
        perl_inspect_container(&child, node, conditions);
    }
}

// Phase-2B helper (issue #403): Perl's `Array` node serves double
// duty as the `(...)` wrapper around `if` / `while` / `unless` /
// `until` conditions AND as the call-argument-list wrapper. The
// dispatcher routes call-argument Arrays through
// `perl_count_unary_conditions`; condition-slot Arrays are
// already unwrapped by `perl_inspect_container`. This predicate
// disambiguates by checking the parent kind.
// Returns the last named child of a node, or None if there are no
// named children. Used by `perl_inspect_container` to descend through
// the `Array` `(...)` wrapper: for a single-element grouping
// `($a)` the last named child is `$a`; for a multi-element list
// literal `($x, $y)` the last named child is `$y` (the value the
// expression evaluates to in Perl's scalar context, which is the
// only context the walker operates in).
fn perl_last_named_child<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    let mut cursor = node.cursor();
    let mut last_named = None;
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.is_named() {
                last_named = Some(child);
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    last_named
}

// Phase-2B (issues #403 / #1102): a ternary's condition and its two
// branch operands are each a Fitzpatrick Rule 9 unary condition, exactly
// as `java_walk_ternary` already counts them. Without this Perl scored
// `$a ? !$b : !$c` as 1 (the `ternary_expression` node alone) against
// Java's 4, and `perl_inspect_container`'s `TernaryExpression`
// boolean-context seed was unreachable.
//
// Slots are addressed by grammar FIELD, not by child index.
// tree-sitter-perl names the branches `true` / `false` rather than the
// C-family `consequence` / `alternative`, and all three slots are
// mandatory — Perl has no short-ternary elision.
//
// The condition goes through `perl_count_condition`, whose top-level
// terminal check is what stops a bare `$a ? … : …` scoring zero:
// `perl_inspect_container` alone only counts *after* unwrapping a
// `(...)` / `!...` layer. Branch operands get no such check: an
// unnegated branch is type-free and contributes nothing, which is what
// keeps `($a > 0) ? $b : -$b` at 2 (the ternary node and the `>`).
fn perl_walk_ternary(node: &Node, conditions: &mut f64) {
    if let Some(condition) = node.child_by_field_name("condition") {
        perl_count_condition(&condition, node, conditions);
    }
    for field in ["true", "false"] {
        if let Some(branch) = node.child_by_field_name(field) {
            perl_inspect_container(&branch, node, conditions);
        }
    }
}

// Classifies one condition-slot expression: a bare boolean terminal
// counts directly, anything else is offered to the `(...)` / `!...`
// unwrap chain. Shared by the ternary condition slot and the C-style
// `for` header's condition slot — the two places a Perl condition
// arrives *unwrapped*. `if` / `while` / `unless` / `until` hand
// `perl_inspect_container` the `(...)` wrapper that supplies the unwrap
// step itself, so they must not take the top-level terminal count.
// Mirrors `cpp_count_condition`.
fn perl_count_condition(condition: &Node, parent: &Node, conditions: &mut f64) {
    if matches!(condition.kind_id().into(), perl_bool_terminal_kinds!()) {
        *conditions += 1.;
    } else {
        perl_inspect_container(condition, parent, conditions);
    }
}

// Phase-2B (issues #403 / #1276): the C-style `for (init; condition;
// update)` header's condition slot is a Fitzpatrick Rule 9 unary
// condition, exactly like the `if` / `while` slots the dispatcher
// already walks. Without this Perl scored `for (my $i = 0; $ok; $i++)`
// zero where `if ($ok)` scores one. Comparison-shaped conditions
// (`$i < $n`) were never affected — the `<` token arm counts those.
//
// The slot is addressed by grammar FIELD (`condition`, beside
// `initializer` and `incrementor`), and an empty condition exposes no
// field, so `for (;;)` counts zero with no special case — see the
// `Stats` doc comment's cross-language empty-`for`-condition policy.
// tree-sitter-perl does not parse an empty *initializer* (`for (; $ok;
// )` becomes an `ERROR`-laden `for_statement_2`), so the three-clause
// spelling is the only one this reaches. `for_statement_2` is the
// `foreach` form and carries no condition.
fn perl_walk_for_statement(node: &Node, conditions: &mut f64) {
    if let Some(condition) = node.child_by_field_name("condition") {
        perl_count_condition(&condition, node, conditions);
    }
}

fn perl_is_call_argument_parent(parent: Node) -> bool {
    use Perl as P;
    matches!(
        parent.kind_id().into(),
        P::CallExpressionWithArgsWithBrackets
            | P::CallExpressionWithSpacedArgs
            | P::CallExpressionWithSub
            | P::CallExpressionWithVariable
            | P::CallExpressionRecursive
            | P::MethodInvocation
    )
}

fn perl_count_unary_conditions(list_node: &Node, conditions: &mut f64) {
    use Perl as P;

    let list_kind = list_node.kind_id().into();
    let mut cursor = list_node.cursor();

    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();
            let node_kind = node.kind_id().into();

            if matches!(node_kind, perl_bool_terminal_kinds!())
                && matches!(list_kind, P::BinaryExpression)
            {
                *conditions += 1.;
            } else if node.is_named() {
                perl_inspect_container(&node, list_node, conditions);
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

impl Abc for PerlCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        // bca: suppress(halstead)
        // Exhaustive one-arm-per-grammar-kind dispatch table; see the
        // rationale on `CppCode::compute`. Perl's arm list is the
        // longest of the family — tree-sitter-perl tokenises all
        // nineteen assignment operators and all six call-expression
        // wrappers separately — so `halstead.effort` here is a count of
        // distinct enum operands, not of reasoning a reader must do.
        use Perl as P;

        match node.kind_id().into() {
            // Plain `=` and every compound assignment operator. The
            // grammar tokenises each operator separately, so one
            // textual `+=` produces exactly one token and there is no
            // double-counting via a wrapper.
            P::EQ
            | P::PLUSEQ
            | P::DASHEQ
            | P::STAREQ
            | P::SLASHEQ
            | P::PERCENTEQ
            | P::STARSTAREQ
            | P::DOTEQ
            | P::XEQ
            | P::AMPEQ
            | P::PIPEEQ
            | P::CARETEQ
            | P::LTLTEQ
            | P::GTGTEQ
            | P::AMPAMPEQ
            | P::PIPEPIPEEQ
            | P::SLASHSLASHEQ
            | P::AMPDOTEQ
            | P::PIPEDOTEQ
            | P::CARETDOTEQ => {
                stats.assignments += 1.;
            }
            // Argument-bearing call wrappers always count.
            P::CallExpressionWithSpacedArgs
            | P::CallExpressionWithSub
            | P::CallExpressionWithArgsWithBrackets
            | P::CallExpressionWithVariable
            | P::CallExpressionRecursive
            | P::MethodInvocation => {
                stats.branches += 1.;
            }
            // Bareword-only call (`shift`, `time`, …) — count only
            // when this node is the outermost dispatch site. When the
            // bareword sits inside one of the wrappers above, the
            // outer node has already been counted and this child
            // would double the branch tally.
            P::CallExpressionWithBareword
                if !ancestors.parent(node).is_some_and(|p| {
                    matches!(
                        p.kind_id().into(),
                        P::CallExpressionWithSpacedArgs
                            | P::CallExpressionWithSub
                            | P::CallExpressionWithArgsWithBrackets
                            | P::CallExpressionWithVariable
                            | P::CallExpressionRecursive
                    )
                }) =>
            {
                stats.branches += 1.;
            }
            // Numeric, string, and pattern-match comparison operators
            // plus the spaceship / `cmp` three-way comparisons, and each
            // `elsif` / `else` clause of an `if` / `unless` chain.
            P::EQEQ
            | P::BANGEQ
            | P::LT
            | P::GT
            | P::LTEQ
            | P::GTEQ
            | P::LTEQGT
            | P::Eq
            | P::Ne
            | P::Lt
            | P::Gt
            | P::Le
            | P::Ge
            | P::Cmp
            | P::EQTILDE
            | P::BANGTILDE
            | P::ElsifClause
            | P::ElseClause => {
                stats.conditions += 1.;
            }
            // Fitzpatrick Rule 9 walker: each operand of a Perl
            // short-circuit / low-precedence logical chain is one
            // condition (issue #403). Covers `&&`, `||`, `//`,
            // `and`, `or`, `xor`.
            P::AMPAMP | P::PIPEPIPE | P::SLASHSLASH | P::And | P::Or | P::Xor => {
                if let Some(parent) = ancestors.parent(node) {
                    perl_count_unary_conditions(&parent, &mut stats.conditions);
                }
            }
            // Phase-2B (issue #403): condition slots. Perl wraps
            // `if (cond)` / `while (cond)` / `unless (cond)` /
            // `until (cond)` in the `Array` `(...)` shape (the
            // grammar's name for parenthesized expressions in
            // statement-modifier slots) — the paren unwrap handles
            // boolean-literal cases. Condition sits at child(1)
            // (child(0) is the `if` / `while` keyword).
            // `return value`'s value also sits at child(1); merged
            // into the same arm body to satisfy `match_same_arms`.
            P::IfStatement
            | P::UnlessStatement
            | P::WhileStatement
            | P::UntilStatement
            | P::ReturnExpression => {
                perl_inspect_child(node, 1, &mut stats.conditions);
            }
            // `call(!$a, !$b)` — argument list walker. Perl wraps
            // call-argument lists in an `Array` node (same kind name
            // as the `(...)` wrapper around `if` / `while`
            // conditions). To avoid re-handling condition slots that
            // were already walked through inspect_container, only
            // dispatch when the parent is a call-expression form.
            P::Array
                if ancestors
                    .parent(node)
                    .is_some_and(perl_is_call_argument_parent) =>
            {
                perl_count_unary_conditions(node, &mut stats.conditions);
            }
            // `$a ? !$b : !$c`. Unlike the C family, this dispatcher
            // has no `?`-token arm — the grammar emits the token, but
            // the `ternary_expression` node is what carries the
            // condition tally's +1 — so this arm keeps that increment
            // and adds the three operand slots (issue #1102).
            P::TernaryExpression => {
                stats.conditions += 1.;
                perl_walk_ternary(node, &mut stats.conditions);
            }
            // `for (init; cond; update)` — the condition slot, read by
            // grammar field (issue #1276). `for (;;)` has no condition
            // field and counts nothing.
            P::ForStatement1 => {
                perl_walk_for_statement(node, &mut stats.conditions);
            }
            _ => {}
        }
    }
}
