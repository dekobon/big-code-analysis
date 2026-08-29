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
use crate::macros::php_bool_terminal_kinds;
use crate::*;

// PHP ABC unary-conditional walker (Fitzpatrick Rule 9; issue #403).
// PHP's grammar uses `unary_op_expression` (not `unary_expression`) for
// `!` and `~` prefix operators. Terminal-bool kinds: `Name` (function/
// constant identifier — the bare-identifier kind in tree-sitter-php),
// `VariableName` (`$x`), `Boolean` (the named `true` / `false` wrapper),
// and every call / member-access / subscript form. `ParenthesizedExpression`
// wraps `if (...)`-style condition slots.
fn php_inspect_container(container_node: &Node, parent: &Node, conditions: &mut f64) {
    // bca: suppress(cognitive) — wrapper-peeling state machine, clearest whole
    // See `cpp_inspect_container` for the shared rationale: one loop peels
    // `(...)` / `!...` layers while carrying a single boolean-context flag.
    use Php::*;

    let mut node = *container_node;
    let mut node_kind = node.kind_id().into();
    let parent_kind = parent.kind_id().into();
    let mut has_boolean_content = matches!(
        parent_kind,
        BinaryExpression | IfStatement | WhileStatement | DoStatement | ForStatement
    ) || (matches!(parent_kind, ConditionalExpression)
        && parent
            .child_by_field_name("condition")
            .is_some_and(|condition| condition.id() == node.id()));

    loop {
        let is_parens = matches!(node_kind, ParenthesizedExpression);
        let is_not = matches!(node_kind, UnaryOpExpression)
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

        if matches!(node_kind, php_bool_terminal_kinds!()) {
            if has_boolean_content {
                *conditions += 1.;
            }
            break;
        }
    }
}

// Phase-2B helper (issue #403): pass `node.child(idx)` through
// `php_inspect_container`. PHP wraps `if (...)` / `while (...)` /
// `do {…} while (...)` in `parenthesized_expression`, so the paren
// unwrap handles the boolean-literal case (`if (true)` counts 1).
fn php_inspect_child(node: &Node, idx: usize, conditions: &mut f64) {
    if let Some(child) = node.child(idx) {
        php_inspect_container(&child, node, conditions);
    }
}

// Phase-2B (issues #403 / #1102): a ternary's condition and its two
// branch operands are each a Fitzpatrick Rule 9 unary condition, exactly
// as `java_walk_ternary` already counts them. Without this PHP scored
// `$a ? !$b : !$c` as 1 (the `conditional_expression` node alone)
// against Java's 4, and `php_inspect_container`'s `ConditionalExpression`
// boolean-context seed was unreachable.
//
// Slots are addressed by grammar FIELD, not by child index. PHP names
// the consequence `body` (not `consequence`) and marks it OPTIONAL to
// admit the short ternary `$a ?: $b`, which shifts the alternative from
// child(4) to child(3).
//
// The condition goes through `php_count_condition`, whose top-level
// terminal check is what stops a bare `$a ? … : …` scoring zero.
// Branch operands get no such check: an unnegated branch is type-free
// and contributes nothing, which is what keeps `($a > 0) ? $b : -$b`
// at 2 (the ternary node and the `>`).
fn php_walk_ternary(node: &Node, conditions: &mut f64) {
    if let Some(condition) = node.child_by_field_name("condition") {
        php_count_condition(&condition, node, conditions);
    }
    for field in ["body", "alternative"] {
        if let Some(branch) = node.child_by_field_name(field) {
            php_inspect_container(&branch, node, conditions);
        }
    }
}

// Classifies one condition-slot expression: a bare boolean terminal
// counts directly, anything else is offered to the `(...)` / `!...`
// unwrap chain. Shared by the ternary condition slot and the `for`
// header's condition slot — the two places a PHP condition arrives
// *unwrapped*. `if` / `while` / `do` hand `php_inspect_container` a
// `parenthesized_expression` that supplies the unwrap step itself, so
// they must not take the top-level terminal count.
fn php_count_condition(condition: &Node, parent: &Node, conditions: &mut f64) {
    if matches!(condition.kind_id().into(), php_bool_terminal_kinds!()) {
        *conditions += 1.;
    } else {
        php_inspect_container(condition, parent, conditions);
    }
}

// Phase-2B (issues #403 / #1276): the `for (init; condition; update)`
// condition slot is a Fitzpatrick Rule 9 unary condition, exactly like
// the `if` / `while` slots the dispatcher already walks. Without this
// PHP scored `for (; $a; ) {}` zero where `if ($a) {}` scores one, and
// `php_inspect_container`'s `ForStatement` boolean-context seed was
// unreachable. Comparison-shaped conditions (`$i < $n`) were never
// affected — the `<` token arm counts those.
//
// Addressed by grammar FIELD (`condition`, alongside `initialize` and
// `update`): all three slots are optional, so every child index moves
// with the shape written.
//
// An empty condition (`for (;;)`) exposes no `condition` field, so it
// counts zero with no special case — see the `Stats` doc comment's
// cross-language empty-`for`-condition policy.
fn php_walk_for_statement(node: &Node, conditions: &mut f64) {
    if let Some(condition) = node.child_by_field_name("condition") {
        php_count_condition(&condition, node, conditions);
    }
}

// Returns the value slot of a PHP `argument` wrapper node.
// Positional argument `m(!$a)` has a single named child — the value.
// Named argument `m(name: !$a)` has children `name`, `:`, value — the
// last named child is the value. Returns the last named child for
// both shapes; returns None only when the argument has no named
// children (grammar-error case).
fn php_argument_value<'a>(argument: &Node<'a>) -> Option<Node<'a>> {
    let mut cursor = argument.cursor();
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

fn php_count_unary_conditions(list_node: &Node, conditions: &mut f64) {
    use Php::*;

    let list_kind = list_node.kind_id().into();

    // `children()` rather than a hand-driven cursor: the manual
    // `goto_first_child` / `goto_next_sibling` bookkeeping put the
    // skip-this-child case three levels deep (a `continue` inside a
    // `let`-`else` inside the loop) for no behavioural difference —
    // `children()` walks the same cursor over the same children.
    for node in list_node.children() {
        let node_kind = node.kind_id().into();

        // PHP wraps each call argument in an `argument` node; descend
        // through that wrapper to the value slot. For named arguments
        // `m(name: !$a)` the value is the LAST named child
        // (`name`/`:`/`value`); for positional arguments `m(!$a)` the
        // value is the only child. Use the last named child to handle
        // both shapes — and skip the rare grammar-error case where
        // Argument has no named children.
        // `inner_parent` is carried alongside `inner` because
        // `php_inspect_container` seeds its boolean-context flag from
        // the parent kind and must not rediscover it (#1096):
        // unwrapping an `argument` makes the wrapper the parent,
        // otherwise it is the list.
        let (inner, inner_parent) = if matches!(node_kind, Argument) {
            match php_argument_value(&node) {
                Some(value) => (value, node),
                None => continue,
            }
        } else {
            (node, *list_node)
        };
        let inner_kind = inner.kind_id().into();

        if matches!(inner_kind, php_bool_terminal_kinds!()) && matches!(list_kind, BinaryExpression)
        {
            *conditions += 1.;
        } else if inner.is_named() {
            php_inspect_container(&inner, &inner_parent, conditions);
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
            | ElseIfClause
            | ElseIfClause2
            | CaseStatement
            | MatchConditionalExpression
            | CatchClause => {
                stats.conditions += 1.;
            }
            // Fitzpatrick Rule 9: each operand of a `&&` / `||` / `and`
            // / `or` / `xor` chain is one condition (issue #403). PHP
            // exposes both the punctuation forms (`&&`, `||`) and the
            // low-precedence keyword forms (`and`, `or`, `xor`) as
            // distinct tokens inside `binary_expression`; both fire
            // the walker so `connect() or die();`-style idiom counts
            // the same as `connect() || die();`.
            AMPAMP | PIPEPIPE | And | Or | Xor => {
                if let Some(parent) = ancestors.parent(node) {
                    php_count_unary_conditions(&parent, &mut stats.conditions);
                }
            }
            // Phase-2B (issue #403): condition slots. PHP wraps
            // `if (...)` / `while (...)` in `parenthesized_expression`
            // at child(1); `return value;` exposes the value at the
            // same index. `do {…} while (...)` has the parenthesized
            // condition at child(3).
            IfStatement | WhileStatement | ReturnStatement => {
                php_inspect_child(node, 1, &mut stats.conditions);
            }
            DoStatement => {
                php_inspect_child(node, 3, &mut stats.conditions);
            }
            // `f(!$a, !$b)` — argument list walker.
            Arguments => {
                php_count_unary_conditions(node, &mut stats.conditions);
            }
            // `$a ? !$b : !$c`. Unlike the C family, this dispatcher
            // has no `?`-token arm — the grammar emits the token, but
            // the `conditional_expression` node is what carries the
            // condition tally's +1 — so this arm keeps that increment
            // and adds the three operand slots (issue #1102).
            ConditionalExpression => {
                stats.conditions += 1.;
                php_walk_ternary(node, &mut stats.conditions);
            }
            // `for (init; cond; update)` — the condition slot, read by
            // grammar field (issue #1276). `for (;;)` has no condition
            // field and counts nothing.
            ForStatement => {
                php_walk_for_statement(node, &mut stats.conditions);
            }
            _ => {}
        }
    }
}
