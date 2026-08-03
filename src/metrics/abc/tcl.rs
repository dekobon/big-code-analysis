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
use crate::macros::tcl_bool_terminal_kinds;
use crate::*;

// Names of Tcl commands that mutate a variable. Each invocation of
// one of these commands counts as an assignment, not a branch — the
// command is acting as an assignment operator, not as a generic
// dispatch. The list is intentionally narrow: only commands that
// every Tcl programmer recognises as primary mutators. Less-common
// mutators (`dict set`, `array set`, `lset`, `regsub … name`) are
// left as branches; treating them as assignments would require
// inspecting the command's second word, and the additional
// fidelity is not worth the complexity for the ABC magnitude.
const TCL_ASSIGNMENT_COMMANDS: &[&[u8]] = &[b"incr", b"append", b"lappend"];

// Fitzpatrick's ABC rules adapted for Tcl.
//
// - Assignments: every `set` production (`set name value`) plus
//   every `command` whose first word is one of the recognised
//   mutator commands in `TCL_ASSIGNMENT_COMMANDS`. Tcl has no
//   assignment operators — variable mutation is always a command
//   invocation, so we filter on the command name. The `set` form
//   has its own grammar production (`Tcl::Set`) and counts directly
//   without any source-text inspection.
// - Branches: every other `command` node. Like Bash, `return` and
//   `error` builtins parse as plain `command` nodes and count here
//   too — Tcl treats every dispatch the same regardless of whether
//   the command is a procedure call, a control-flow primitive, or a
//   builtin. The grammar productions for `if`, `while`, `foreach`,
//   etc. live separately from `command` and do not double-count.
// - Conditions: numeric (`==`, `!=`, `<`, `>`, `<=`, `>=`) and
//   string (`eq`, `ne`, `in`, `ni`) comparison tokens, the ternary
//   expression production, and each `elseif` / `else` clause of an
//   `if`. The short-circuit operators `&&` / `||` are deliberately
//   NOT counted; see the module-level `Stats` doc-comment for the
//   cross-language policy (issue #395, walker tracked in #403).
// Tcl ABC unary-conditional walker (Fitzpatrick Rule 9; issue #403).
// Tcl expression syntax appears inside `if {…}` / `while {…}` braces
// and parses as `binop_expr` whose operator tokens include `AMPAMP`
// and `PIPEPIPE`. Terminal-bool kinds are the bare-word literals
// `simple_word`, the braced / quoted variants, variable substitutions
// (`$x`), command substitutions (`[cmd]`), the boolean keyword, and
// the numeric literal.
fn tcl_inspect_container(container_node: &Node, parent: &Node, conditions: &mut f64) {
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
        Tcl::BinopExpr | Tcl::If | Tcl::Elseif | Tcl::While
    ) || (matches!(parent_kind, Tcl::TernaryExpr)
        && tcl_ternary_slots(parent)
            .0
            .is_some_and(|condition| condition.id() == node.id()));

    loop {
        // The `expr` wrapper is this grammar's `{ … }` predicate node —
        // the analogue of the C family's `condition_clause`, and peeled
        // the same way.
        let is_parens = matches!(node_kind, Tcl::Expr);
        let is_not = matches!(node_kind, Tcl::UnaryExpr)
            && node
                .child(0)
                .is_some_and(|c| c.kind_id() == Tcl::BANG as u16);

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

        if matches!(node_kind, tcl_bool_terminal_kinds!()) {
            if has_boolean_content {
                *conditions += 1.;
            }
            break;
        }
    }
}

fn tcl_count_unary_conditions(list_node: &Node, conditions: &mut f64) {
    let list_kind = list_node.kind_id().into();
    let mut cursor = list_node.cursor();

    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();
            let node_kind = node.kind_id().into();

            if matches!(node_kind, tcl_bool_terminal_kinds!())
                && matches!(list_kind, Tcl::BinopExpr)
            {
                *conditions += 1.;
            } else if node.is_named() {
                tcl_inspect_container(&node, list_node, conditions);
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

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
fn tcl_ternary_slots<'a>(
    node: &Node<'a>,
) -> (Option<Node<'a>>, Option<Node<'a>>, Option<Node<'a>>) {
    let (mut condition, mut consequence, mut alternative) = (None, None, None);
    let mut seen_question = false;
    let mut seen_colon = false;

    for child in node.children() {
        match child.kind_id().into() {
            Tcl::QMARK => seen_question = true,
            Tcl::COLON => seen_colon = true,
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
fn tcl_walk_ternary(node: &Node, conditions: &mut f64) {
    let (condition, consequence, alternative) = tcl_ternary_slots(node);
    if let Some(condition) = condition {
        if matches!(condition.kind_id().into(), tcl_bool_terminal_kinds!()) {
            *conditions += 1.;
        } else {
            tcl_inspect_container(&condition, node, conditions);
        }
    }
    for branch in [consequence, alternative].into_iter().flatten() {
        tcl_inspect_container(&branch, node, conditions);
    }
}

/// The `expr` wrapper holding an `if` / `elseif` / `while` predicate.
///
/// `while` exposes no `condition` field at all — the grammar is
/// `seq('while', $.expr, $._word)` — so the slot is found by kind. Match
/// `Tcl::Expr` (the braced `{ … }` expression node) and not
/// `Tcl::Expr2`, which is the `expr` *command keyword* under
/// `expr_cmd` and renders to the same name (grammar-dispatch item 1).
/// `Tcl::Expr3` is the hidden `_expr` supertype the parser never emits
/// (item 2).
fn tcl_condition_expr<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    node.children()
        .find(|child| matches!(child.kind_id().into(), Tcl::Expr))
}

impl Abc for TclCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        match node.kind_id().into() {
            // The `set` production wraps `set name value` as a
            // first-class node distinct from generic commands.
            Tcl::Set => {
                stats.assignments += 1.;
            }
            // Generic command: branch by default, assignment when
            // the first word names a known mutator. The first word
            // can be either a `simple_word` or a wrapped form; both
            // surface their literal text via `utf8_text`.
            Tcl::Command => {
                if tcl_command_is_assignment(node, code) {
                    stats.assignments += 1.;
                } else {
                    stats.branches += 1.;
                }
            }
            Tcl::EQEQ
            | Tcl::BANGEQ
            | Tcl::LT
            | Tcl::GT
            | Tcl::LTEQ
            | Tcl::GTEQ
            | Tcl::Eq
            | Tcl::Ne
            | Tcl::In
            | Tcl::Ni
            | Tcl::Else => {
                stats.conditions += 1.;
            }
            // Phase 2B slot routing (#1180). `if` / `while` / `elseif`
            // carry their predicate in an `expr` wrapper; routing it is
            // what makes a bare truthy test (`if {$a}`) and a negated one
            // (`if {!$a}`) count at all. A comparison predicate is
            // unaffected: its `binop_expr` is neither a wrapper nor a
            // negation, so the walker breaks without counting and the
            // operator token arm above still supplies the one condition.
            Tcl::If | Tcl::While => {
                if let Some(expr) = tcl_condition_expr(node) {
                    tcl_inspect_container(&expr, node, &mut stats.conditions);
                }
            }
            // `elseif` is both a clause (one condition, as before) and a
            // predicate owner, matching the C family, where
            // `if (a) {} else if (b) {}` scores 3.
            Tcl::Elseif => {
                stats.conditions += 1.;
                if let Some(expr) = tcl_condition_expr(node) {
                    tcl_inspect_container(&expr, node, &mut stats.conditions);
                }
            }
            // The `?` marker is one condition, as before; its three
            // operand slots are new.
            Tcl::TernaryExpr => {
                stats.conditions += 1.;
                tcl_walk_ternary(node, &mut stats.conditions);
            }
            // Fitzpatrick Rule 9 walker: each operand of a `&&` / `||`
            // chain inside an `expr` slot is one condition (issue #403).
            Tcl::AMPAMP | Tcl::PIPEPIPE => {
                if let Some(parent) = ancestors.parent(node) {
                    tcl_count_unary_conditions(&parent, &mut stats.conditions);
                }
            }
            _ => {}
        }
    }
}

// Returns true when the `command` node's first word is one of the
// recognised Tcl assignment commands. The first word is the leftmost
// non-comment child; we slice the source bytes directly using the
// child node's byte range, which is robust to `simple_word` wrappers
// and avoids depending on a particular grammar shape.
fn tcl_command_is_assignment(node: &Node, code: &[u8]) -> bool {
    let Some(first) = node.child(0) else {
        return false;
    };
    let start = first.start_byte();
    let end = first.end_byte();
    if end > code.len() || start >= end {
        return false;
    }
    let word = &code[start..end];
    TCL_ASSIGNMENT_COMMANDS.contains(&word)
}
