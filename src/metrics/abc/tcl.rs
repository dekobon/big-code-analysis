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
    let mut node = *container_node;
    let mut node_kind = node.kind_id().into();
    let has_boolean_content = matches!(parent.kind_id().into(), Tcl::BinopExpr);

    loop {
        let is_not = matches!(node_kind, Tcl::UnaryExpr)
            && node
                .child(0)
                .is_some_and(|c| c.kind_id() == Tcl::BANG as u16);

        if !is_not {
            break;
        }

        let Some(child) = node.child(1) else { break };
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
            | Tcl::TernaryExpr
            | Tcl::Elseif
            | Tcl::Else => {
                stats.conditions += 1.;
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
