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

use super::{Abc, Stats, for_each_named_child};
use crate::macros::lua_bool_terminal_kinds;
use crate::*;

// Fitzpatrick's ABC rules adapted for Lua.
//
// - Assignments: every `assignment_statement` node. Lua has no
//   compound assignment operators (`+=` and friends do not exist in
//   the grammar), so the wrapper kind is the sole assignment node
//   and there is no per-operator alternative to track. `local x = 1`
//   wraps an `assignment_statement` under a `variable_declaration`,
//   so initialisers count the same as later mutations.
// - Branches: every `function_call`. The Lua grammar collapses
//   `obj.method(args)`, `obj:method(args)`, and `f(args)` into the
//   same `function_call` node, so one arm covers all dispatch forms.
// - Conditions: comparison operators (`==`, `~=`, `<`, `>`, `<=`,
//   `>=`) and each elseif / else arm of an `if`. Lua has no ternary
//   operator (`cond and a or b` is the idiom). The short-circuit
//   operators `and` / `or` are deliberately NOT counted; see the
//   module-level `Stats` doc-comment for the cross-language policy
//   (issue #395, walker tracked in #403).
// Lua ABC unary-conditional walker (Fitzpatrick Rule 9; issue #403).
// Lua's logical operators are keyword tokens (`and` / `or`) inside a
// `binary_expression`; `not x` is a `unary_expression` whose first
// child is the `not` keyword. Terminal-bool kinds include identifiers,
// the three keyword literals (`true`, `false`, `nil`), numbers, and
// every call / indexing form.
fn lua_inspect_container(container_node: &Node, parent: &Node, conditions: &mut f64) {
    let mut node = *container_node;
    let mut node_kind = node.kind_id().into();
    let mut has_boolean_content = matches!(
        parent.kind_id().into(),
        Lua::BinaryExpression | Lua::IfStatement | Lua::WhileStatement | Lua::RepeatStatement
    );

    loop {
        let is_parens = matches!(node_kind, Lua::ParenthesizedExpression);
        let is_not = matches!(node_kind, Lua::UnaryExpression)
            && node
                .child(0)
                .is_some_and(|c| c.kind_id() == Lua::Not as u16);

        if !is_parens && !is_not {
            break;
        }
        // A `not` wrapper proves the operand is boolean even when
        // the parent context didn't — matches the JS/Java/etc.
        // pattern. Without this, `m(not a)` and other call-argument
        // contexts would never propagate has_boolean_content.
        has_boolean_content |= is_not;

        let Some(child) = node.child(1) else { break };
        node = child;
        node_kind = node.kind_id().into();

        if matches!(node_kind, lua_bool_terminal_kinds!()) {
            if has_boolean_content {
                *conditions += 1.;
            }
            break;
        }
    }
}

// Phase-2B (issue #403): Lua `if` / `while` / `repeat` condition
// slots. Lua has no paren wrap, so the condition has to be classified
// directly: terminal-bool kinds (Identifier, True, False, Nil,
// FunctionCall, etc.) count at the top level; `(...)` / `not ...`
// route through `lua_inspect_container`.
fn lua_count_condition(condition: &Node, parent: &Node, conditions: &mut f64) {
    let kind = condition.kind_id().into();
    if matches!(kind, lua_bool_terminal_kinds!()) {
        *conditions += 1.;
    } else if matches!(kind, Lua::ParenthesizedExpression | Lua::UnaryExpression) {
        lua_inspect_container(condition, parent, conditions);
    }
}

fn lua_count_unary_conditions(list_node: &Node, conditions: &mut f64) {
    let list_kind = list_node.kind_id().into();
    let mut cursor = list_node.cursor();

    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();
            let node_kind = node.kind_id().into();

            if matches!(node_kind, lua_bool_terminal_kinds!())
                && matches!(list_kind, Lua::BinaryExpression)
            {
                *conditions += 1.;
            } else if node.is_named() {
                lua_inspect_container(&node, list_node, conditions);
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

impl Abc for LuaCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        match node.kind_id().into() {
            Lua::AssignmentStatement | Lua::AssignmentStatement2 => {
                stats.assignments += 1.;
            }
            Lua::FunctionCall => {
                stats.branches += 1.;
            }
            Lua::EQEQ
            | Lua::TILDEEQ
            | Lua::LT
            | Lua::GT
            | Lua::LTEQ
            | Lua::GTEQ
            | Lua::ElseifStatement
            | Lua::ElseStatement => {
                stats.conditions += 1.;
            }
            // Fitzpatrick Rule 9 walker: each operand of an `and` /
            // `or` chain is one condition (issue #403).
            Lua::And | Lua::Or => {
                if let Some(parent) = ancestors.parent(node) {
                    lua_count_unary_conditions(&parent, &mut stats.conditions);
                }
            }
            // Phase-2B (issue #403): condition slots. Lua has no paren
            // wrap around `if` / `while` / `repeat …` conditions, so
            // `lua_count_condition` classifies the slot directly. Use
            // `child_by_field_name("condition")` so the lookup is
            // grammar-version-robust — tree-sitter-lua exposes the
            // `condition` field on if/while/repeat statements. Pinning
            // by name handles the rare empty-body `repeat until cond`
            // shape where the BLANK alternative for the body would
            // shift positional child indices.
            Lua::IfStatement | Lua::WhileStatement | Lua::RepeatStatement => {
                if let Some(cond) = node.child_by_field_name("condition") {
                    lua_count_condition(&cond, node, &mut stats.conditions);
                }
            }
            // `return value` — Lua wraps return values in
            // `expression_list` at child(1). Route each named child
            // through `inspect_container` (no top-level terminal
            // count) so `return not x` counts the unary unwrap once
            // while `return x` (bare) reports zero. Bare `return`
            // (no values) has no child(1).
            Lua::ReturnStatement => {
                if let Some(expr_list) = node.child(1) {
                    for_each_named_child(&expr_list, &mut stats.conditions, lua_inspect_container);
                }
            }
            // `f(not a, not b)` — argument-list walker.
            Lua::Arguments => {
                lua_count_unary_conditions(node, &mut stats.conditions);
            }
            _ => {}
        }
    }
}
