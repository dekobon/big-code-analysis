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
    Abc, Stats, count_boolean_slot, count_each_operand, count_negated_operand, wrapped_operand,
};
use crate::roles::irules::{comparison, short_circuit, ternary};
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

    // role-bypass: allow(Ternary) — traversal: the `?` / `:` markers the operand slots are read
    // relative to, inside an owner the row already admitted
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

/// Routes the three ternary slots (#1180) — see `tcl_walk_ternary`.
fn irules_walk_ternary(node: &Node, code: &[u8], conditions: &mut f64) {
    let (condition, consequence, alternative) = irules_ternary_slots(node);
    if let Some(condition) = condition {
        irules_count_condition(&condition, code, conditions);
    }
    for branch in [consequence, alternative].into_iter().flatten() {
        count_negated_operand(
            &branch,
            irules_wrapper_operand,
            |expr| irules_condition_scores_itself(expr, code),
            conditions,
        );
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
        // bca: suppress(cyclomatic) — exhaustive kind dispatch table
        // One arm per grammar kind, like `CppCode::compute`: the count is
        // the number of node kinds the grammar can hand us, not branching
        // a reader must hold; the two row reads (#1695) are arms like
        // the token arms they replaced.
        let kind = node.kind_id();
        match kind.into() {
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
            Irules::Else => {
                stats.conditions += 1.;
            }
            // The comparison tokens, the `Comparison` row's payers
            // (`roles/irules.rs`): Tcl's ten plus the word-form string
            // comparators (`starts_with`, `contains`, `matches`, …) Tcl
            // lacks.
            _ if comparison::pays_at_leaf_kind(kind, node, code, ancestors) => {
                stats.conditions += 1.;
            }
            // Phase 2B slot routing (#1180) — see `tcl.rs`, which this
            // mirrors arm for arm.
            Irules::If | Irules::While => {
                if let Some(expr) = irules_condition_expr(node) {
                    irules_count_condition(&expr, code, &mut stats.conditions);
                }
            }
            Irules::Elseif => {
                stats.conditions += 1.;
                if let Some(expr) = irules_condition_expr(node) {
                    irules_count_condition(&expr, code, &mut stats.conditions);
                }
            }
            // The `ternary_expr` the `Ternary` row owns, read at the
            // wrapper: one condition, as before, plus its three operand
            // slots.
            _ if ternary::owns_kind(kind, node, code, ancestors) => {
                stats.conditions += 1.;
                irules_walk_ternary(node, code, &mut stats.conditions);
            }
            // Fitzpatrick Rule 9: the short-circuit operators are not counted
            // directly (cross-language policy, #395); instead each operand of
            // a `&&`/`||`/`and`/`or` chain is one condition (#403). iRules'
            // keyword forms (`and`/`or`) get the same treatment as `&&`/`||`.
            _ if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors) => {
                if let Some(chain) = ancestors.parent(node) {
                    irules_count_chain_operands(&chain, code, &mut stats.conditions);
                }
            }
            _ => {}
        }
    }
}

// iRules mutator commands (same Tcl builtins; the dedicated `set`
// production is handled separately in the impl, like Tcl).
const IRULES_ASSIGNMENT_COMMANDS: &[&str] = &["incr", "append", "lappend"];

// iRules counterpart of `tcl_command_is_assignment`, and normalised the
// same way: `::incr` is `incr` through the global namespace, and scored
// as a branch rather than an assignment until the strip was shared
// (#1381 review). The leading word is still addressed by index rather
// than by the `name` field — a grammar-dispatch §3 smell this fix
// deliberately leaves alone, since changing the child selection is a
// behaviour change of its own.
fn irules_command_is_assignment(node: &Node, code: &[u8]) -> bool {
    let Some(first) = node.child(0) else {
        return false;
    };
    first
        .utf8_text(code)
        .map(crate::lang_helpers::strip_global_qualifier)
        .is_some_and(|word| IRULES_ASSIGNMENT_COMMANDS.contains(&word))
}

// iRules counterpart of `tcl_wrapper_operand`, which also takes the
// keyword spelling `not` — Tcl's `expr` has no such token — as Perl and
// Elixir do.
fn irules_wrapper_operand<'a>(node: &Node<'a>) -> Option<(Node<'a>, bool)> {
    match node.kind_id().into() {
        Irules::Expr => wrapped_operand(node).map(|o| (o, false)),
        Irules::UnaryExpr
            if node
                .child(0)
                .is_some_and(|c| matches!(c.kind_id().into(), Irules::BANG | Irules::Not)) =>
        {
            wrapped_operand(node).map(|o| (o, true))
        }
        _ => None,
    }
}

// iRules counterpart of `tcl_condition_scores_itself`, with the word-form
// string comparators and the keyword chain forms Tcl lacks.
fn irules_condition_scores_itself(expr: &Node, code: &[u8]) -> bool {
    let unknown = Ancestors::unknown();
    comparison::owns(expr, code, unknown)
        || short_circuit::owns(expr, code, unknown)
        || ternary::owns(expr, code, unknown)
}

// iRules counterpart of `tcl_count_condition` (#1526).
fn irules_count_condition(condition: &Node, code: &[u8], conditions: &mut f64) {
    count_boolean_slot(
        condition,
        irules_wrapper_operand,
        |expr| irules_condition_scores_itself(expr, code),
        conditions,
    );
}

// Each operand of a chain is a boolean slot (Fitzpatrick Rule 9, #403).
// The operands are the chain's named children: the grammar names no
// operand field, and inlines the parens of `($a)`. The chain is the
// `ShortCircuit` row's owner, the token's parent.
fn irules_count_chain_operands(chain: &Node, code: &[u8], conditions: &mut f64) {
    if short_circuit::owns(chain, code, Ancestors::unknown()) {
        count_each_operand(
            chain,
            |operand, conds| irules_count_condition(operand, code, conds),
            conditions,
        );
    }
}
