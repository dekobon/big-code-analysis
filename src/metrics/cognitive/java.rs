//! `Cognitive` implementation for Java.
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

impl Cognitive for JavaCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        nesting_map: &mut NestingMap,
    ) {
        // bca: suppress(cyclomatic, halstead) — exhaustive grammar dispatch
        // One arm per construct the grammar can hand us, each a row read
        // or a kind the rows do not own: the count is the construct list,
        // not branching a reader must hold, and the effort is the table's
        // vocabulary (#1695, #1697).
        use crate::roles::java::{
            branch, catch, closure, else_if, func_opener, is_switch_body_kind, r#loop,
            short_circuit, ternary,
        };
        use Java::*;

        let mut nesting = get_nesting_from_map(node, nesting_map);

        let kind = node.kind_id();

        match kind.into() {
            // An `if_statement` the `Branch` row owns opens a nesting
            // level — unless the `ElseIf` row recognises it as the
            // continuation of an `else if` chain (its previous sibling is
            // the `else` token), where the `Else` arm below pays the +1
            // and no level opens (#1697). The chain read is asked only of
            // `if`s, inside this guard, so no other node pays for it.
            _ if branch::owns_kind(kind, node, code, ancestors)
                && !else_if::owns_kind(kind, node, code, ancestors) =>
            {
                increase_nesting(stats, &mut nesting);
            }
            // A `binary_expression` the `ShortCircuit` row owns: its
            // `&&` / `||` token — the row's payer — joins or opens a
            // boolean sequence (#1694).
            _ if short_circuit::owns_kind(kind, node, code, ancestors) => {
                compute_booleans_from(node, stats, short_circuit::PAYERS);
            }
            // Every loop the `Loop` row owns, every handler the `Catch`
            // row owns, and the body of the switch the `Switch` row owns
            // — the level opens at the body, not the owner;
            // `is_switch_body_kind` says why.
            _ if r#loop::owns_kind(kind, node, code, ancestors)
                || catch::owns_kind(kind, node, code, ancestors)
                || is_switch_body_kind(kind) =>
            {
                increase_nesting(stats, &mut nesting);
            }
            // The `ternary_expression` the `Ternary` row owns (#1695).
            _ if ternary::owns_kind(kind, node, code, ancestors) => {
                increase_nesting(stats, &mut nesting);
            }
            // `Else` here is the `else` keyword token, which the grammar
            // also emits for the `else` of an `else if` — so this arm
            // covers both.
            Else => {
                increment_by_one(stats);
            }
            // Per SonarSource Cognitive Complexity §B2, labeled `break LABEL`
            // and `continue LABEL` each add +1 for breaking the structured
            // control flow. Plain `break;` / `continue;` are not penalized.
            BreakStatement | ContinueStatement if node.is_child(Identifier as u16) => {
                increment_by_one(stats);
            }
            // A lambda — the `Closure` row — charges the lambda channel
            // and does not reset nesting.
            _ if closure::owns_kind(kind, node, code, ancestors) => {
                nesting.lambda += 1;
            }
            // At a (possibly nested) method / constructor boundary — every
            // owner of the `FuncOpener` row, `static { … }` included
            // (#1184: it opens a `FuncSpace`, so without this an
            // initialiser written inside a nested class inherited the
            // enclosing nesting), which is why this reads `owns` and
            // `OWNERS` rather than the callables, as Kotlin and Groovy do
            // — reset structural nesting to zero and bump the
            // function-depth surcharge when this declaration is itself
            // nested inside another. Without this, a method declared
            // inside a control construct (Java local / member classes)
            // inherited the enclosing nesting and every nested method
            // missed the SonarSource B-nesting amplification (#696). A
            // record's compact constructor is its own kind (#1160) and is
            // on the row.
            _ if func_opener::owns_kind(kind, node, code, ancestors) => {
                enter_function_boundary(&mut nesting, node, ancestors, func_opener::OWNERS);
            }
            _ => {}
        }
        nesting_map.insert(node.id(), nesting);
    }
}
