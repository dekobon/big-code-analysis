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

impl Abc for IrulesCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
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
            | Irules::TernaryExpr
            | Irules::Elseif
            | Irules::Else => {
                stats.conditions += 1.;
            }
            // Fitzpatrick Rule 9: the short-circuit operators are not counted
            // directly (cross-language policy, #395); instead each operand of
            // a `&&`/`||`/`and`/`or` chain is one condition (#403). iRules'
            // keyword forms (`and`/`or`) get the same treatment as `&&`/`||`.
            Irules::AMPAMP | Irules::PIPEPIPE | Irules::And | Irules::Or => {
                if let Some(parent) = node.parent() {
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
fn irules_inspect_container(container_node: &Node, conditions: &mut f64) {
    let mut node = *container_node;
    let mut node_kind = node.kind_id().into();
    let Some(parent) = node.parent() else { return };
    let has_boolean_content = matches!(parent.kind_id().into(), Irules::BinopExpr);

    loop {
        let is_not = matches!(node_kind, Irules::UnaryExpr)
            && node
                .child(0)
                .is_some_and(|c| c.kind_id() == Irules::BANG as u16);

        if !is_not {
            break;
        }

        let Some(child) = node.child(1) else { break };
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
                irules_inspect_container(&node, conditions);
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}
