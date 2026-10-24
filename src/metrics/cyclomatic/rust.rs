//! `Cyclomatic` implementation for Rust.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cyclomatic for RustCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        // The default (#409): `?` counts toward cyclomatic, matching
        // upstream rust-code-analysis and every published metric value.
        Self::compute_with_options(node, code, ancestors, stats, true);
    }

    fn compute_with_options<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        count_try: bool,
    ) {
        rust_cyclomatic_increment(node, code, ancestors, stats, count_try);
    }
}

/// Rust's per-node cyclomatic increment, shared by both
/// [`Cyclomatic::compute`] and [`Cyclomatic::compute_with_options`].
///
/// Extracted as a free function so the `impl Cyclomatic for RustCode`
/// block carries only the two thin trait methods — the bare-wildcard
/// closure lives here instead, keeping the impl block's aggregate
/// `nargs` within the self-scan gate.
///
/// `count_try` toggles the `?` operator's contribution (#409): `true`
/// counts it toward both standard and modified cyclomatic, `false`
/// treats it as linear error propagation.
fn rust_cyclomatic_increment<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
    count_try: bool,
) {
    use crate::roles::rust::{case_arm, short_circuit};
    use Rust::*;

    let kind = node.kind_id();
    match kind.into() {
        // Standard-only: each match arm the `CaseArm` row owns. Lizard
        // counts `match` as a single control-flow keyword; we count each
        // arm, so the modified metric collapses them back to the
        // container. A bare wildcard `_ =>` arm is the C-family
        // `default:` and the row leaves it out — `Some(_)`, `(_, x)` and
        // `_ if guard` are not bare and still count. The guard's own `if`
        // is the `If` arm below, as the `Guard` row says.
        _ if case_arm::owns_kind(kind, node, code, ancestors) => {
            stats.cyclomatic += 1.;
        }
        // Modified-only: the match expression container.
        MatchExpression => {
            stats.cyclomatic_modified += 1.;
        }
        // The `?` operator. Counted toward both standard and modified by
        // default; when `count_try` is false the arm's guard fails and
        // `?` falls through to `_ => {}`, treating it as linear error
        // propagation (#409). Gated separately from the unconditional
        // branching kinds below.
        TryExpression if count_try => {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
        }
        // `let PAT = e else { … };` either binds or diverges: one
        // decision, as its `if let` twin pays. Keyed on the
        // `alternative` field rather than the shared `else` token, which
        // `if … else` also emits.
        LetDeclaration if node.child_by_field_name("alternative").is_some() => {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
        }
        // Both standard and modified. `&&` / `||` are the `ShortCircuit`
        // row's payers (#1695), read on the arm's guard.
        _ if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors) => {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
        }
        // The `if` keyword wherever it is written — an `if` expression's
        // and a match guard's alike, the latter the `Guard` row's payer,
        // read here as the keyword it is since the row pays nothing more.
        // role-bypass: allow(Guard) — the keyword arm the row's gated payer is one spelling of:
        // the `if` expression's keyword is `Branch`'s, pending R07
        If | For | While | Loop => {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
        }
        _ => {}
    }
}
