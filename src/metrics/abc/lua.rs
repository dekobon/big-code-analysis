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
    Abc, Stats, count_boolean_slot, count_negated_operand, for_each_named_child, wrapped_operand,
};
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

// One step of the `(...)` / `not` peel (see `PeelStep`). Each operand
// is read by role rather than at child(1) after the `(` or the `not`
// keyword, where a comment may sit (`not --[[c]] b`, #1455):
// `unary_expression` names its `operand`, and a parenthesis holds only
// its operand. `not` is the one wrapper that proves its operand
// boolean; a `unary_expression` spelled `-x`, `#t` or `~x` yields a
// value and stops the peel.
fn lua_wrapper_operand<'a>(node: &Node<'a>) -> Option<(Node<'a>, bool)> {
    match node.kind_id().into() {
        Lua::ParenthesizedExpression => wrapped_operand(node).map(|o| (o, false)),
        Lua::UnaryExpression
            if node
                .child(0)
                .is_some_and(|c| c.kind_id() == Lua::Not as u16) =>
        {
            node.child_by_field_name("operand").map(|o| (o, true))
        }
        _ => None,
    }
}

// Whether an arm of `compute` already charges `expr` (already peeled) as
// a condition: a comparison (the token arms) or an `and` / `or` chain,
// whose operands each pay through `lua_count_condition`.
fn lua_condition_scores_itself(expr: &Node) -> bool {
    expr.kind_id() == Lua::BinaryExpression
        && expr.child_by_field_name("operator").is_some_and(|op| {
            matches!(
                op.kind_id().into(),
                Lua::EQEQ
                    | Lua::TILDEEQ
                    | Lua::LT
                    | Lua::GT
                    | Lua::LTEQ
                    | Lua::GTEQ
                    | Lua::And
                    | Lua::Or
            )
        })
}

// Scores one boolean slot — an `if` / `elseif` / `while` / `repeat …
// until` condition or an operand of an `and` / `or` chain (see
// `count_boolean_slot`). Lua is truthy-valued, so `if -x`, `if #t` and
// `if x + 1` are each a decision, and each scored 0 while the slot paid
// only for a fixed list of terminal kinds (#1526).
fn lua_count_condition(condition: &Node, conditions: &mut f64) {
    count_boolean_slot(
        condition,
        lua_wrapper_operand,
        lua_condition_scores_itself,
        conditions,
    );
}

// An operand outside a boolean slot — a `return` value, a call argument
// — scores only when a `not` proves it boolean (see
// `count_negated_operand`): `return not x` scores one, `return x` none.
fn lua_count_negated(operand: &Node, conditions: &mut f64) {
    count_negated_operand(
        operand,
        lua_wrapper_operand,
        lua_condition_scores_itself,
        conditions,
    );
}

fn lua_count_slot(slot: Option<Node>, conditions: &mut f64) {
    if let Some(condition) = slot {
        lua_count_condition(&condition, conditions);
    }
}

// Each operand of an `and` / `or` chain is a boolean slot (Fitzpatrick
// Rule 9, #403). `a and b or c` is a left-nested chain of
// `binary_expression`s, so an operand that is itself a chain is paid by
// its own operator's visit. Read by field: a comment beside the operator
// is a named child too, and must not pay.
fn lua_count_chain_operands(chain: &Node, conditions: &mut f64) {
    if chain.kind_id() == Lua::BinaryExpression {
        for field in ["left", "right"] {
            lua_count_slot(chain.child_by_field_name(field), conditions);
        }
    }
}

// Each named child of an `expression_list` or `arguments` list is a
// negated operand. `parent` is unused: `for_each_named_child` serves
// the walkers that seed a boolean-context flag from it.
fn lua_count_negated_child(operand: &Node, _parent: &Node, conditions: &mut f64) {
    lua_count_negated(operand, conditions);
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
            Lua::EQEQ | Lua::TILDEEQ | Lua::LTEQ | Lua::GTEQ | Lua::ElseStatement => {
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
                if let Some(chain) = ancestors.parent(node) {
                    lua_count_chain_operands(&chain, &mut stats.conditions);
                }
            }
            // An `elseif` is Java's `else if`: the `else` (+1, Rule 5) and
            // an `if` predicate slot, as Ruby's `elsif` is. It paid only
            // the first, so `elseif b` scored one below `elseif x > 0`
            // (#1526).
            Lua::ElseifStatement => {
                stats.conditions += 1.;
                lua_count_slot(node.child_by_field_name("condition"), &mut stats.conditions);
            }
            // Phase-2B (issue #403): condition slots. Lua has no paren
            // wrap around `if` / `while` / `repeat …` conditions, so
            // `lua_count_slot` classifies the slot directly. Use
            // `child_by_field_name("condition")` so the lookup is
            // grammar-version-robust — tree-sitter-lua exposes the
            // `condition` field on if/while/repeat statements. Pinning
            // by name handles the rare empty-body `repeat until cond`
            // shape where the BLANK alternative for the body would
            // shift positional child indices.
            Lua::IfStatement | Lua::WhileStatement | Lua::RepeatStatement => {
                lua_count_slot(node.child_by_field_name("condition"), &mut stats.conditions);
            }
            // `return value` — Lua wraps return values in an
            // `expression_list`, the statement's only operand (not
            // child(1), where `return --[[c]] not x` puts a comment —
            // #1455). Each named child is a negated operand, so
            // `return not x` counts once while `return x` (bare)
            // reports zero. Bare `return` (no values) has no operand.
            Lua::ReturnStatement => {
                if let Some(expr_list) = wrapped_operand(node) {
                    for_each_named_child(
                        &expr_list,
                        &mut stats.conditions,
                        lua_count_negated_child,
                    );
                }
            }
            // `f(not a, not b)` — argument-list walker.
            Lua::Arguments => {
                for_each_named_child(node, &mut stats.conditions, lua_count_negated_child);
            }
            _ => {}
        }
    }
}
