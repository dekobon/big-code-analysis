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
        use crate::roles::kotlin::short_circuit;
        use Kotlin::*;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
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
            // A `binary_expression` the `ShortCircuit` row owns (#1695),
            // or one applying Kotlin's Elvis operator `?:` (token
            // `QMARKCOLON`), a short-circuit nullish operator analogous
            // to JS `??` that forms boolean sequences alongside `&&` /
            // `||` per SonarSource Cognitive Complexity B1 — pending its
            // `NullSafe` row.
            // role-bypass: allow(Comparison, ShortCircuit) — the elvis owner, which the
            // `NullSafe` row of #1695 takes over; the row is read on the arm's guard
            _ if short_circuit::owns(node, code, ancestors)
                || (node.kind_id() == BinaryExpression as u16
                    && node
                        .child_by_field_name("operator")
                        .is_some_and(|op| op.kind_id() == QMARKCOLON as u16)) =>
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
            FunctionDeclaration | SecondaryConstructor | Getter | Setter | AnonymousInitializer => {
                enter_function_boundary(
                    &mut nesting,
                    node,
                    ancestors,
                    &[
                        FunctionDeclaration,
                        SecondaryConstructor,
                        Getter,
                        Setter,
                        AnonymousInitializer,
                    ],
                );
            }
            LambdaLiteral | AnonymousFunction => {
                nesting.lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
