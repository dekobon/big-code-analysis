//! `Cyclomatic` implementation for C.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Every decision is a row of `roles/c.rs`, read at the projection the
// metric counts (#1694, #1695, #1697): `&&` / `||` from `ShortCircuit`
// at the token; the `conditional_expression` from `Ternary` at its
// wrapper; the `if` / `for` / `while` *keyword tokens* from `Branch` /
// `Loop` (#284: the `while` of a `do … while` fires once, and the
// statement nodes would double-count); the `case` label from `CaseArm`
// at the token, its row excluding `default`; and the `switch_statement`
// from `Switch` at the owner, the modified tier's container. C has no
// exceptions, so `Catch` is absent and not read.
impl_cyclomatic_rows!(
    CCode,
    C,
    roles = crate::roles::c,
    both = [
        short_circuit::pays_at_leaf_kind,
        ternary::owns_kind,
        branch::pays_at_leaf_kind,
        r#loop::pays_at_leaf_kind,
    ],
    standard = [case_arm::pays_at_leaf_kind],
    modified = [switch::owns_kind],
    extra = [],
    constraints = [],
);
