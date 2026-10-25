//! `Cognitive` implementation for Kotlin.
#![allow(
    clippy::enum_glob_use,
    clippy::match_same_arms,
    clippy::needless_pass_by_value,
    clippy::wildcard_imports
)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

/// Whether `node` is a labeled *jump* — `break@label` / `continue@label`.
///
/// tree-sitter-kotlin-ng has no break/continue/jump statement kind: it
/// models a labeled jump as a `labeled_expression` whose first child is a
/// `label` holding the *fused* jump keyword `break@` / `continue@` (the
/// loop-target identifier is a sibling). A labeled *non-jump*
/// (`lbl@ run { … }`, `lbl@ if (…) {…}`) is also a `labeled_expression`,
/// but its label is an ordinary `name@`, so gating on the label token
/// starting with `break@` / `continue@` excludes it (#450). `return@label`
/// is a distinct `return_expression` and never reaches here; a bare
/// `break` / `continue` parses as a plain identifier, never a
/// `labeled_expression`.
fn kotlin_is_labeled_jump(node: &Node, code: &[u8]) -> bool {
    node.child(0)
        .filter(|c| c.kind_id() == Kotlin::Label as u16)
        .and_then(|c| c.utf8_text(code))
        .is_some_and(|t| t.starts_with("break@") || t.starts_with("continue@"))
}

impl Cognitive for KotlinCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use crate::roles::kotlin::{closure, func_opener, null_safe, short_circuit};
        use Kotlin::*;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        let kind = node.kind_id();

        match kind.into() {
            IfExpression if !Self::is_else_if(node, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            ForStatement | WhileStatement | DoWhileStatement | WhenExpression | CatchBlock => {
                increase_nesting(stats, &mut nesting);
            }
            Else => {
                // Per the SonarSource spec, `else ->` inside a `when`
                // expression is the default arm of a switch-like construct
                // and should be +0, not +1.
                //
                // Read off the walker's chain rather than `Node::parent`,
                // which is `O(depth)` — one climb per `else` keeps the
                // metric quadratic on a deeply nested `if`/`else` chain,
                // the shape #1062 exists to make linear.
                // role-bypass: allow(CaseArm) — traversal: whether the `else` is a `when` entry's,
                // not whether the entry is an arm
                let in_when = ancestors.parent_has_kind(node, WhenEntry as u16);
                if !in_when {
                    increment_by_one(stats);
                }
            }
            // SonarSource §B2: labeled `break@outer` / `continue@outer`
            // each add +1 for breaking structured control flow; bare
            // `break` / `continue` are +0. See `kotlin_is_labeled_jump`
            // for how the grammar models the labeled-jump shape (#450).
            LabeledExpression if kotlin_is_labeled_jump(node, code) => {
                increment_by_one(stats);
            }
            // A `binary_expression` the `ShortCircuit` row owns, or one
            // the `NullSafe` row owns through Kotlin's Elvis operator
            // `?:`, a short-circuit nullish operator analogous to JS `??`
            // that forms boolean sequences alongside `&&` / `||` per
            // SonarSource Cognitive Complexity B1 (#1695). The row's
            // other owner, a safe navigation, is no `binary_expression`
            // and never reaches the key.
            _ if short_circuit::owns_kind(kind, node, code, ancestors)
                || null_safe::owns_kind(kind, node, code, ancestors) =>
            {
                compute_booleans_with(node, stats, |id| {
                    short_circuit::PAYERS.contains(&id) || id == QMARKCOLON as u16
                });
            }
            // Property accessors and `init { … }` are function
            // boundaries too (#1184). They open a `FuncSpace`, so without
            // this arm an accessor written inside an `if` inherited that
            // `if`'s nesting and scored more than the byte-identical
            // accessor at top level — the same defect #1160 fixed for
            // Java's compact constructor. They are `is_func_space`
            // without being `is_func`, which does not matter here: this
            // arm matches kinds directly.
            // Every owner of the `FuncOpener` row — the accessors and
            // `init` included, which are openers without being callable
            // — so this reads `owns` and `OWNERS`, not the callables.
            _ if func_opener::owns(node, code, ancestors) => {
                enter_function_boundary(&mut nesting, node, ancestors, func_opener::OWNERS);
            }
            _ if closure::owns(node, code, ancestors) => {
                nesting.lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
