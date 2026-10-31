//! `Cyclomatic` implementation for Go.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Every decision is a row of `roles/go.rs`, read at the projection the
// metric counts (#1695, #1697). Go counts statement nodes, so `Branch`
// and `Loop` are plain rows read at the owner — every `for` spelling is
// one `for_statement`. `&&` / `||` from `ShortCircuit` at the token;
// each `case` clause of a switch or type switch, and each communication
// clause of a `select`, from `CaseArm` at the owner (the row leaves the
// `default` clauses out, #469); the three switch-like statements from
// `Switch`, the modified tier's container. Go has no ternary, no guard,
// no catch and no null-safe operator.
impl_cyclomatic_rows!(
    GoCode,
    Go,
    roles = crate::roles::go,
    both = [
        short_circuit::pays_at_leaf_kind,
        branch::pays_at_leaf_kind,
        r#loop::pays_at_leaf_kind,
    ],
    standard = [case_arm::owns_kind],
    modified = [switch::owns_kind],
    extra = [],
    constraints = [],
);
