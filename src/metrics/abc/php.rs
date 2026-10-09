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
    Abc, Stats, count_boolean_slot, count_field_operands, count_negated_operand, is_operand,
    wrapped_operand,
};
use crate::*;

// One step of the value peel (see `PeelStep`). A parenthesis, a cast
// (its `value`), an `@` error suppression and an assignment (its
// `right`, so `if ($y = $x > 1)` tests the comparison its own arm
// already counts) each evaluate to their operand. A compound assignment
// is not peeled: the slot tests the updated value. PHP's grammar spells
// prefix operators `unary_op_expression`; `!` is the one that proves its
// operand boolean, and `-$x` / `~$x` / `+$x` yield a value and stop the
// peel. Every operand is read by role rather than at child(1), where a
// comment may sit (`(/*c*/ $b)`, `! /*c*/ $b` — #1455).
fn php_wrapper_operand<'a>(node: &Node<'a>) -> Option<(Node<'a>, bool)> {
    use Php::*;

    match node.kind_id().into() {
        ParenthesizedExpression | ErrorSuppressionExpression => {
            wrapped_operand(node).map(|o| (o, false))
        }
        CastExpression | CastExpression2 => node.child_by_field_name("value").map(|o| (o, false)),
        AssignmentExpression | ReferenceAssignmentExpression => {
            node.child_by_field_name("right").map(|o| (o, false))
        }
        UnaryOpExpression | UnaryOpExpression2
            if node
                .child_by_field_name("operator")
                .is_some_and(|op| op.kind_id() == BANG) =>
        {
            node.child_by_field_name("argument").map(|o| (o, true))
        }
        _ => None,
    }
}

// Whether an arm of `compute` already charges `expr` (already peeled) as
// a condition: a ternary (its own arm), a comparison or `instanceof`
// (the token arm), or an `&&` / `||` / `and` / `or` chain, whose
// operands each pay through `php_count_condition`. `??` is no condition
// token in PHP, so a slot holding one pays for it.
fn php_condition_scores_itself(expr: &Node) -> bool {
    use Php::*;

    match expr.kind_id().into() {
        ConditionalExpression => true,
        BinaryExpression => expr.child_by_field_name("operator").is_some_and(|op| {
            matches!(
                op.kind_id().into(),
                EQEQ | EQEQEQ
                    | BANGEQ
                    | BANGEQEQ
                    | LT
                    | GT
                    | LTEQ
                    | GTEQ
                    | LTEQGT
                    | LTGT
                    | Instanceof
                    | AMPAMP
                    | PIPEPIPE
                    | And
                    | Or
            )
        }),
        _ => false,
    }
}

// Scores one boolean slot — an `if` / `elseif` / `while` / `do` / `for`
// condition, a ternary condition, an operand of an `&&` / `||` / `and` /
// `or` chain (see `count_boolean_slot`). PHP is truthy-valued,
// so `if (-$x)`, `if ($x + 1)`, `if ($y = $x)`, `if (A::B)` and
// `if ($a ?? $b)` are each a decision, and each scored 0 while the slot
// paid only for a fixed list of terminal kinds (#1526).
fn php_count_condition(condition: &Node, conditions: &mut f64) {
    count_boolean_slot(
        condition,
        php_wrapper_operand,
        php_condition_scores_itself,
        conditions,
    );
}

// An operand outside a boolean slot — a `return` value, a call
// argument, a ternary branch — scores only when a `!` proves it boolean
// (see `count_negated_operand`), which keeps `($a > 0) ? $b : -$b` at 2
// (the ternary node and the `>`).
fn php_count_negated(operand: &Node, conditions: &mut f64) {
    count_negated_operand(
        operand,
        php_wrapper_operand,
        php_condition_scores_itself,
        conditions,
    );
}

fn php_count_slot(slot: Option<Node>, conditions: &mut f64) {
    if let Some(condition) = slot {
        php_count_condition(&condition, conditions);
    }
}

// Phase-2B (issues #403 / #1102): a ternary's condition is a boolean
// slot and each branch a negated operand, exactly as `java_walk_ternary`
// counts them. Without this PHP scored `$a ? !$b : !$c` as 1 (the
// `conditional_expression` node alone) against Java's 4.
//
// Slots are addressed by grammar FIELD, not by child index. PHP names
// the consequence `body` (not `consequence`) and marks it OPTIONAL to
// admit the short ternary `$a ?: $b`, which shifts the alternative from
// child(4) to child(3).
fn php_walk_ternary(node: &Node, conditions: &mut f64) {
    php_count_slot(node.child_by_field_name("condition"), conditions);
    for field in ["body", "alternative"] {
        if let Some(branch) = node.child_by_field_name(field) {
            php_count_negated(&branch, conditions);
        }
    }
}

// Returns the value slot of a PHP `argument` wrapper node.
// Positional argument `m(!$a)` has a single named child — the value.
// Named argument `m(name: !$a)` has children `name`, `:`, value — the
// last named child is the value. Returns the last named child for
// both shapes; returns None only when the argument has no named
// children (grammar-error case).
fn php_argument_value<'a>(argument: &Node<'a>) -> Option<Node<'a>> {
    argument.children().filter(Node::is_named).last()
}

// `f(!$a, !$b)`: each argument's value is a negated operand. PHP wraps
// each call argument in an `argument` node; for a named argument
// `m(name: !$a)` the value is its LAST named child, for a positional one
// its only child.
fn php_count_arguments(arguments: &Node, conditions: &mut f64) {
    for argument in arguments.children() {
        let value = if argument.kind_id() == Php::Argument {
            php_argument_value(&argument)
        } else {
            Some(argument).filter(is_operand)
        };
        if let Some(value) = value {
            php_count_negated(&value, conditions);
        }
    }
}

impl Abc for PhpCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Php::*;

        match node.kind_id().into() {
            // Assignments: explicit assignment expressions and augmented forms,
            // plus pre/post increment and decrement. `const_declaration` and
            // `enum_case` use their own `const_element` / value-assignment
            // shapes, so they do not produce `AssignmentExpression` nodes —
            // matching the assignment-expression kinds naturally excludes
            // them.
            AssignmentExpression
            | AugmentedAssignmentExpression
            | ReferenceAssignmentExpression
            | PLUSPLUS
            | DASHDASH => {
                stats.assignments += 1.;
            }
            // Branches: every PHP call kind plus object construction.
            FunctionCallExpression
            | MemberCallExpression
            | ScopedCallExpression
            | NullsafeMemberCallExpression
            | ObjectCreationExpression => {
                stats.branches += 1.;
            }
            // Conditions: comparison and identity operators (anonymous tokens
            // inside `binary_expression`), `instanceof`, and control-flow
            // arms. The ternary has its own arm below (#1102).
            //
            // `CaseStatement` (`case` arms) and `MatchConditionalExpression`
            // (non-default `match` arms) are conditions; the `default:`
            // (`DefaultStatement`) and `default =>`
            // (`MatchDefaultExpression`) arms are NOT — they are the
            // unconditional fallthrough, which PHP's cyclomatic gate also
            // excludes (it counts `CaseStatement | MatchConditionalExpression`
            // only). Dropping both Default kinds keeps ABC conditions equal
            // to the cyclomatic decision count (issue #473, mirroring the
            // #469 C-family fix and #456 Kotlin/C# fixes).
            EQEQ
            | EQEQEQ
            | BANGEQ
            | BANGEQEQ
            | LT
            | GT
            | LTEQ
            | GTEQ
            | LTEQGT
            | LTGT
            | Instanceof
            | ElseClause
            | ElseClause2
            | CaseStatement
            | MatchConditionalExpression
            | CatchClause => {
                stats.conditions += 1.;
            }
            // Fitzpatrick Rule 9: each operand of a `&&` / `||` / `and`
            // / `or` chain is one condition (issue #403). `xor` is no
            // chain: it evaluates both operands, so like `^` it is a
            // value and its slot pays once (#1536). PHP exposes both
            // the punctuation forms (`&&`, `||`) and the
            // low-precedence keyword forms (`and`, `or`) as
            // distinct tokens inside `binary_expression`; both fire
            // the walker so `connect() or die();`-style idiom counts
            // the same as `connect() || die();`. `$a && $b || $c` is a
            // left-nested chain of `binary_expression`s, so an operand
            // that is itself a chain is paid by its own operator's visit.
            AMPAMP | PIPEPIPE | And | Or => {
                if let Some(chain) = ancestors
                    .parent(node)
                    .filter(|p| p.kind_id() == BinaryExpression)
                {
                    count_field_operands(&chain, php_count_condition, &mut stats.conditions);
                }
            }
            // An `elseif` is Java's `else if` — and PHP's own two-word
            // `else if`: the `else` (+1, Rule 5) and an `if` predicate
            // slot, as Ruby's `elsif` is. It paid only the first, so
            // `elseif ($b)` scored one below `else if ($b)` (#1526).
            ElseIfClause | ElseIfClause2 => {
                stats.conditions += 1.;
                php_count_slot(node.child_by_field_name("condition"), &mut stats.conditions);
            }
            // Phase-2B (issue #403): condition slots. PHP wraps
            // `if (...)` / `while (...)` / `do {…} while (...)` in a
            // `parenthesized_expression`, read by grammar field: a
            // comment before it (`if /*c*/ ($b)`) shifted the fixed
            // index it replaced (#1455). The `for` header's condition
            // is read by the same field (#1276); an empty `for (;;)`
            // exposes none and counts nothing — see the `Stats` doc
            // comment's cross-language empty-`for`-condition policy.
            // `return value;` names no field; its value is its only
            // operand.
            IfStatement | WhileStatement | DoStatement | ForStatement => {
                php_count_slot(node.child_by_field_name("condition"), &mut stats.conditions);
            }
            ReturnStatement => {
                if let Some(value) = wrapped_operand(node) {
                    php_count_negated(&value, &mut stats.conditions);
                }
            }
            // `f(!$a, !$b)` — argument list walker.
            Arguments => php_count_arguments(node, &mut stats.conditions),
            // `$a ? !$b : !$c`. Unlike the C family, this dispatcher
            // has no `?`-token arm — the grammar emits the token, but
            // the `conditional_expression` node is what carries the
            // condition tally's +1 — so this arm keeps that increment
            // and adds the three operand slots (issue #1102).
            ConditionalExpression => {
                stats.conditions += 1.;
                php_walk_ternary(node, &mut stats.conditions);
            }
            _ => {}
        }
    }
}
