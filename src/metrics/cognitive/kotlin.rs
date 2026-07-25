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
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        use Kotlin::*;

        let (mut nesting, mut depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            IfExpression if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            ForStatement | WhileStatement | DoWhileStatement | WhenExpression | CatchBlock => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            Else => {
                // Per the SonarSource spec, `else ->` inside a `when`
                // expression is the default arm of a switch-like construct
                // and should be +0, not +1.
                let in_when = node.parent().is_some_and(|p| p.kind_id() == WhenEntry);
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
            BinaryExpression => {
                // Kotlin's Elvis operator `?:` (token `QMARKCOLON`) is a
                // short-circuit nullish operator analogous to JS `??` and
                // forms boolean sequences alongside `&&` / `||` per
                // SonarSource Cognitive Complexity B1.
                compute_booleans_with(node, stats, |id| {
                    matches!(id.into(), AMPAMP | PIPEPIPE | QMARKCOLON)
                });
            }
            FunctionDeclaration | SecondaryConstructor => {
                nesting = 0;
                increment_function_depth(
                    &mut depth,
                    node,
                    &[FunctionDeclaration, SecondaryConstructor],
                );
            }
            LambdaLiteral | AnonymousFunction => {
                lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}
