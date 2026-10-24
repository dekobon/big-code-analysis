//! `Cyclomatic` implementation for Mozilla C++.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// The `ShortCircuit` row of `roles/mozcpp.rs` (#1694): `and` / `or`
// are the ISO alternative spellings of `&&` / `||`, the tokens count
// only where applied, and a requires clause scores nothing; the
// derivations are on the Cpp twin (#1522, #1525, #1533).
impl_cyclomatic_c_family_rows!(
    MozcppCode,
    Mozcpp,
    short_circuit = crate::roles::mozcpp::short_circuit,
    ternary = crate::roles::mozcpp::ternary,
    constraints = [RequiresClause, RequiresExpression],
);
