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

use super::{Abc, Stats, for_each_named_child, wrapped_operand};
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
// One step of the `(...)` / `not` peel: the operand a wrapper wraps, and
// whether the wrapper itself proves that operand boolean. `None` for
// anything this peel does not descend, which is also the answer
// `lua_count_condition` asks for, so the slot and the peel cannot
// disagree about which kinds are wrappers (#1470; the Kotlin, Groovy
// and C# instances of that disagreement were #1459, #1466 and #1463).
// Each operand is read by role rather than at child(1) after the `(` or
// the `not` keyword, where a comment may sit (`not --[[c]] b`, #1455):
// `unary_expression` names its `operand`, and a parenthesis holds only
// its operand. A `unary_expression` spelled `-x`, `#t` or `~x` is
// arithmetic, length or bitwise — never a boolean slot's operand — so
// the peel declines it.
//
// A `not` proves the operand boolean even when the parent context does
// not — matching the JS / Java pattern. Without that, `m(not a)` and
// other call-argument contexts would never set the walker's flag.
fn lua_wrapper_operand<'a>(node: &Node<'a>) -> Option<(Node<'a>, bool)> {
    match node.kind_id().into() {
        Lua::ParenthesizedExpression => Some((wrapped_operand(node)?, false)),
        Lua::UnaryExpression if node.child(0)?.kind_id() == Lua::Not as u16 => {
            Some((node.child_by_field_name("operand")?, true))
        }
        _ => None,
    }
}

// Lua ABC unary-conditional walker (Fitzpatrick Rule 9; issue #403).
// Lua's logical operators are keyword tokens (`and` / `or`) inside a
// `binary_expression`; `not x` is a `unary_expression` whose first
// child is the `not` keyword. Terminal-bool kinds include identifiers,
// the three keyword literals (`true`, `false`, `nil`), numbers, and
// every call / indexing form.
fn lua_inspect_container(container_node: &Node, parent: &Node, conditions: &mut f64) {
    let mut node = *container_node;
    let mut has_boolean_content = matches!(
        parent.kind_id().into(),
        Lua::BinaryExpression | Lua::IfStatement | Lua::WhileStatement | Lua::RepeatStatement
    );

    while let Some((operand, proves_boolean)) = lua_wrapper_operand(&node) {
        has_boolean_content |= proves_boolean;
        node = operand;

        if matches!(node.kind_id().into(), lua_bool_terminal_kinds!()) {
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
    if matches!(condition.kind_id().into(), lua_bool_terminal_kinds!()) {
        *conditions += 1.;
    } else if lua_wrapper_operand(condition).is_some() {
        // Asking the peel itself which kinds it unwraps, rather than
        // restating the list here (#1470): a restated list that gained a
        // kind the peel lacked would read as covering a shape the peel
        // then dropped (`.claude/rules/grammar-dispatch.md` §7).
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
            | Lua::LTEQ
            | Lua::GTEQ
            | Lua::ElseifStatement
            | Lua::ElseStatement => {
                stats.conditions += 1.;
            }
            // Counts `<` / `>` only as the operator token of a
            // `binary_expression`, the allowlist polarity C / C++ /
            // Rust / Go / Java use. Ungated, Lua 5.4's to-be-closed and
            // constant attributes scored two conditions apiece:
            // `local x <const> = 1` brackets the attribute name with the
            // same two bare tokens a comparison uses (#1297). A
            // `grammar.json` sweep of tree-sitter-lua 0.5.0 finds them
            // in exactly two productions — `binary_expression` and the
            // hidden `_attrib`, which surfaces aliased as `attribute` —
            // so the gate is closed rather than a coverage claim
            // (`.claude/rules/grammar-dispatch.md` §1). `<=` / `>=` and
            // the 5.3 shifts `<<` / `>>` are distinct tokens and never
            // reach this arm.
            Lua::LT | Lua::GT if ancestors.parent_has_kind(node, Lua::BinaryExpression as u16) => {
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
            // `return value` — Lua wraps return values in an
            // `expression_list`, the statement's only operand (not
            // child(1), where `return --[[c]] not x` puts a comment —
            // #1455). Route each named child
            // through `inspect_container` (no top-level terminal
            // count) so `return not x` counts the unary unwrap once
            // while `return x` (bare) reports zero. Bare `return`
            // (no values) has no operand.
            Lua::ReturnStatement => {
                if let Some(expr_list) = wrapped_operand(node) {
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
