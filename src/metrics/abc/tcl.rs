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
const TCL_ASSIGNMENT_COMMANDS: &[&str] = &["incr", "append", "lappend"];

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
// One step of the value peel (see `PeelStep`). The `expr` wrapper is
// this grammar's `{ … }` predicate node — the analogue of the C family's
// `condition_clause` — and holds one operand; `!` is the one unary that
// proves its operand boolean, while `-$x` / `~$x` yield a value and stop
// the peel. Both operands are the first *named* child that is not an
// `extra`: `!` and the `{` / `(` delimiters are all anonymous, so this
// is the operand in every shape, and unlike a fixed index it survives
// `_expr` inlining its parens — `!($a)` puts `(` at child 1.
fn tcl_wrapper_operand<'a>(node: &Node<'a>) -> Option<(Node<'a>, bool)> {
    match node.kind_id().into() {
        Tcl::Expr => wrapped_operand(node).map(|o| (o, false)),
        Tcl::UnaryExpr
            if node
                .child(0)
                .is_some_and(|c| c.kind_id() == Tcl::BANG as u16) =>
        {
            wrapped_operand(node).map(|o| (o, true))
        }
        _ => None,
    }
}

// Whether an arm of `compute` already charges `expr` (already peeled) as
// a condition: a ternary (its own arm), or a `binop_expr` applying a
// comparison (the token arm) or `&&` / `||`, whose operands each pay
// through `tcl_count_condition`. The grammar names no operator field, so
// the operator is found among the node's tokens.
fn tcl_condition_scores_itself(expr: &Node) -> bool {
    match expr.kind_id().into() {
        Tcl::TernaryExpr => true,
        Tcl::BinopExpr => expr.children().any(|token| {
            matches!(
                token.kind_id().into(),
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
                    | Tcl::AMPAMP
                    | Tcl::PIPEPIPE
            )
        }),
        _ => false,
    }
}

// Scores one boolean slot — an `if` / `elseif` / `while` predicate, a
// ternary condition, an operand of an `&&` / `||` chain (see
// `count_boolean_slot`). `expr` tests any number for non-zero, so
// `if {-$x}` and `if {$x + 1}` are each a decision, and each scored 0
// while the slot paid only for a fixed list of terminal kinds (#1526).
fn tcl_count_condition(condition: &Node, conditions: &mut f64) {
    count_boolean_slot(
        condition,
        tcl_wrapper_operand,
        tcl_condition_scores_itself,
        conditions,
    );
}

// Each operand of a chain is a boolean slot (Fitzpatrick Rule 9, #403).
// The operands are the chain's named children: the grammar names no
// operand field, and inlines the parens of `($a)`.
fn tcl_count_chain_operands(chain: &Node, conditions: &mut f64) {
    if chain.kind_id() == Tcl::BinopExpr {
        count_each_operand(chain, tcl_count_condition, conditions);
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

/// Routes the three ternary slots (#1180): the condition is a boolean
/// slot, and each branch counts only if a `!` makes it a negated operand
/// (see `count_negated_operand`).
fn tcl_walk_ternary(node: &Node, conditions: &mut f64) {
    let (condition, consequence, alternative) = tcl_ternary_slots(node);
    if let Some(condition) = condition {
        tcl_count_condition(&condition, conditions);
    }
    for branch in [consequence, alternative].into_iter().flatten() {
        count_negated_operand(
            &branch,
            tcl_wrapper_operand,
            tcl_condition_scores_itself,
            conditions,
        );
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
            // carry their predicate in an `expr` wrapper, which is a
            // boolean slot. A comparison predicate pays through the
            // operator token arm above, not again through the slot.
            Tcl::If | Tcl::While => {
                if let Some(expr) = tcl_condition_expr(node) {
                    tcl_count_condition(&expr, &mut stats.conditions);
                }
            }
            // `elseif` is both a clause (one condition, as before) and a
            // predicate owner, matching the C family, where
            // `if (a) {} else if (b) {}` scores 3.
            Tcl::Elseif => {
                stats.conditions += 1.;
                if let Some(expr) = tcl_condition_expr(node) {
                    tcl_count_condition(&expr, &mut stats.conditions);
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
                if let Some(chain) = ancestors.parent(node) {
                    tcl_count_chain_operands(&chain, &mut stats.conditions);
                }
            }
            _ => {}
        }
    }
}

// Returns true when the `command` node's leading word is one of the
// recognised Tcl assignment commands. The word is read through the shared
// `tcl_command_name` — field-addressed (`name`) and gated on `simple_word`
// (grammar-dispatch §3) — rather than by slicing `child(0)`'s byte range,
// which addressed the slot positionally and read the literal text of
// whatever sat there. A command with a computed leading word (`$cmd x`,
// `[pick] x`) therefore stays a branch: it is not statically a builtin,
// which is what the assignment classification claims.
fn tcl_command_is_assignment(node: &Node, code: &[u8]) -> bool {
    crate::lang_helpers::tcl::tcl_command_name(node, code)
        .is_some_and(|name| TCL_ASSIGNMENT_COMMANDS.contains(&name))
}
