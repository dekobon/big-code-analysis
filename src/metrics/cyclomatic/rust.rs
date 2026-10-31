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
    use crate::roles::rust::{branch, case_arm, r#loop, short_circuit, switch};
    let kind = node.kind_id();
    // Every decision is a row of `roles/rust.rs`, read at the projection
    // the metric counts (#1695, #1697). Rust counts keyword tokens, so
    // `Branch` and `Loop` are read at the leaf: the `if` keyword wherever
    // it is written — an `if` expression's, an `if let`'s, and a match
    // guard's, which is the `Guard` row's payer too; the two rows share
    // the token and it pays once — and `for` / `while` / `loop`. A `let
    // … else` is the `Branch` row's gated owner, paid at the node: one
    // decision, as its `if let` twin pays, keyed on the `alternative`
    // field rather than the `else` token the `if` expression also emits.
    // `&&` / `||` from `ShortCircuit` at the token. `?` is one of the
    // `Exit` row's two owners and a decision only under `count_try`
    // (#409): `true` counts it toward both tiers, `false` treats it as
    // linear error propagation. The other owner, `return`, is never a
    // decision, so this names the kind rather than reading the row.
    // role-bypass: allow(Exit) — the `count_try` policy on one `Exit`
    // owner, not exit membership
    if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors)
        || branch::pays_at_leaf_kind(kind, node, code, ancestors)
        || r#loop::pays_at_leaf_kind(kind, node, code, ancestors)
        || (count_try && kind == Rust::TryExpression as u16)
    {
        stats.cyclomatic += 1.;
        stats.cyclomatic_modified += 1.;
        return;
    }
    // Standard-only: each match arm the `CaseArm` row owns. Lizard counts
    // `match` as a single control-flow keyword; we count each arm, so the
    // modified metric collapses them back to the container, the
    // `match_expression` the `Switch` row owns. A bare wildcard `_ =>`
    // arm is the C-family `default:` and the row leaves it out —
    // `Some(_)`, `(_, x)` and `_ if guard` are not bare and still count.
    // The guard's own `if` is `Branch`'s keyword above, as the `Guard`
    // row says.
    if case_arm::owns_kind(kind, node, code, ancestors) {
        stats.cyclomatic += 1.;
    } else if switch::owns_kind(kind, node, code, ancestors) {
        stats.cyclomatic_modified += 1.;
    }
}
