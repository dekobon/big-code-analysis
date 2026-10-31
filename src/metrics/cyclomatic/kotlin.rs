//! `Cyclomatic` implementation for Kotlin.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Every decision is a row of `roles/kotlin.rs`, read at the projection
// the metric counts (#1695, #1697). Kotlin counts expression / statement
// nodes, so `Branch` / `Loop` / `Catch` / `Switch` are plain rows read
// at the owner. `&&` / `||` from `ShortCircuit` at the token; the Elvis
// `?:` and the safe-navigation `?.` from `NullSafe` at the token — each
// a short-circuit decision, once per textual operator inside a
// `navigation_expression` (so `a?.b?.c` is +2, #281, #436), paralleling
// TS/TSX; each `when` entry from `CaseArm` at the owner (the row leaves
// the `else` entry out); the `when_expression` from `Switch`, the
// modified tier's container. Kotlin has no ternary (`if` is an
// expression) and its `Guard` row is absent.
impl_cyclomatic_rows!(
    KotlinCode,
    Kotlin,
    roles = crate::roles::kotlin,
    both = [
        short_circuit::pays_at_leaf_kind,
        null_safe::pays_at_leaf_kind,
        branch::pays_at_leaf_kind,
        r#loop::pays_at_leaf_kind,
        catch::pays_at_leaf_kind,
    ],
    standard = [case_arm::owns_kind],
    modified = [switch::owns_kind],
    extra = [],
    constraints = [],
);
