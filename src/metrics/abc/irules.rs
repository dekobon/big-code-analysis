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
use crate::macros::irules_bool_terminal_kinds;
use crate::*;

/// The three operand slots of a `ternary_expr`, located relative to the
/// `?` and `:` tokens rather than by fixed index.
///
/// The grammar exposes **no fields** on `ternary_expr` (verified against
/// `node-types.json`: `fields: []`), so the `child_by_field_name` model
/// every other language's ABC ternary uses does not transfer here. Nor
/// does a fixed index: `_expr` inlines `seq('(', $._expr, ')')`, so
/// `($a) ? $b : $c` puts the anonymous parens directly under
/// `ternary_expr` and shifts every operand right (#1180,
/// grammar-dispatch item 3).
///
/// Slots are therefore the *named* children adjacent to the two marker
/// tokens, which is stable under both parenthesisation and comments.
fn irules_ternary_slots<'a>(
    node: &Node<'a>,
) -> (Option<Node<'a>>, Option<Node<'a>>, Option<Node<'a>>) {
    let (mut condition, mut consequence, mut alternative) = (None, None, None);
    let mut seen_question = false;
    let mut seen_colon = false;

    for child in node.children() {
        match child.kind_id().into() {
            Irules::QMARK => seen_question = true,
            Irules::COLON => seen_colon = true,
            _ if !child.is_named() => {}
            _ if seen_colon => alternative = alternative.or(Some(child)),
            _ if seen_question => consequence = consequence.or(Some(child)),
            // The condition is the *last* named child before `?`, so a
            // parenthesised condition resolves to the inner expression
            // rather than to whatever preceded it.
            _ => condition = Some(child),
        }
    }
    (condition, consequence, alternative)
}

/// Routes the three ternary slots (#1180).
///
/// Mirrors `cpp_walk_ternary`: the condition is counted directly when it
/// is already a terminal bool, and otherwise handed to the wrapper-peeling
/// walker; each branch goes to the walker, which counts it only if a `!`
/// establishes boolean content for that slot.
fn irules_walk_ternary(node: &Node, conditions: &mut f64) {
    let (condition, consequence, alternative) = irules_ternary_slots(node);
    if let Some(condition) = condition {
        if matches!(condition.kind_id().into(), irules_bool_terminal_kinds!()) {
            *conditions += 1.;
        } else {
            irules_inspect_container(&condition, node, conditions);
        }
    }
    for branch in [consequence, alternative].into_iter().flatten() {
        irules_inspect_container(&branch, node, conditions);
    }
}

/// The `expr` wrapper holding an `if` / `elseif` / `while` predicate.
///
/// `while` exposes no `condition` field at all — the grammar is
/// `seq('while', $.expr, $._word)` — so the slot is found by kind. Match
/// `Irules::Expr` (the braced `{ … }` expression node) and not
/// `Irules::Expr2`, which is the `expr` *command keyword* under
/// `expr_cmd` and renders to the same name (grammar-dispatch item 1).
/// `Irules::Expr3` is the hidden `_expr` supertype the parser never emits
/// (item 2).
fn irules_condition_expr<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    node.children()
        .find(|child| matches!(child.kind_id().into(), Irules::Expr))
}

impl Abc for IrulesCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        match node.kind_id().into() {
            // The `set name value` production is a first-class node.
            Irules::Set => {
                stats.assignments += 1.;
            }
            // Generic command: assignment when the first word names a known
            // mutator (`incr`/`append`/`lappend`), otherwise a branch — every
            // dispatch counts, including `return`. The `if`/`while`/`switch`/…
            // productions are separate kinds and do not reach this arm.
            Irules::Command => {
                if irules_command_is_assignment(node, code) {
                    stats.assignments += 1.;
                } else {
                    stats.branches += 1.;
                }
            }
            // Numeric and string comparison tokens, the ternary expression,
            // and each `elseif` / `else` clause. iRules adds the word-form
            // string comparators (`starts_with`, `contains`, `matches`, …)
            // that Tcl lacks.
            Irules::EQEQ
            | Irules::BANGEQ
            | Irules::LT
            | Irules::GT
            | Irules::LTEQ
            | Irules::GTEQ
            | Irules::Eq
            | Irules::Ne
            | Irules::StartsWith
            | Irules::EndsWith
            | Irules::Contains
            | Irules::Equals
            | Irules::Matches
            | Irules::MatchesRegex
            | Irules::MatchesGlob
            | Irules::In
            | Irules::Ni
            | Irules::Else => {
                stats.conditions += 1.;
            }
            // Phase 2B slot routing (#1180) — see `tcl.rs`, which this
            // mirrors arm for arm.
            Irules::If | Irules::While => {
                if let Some(expr) = irules_condition_expr(node) {
                    irules_inspect_container(&expr, node, &mut stats.conditions);
                }
            }
            Irules::Elseif => {
                stats.conditions += 1.;
                if let Some(expr) = irules_condition_expr(node) {
                    irules_inspect_container(&expr, node, &mut stats.conditions);
                }
            }
            Irules::TernaryExpr => {
                stats.conditions += 1.;
                irules_walk_ternary(node, &mut stats.conditions);
            }
            // Fitzpatrick Rule 9: the short-circuit operators are not counted
            // directly (cross-language policy, #395); instead each operand of
            // a `&&`/`||`/`and`/`or` chain is one condition (#403). iRules'
            // keyword forms (`and`/`or`) get the same treatment as `&&`/`||`.
            Irules::AMPAMP | Irules::PIPEPIPE | Irules::And | Irules::Or => {
                if let Some(parent) = ancestors.parent(node) {
                    irules_count_unary_conditions(&parent, &mut stats.conditions);
                }
            }
            _ => {}
        }
    }
}

// iRules mutator commands (same Tcl builtins; the dedicated `set`
// production is handled separately in the impl, like Tcl).
const IRULES_ASSIGNMENT_COMMANDS: &[&[u8]] = &[b"incr", b"append", b"lappend"];

// iRules counterpart of `tcl_command_is_assignment`.
fn irules_command_is_assignment(node: &Node, code: &[u8]) -> bool {
    let Some(first) = node.child(0) else {
        return false;
    };
    let start = first.start_byte();
    let end = first.end_byte();
    if end > code.len() || start >= end {
        return false;
    }
    let word = &code[start..end];
    IRULES_ASSIGNMENT_COMMANDS.contains(&word)
}

// iRules counterpart of `tcl_inspect_container` (Fitzpatrick Rule 9): a
// negated bare operand (`!$flag`) inside a boolean chain is one condition.
fn irules_inspect_container(container_node: &Node, parent: &Node, conditions: &mut f64) {
    // bca: suppress(cognitive) — wrapper-peeling state machine, clearest whole
    // The same shape as `cpp_inspect_container`, and it carries the same
    // marker for the same reason: one loop peels the `expr` / `!` layers
    // while carrying a single boolean-context flag, the flag must be
    // readable at every step so any split would have to thread it back
    // out, and the parts have no names a reader would draw. It crossed
    // the limit when #1180 wired the Phase 2B slot routing — the seed
    // grew a ternary-slot disjunct and the loop gained the wrapper peel
    // every sibling already had.
    let mut node = *container_node;
    let mut node_kind = node.kind_id().into();
    let parent_kind = parent.kind_id().into();
    // Phase 2B slot routing (#1180). Before it, only a `&&` / `||` chain
    // seeded boolean context, so `if {!$a}` and every ternary operand
    // scored zero. The `if` / `elseif` / `while` predicate is a boolean
    // context by construction; a ternary seeds only its *condition*
    // slot, since the two branches are type-free and an unnegated branch
    // must contribute nothing.
    let mut has_boolean_content = matches!(
        parent_kind,
        Irules::BinopExpr | Irules::If | Irules::Elseif | Irules::While
    ) || (matches!(parent_kind, Irules::TernaryExpr)
        && irules_ternary_slots(parent)
            .0
            .is_some_and(|condition| condition.id() == node.id()));

    loop {
        // The `expr` wrapper is this grammar's `{ … }` predicate node —
        // the analogue of the C family's `condition_clause`, and peeled
        // the same way.
        let is_parens = matches!(node_kind, Irules::Expr);
        let is_not = matches!(node_kind, Irules::UnaryExpr)
            && node
                .child(0)
                .is_some_and(|c| c.kind_id() == Irules::BANG as u16);

        if !is_parens && !is_not {
            break;
        }
        // A `!` proves the operand is boolean even where the parent did
        // not — every sibling language carries this line; its absence was
        // why a negated operand outside a `&&` chain could never count.
        if !has_boolean_content && is_not {
            has_boolean_content = true;
        }

        // `!` stores its operand at child 1 (after the token). The `expr`
        // wrapper holds `{`, the expression, `}`, and `_expr` inlines
        // parens, so take the first *named* child instead of a fixed
        // index — that resolves `{($a)}` as well as `{$a}`.
        let next = if is_not {
            node.child(1)
        } else {
            node.children().find(Node::is_named)
        };
        let Some(child) = next else { break };
        node = child;
        node_kind = node.kind_id().into();

        if matches!(node_kind, irules_bool_terminal_kinds!()) {
            if has_boolean_content {
                *conditions += 1.;
            }
            break;
        }
    }
}

// iRules counterpart of `tcl_count_unary_conditions`.
fn irules_count_unary_conditions(list_node: &Node, conditions: &mut f64) {
    let list_kind = list_node.kind_id().into();
    let mut cursor = list_node.cursor();

    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();
            let node_kind = node.kind_id().into();

            if matches!(node_kind, irules_bool_terminal_kinds!())
                && matches!(list_kind, Irules::BinopExpr)
            {
                *conditions += 1.;
            } else if node.is_named() {
                irules_inspect_container(&node, list_node, conditions);
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}
