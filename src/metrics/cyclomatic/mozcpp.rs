//! `Cyclomatic` implementation for Mozilla C++.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// `and` / `or` are the ISO alternative spellings of `&&` / `||`, and
// the tokens count only where applied, and a requires clause scores
// nothing; the derivations are on the Cpp twin (#1522, #1525, #1533).
impl_cyclomatic_c_family!(
    MozcppCode,
    Mozcpp,
    ConditionalExpression,
    [AMPAMP, PIPEPIPE, And, Or],
    applied_if = cpp_operator_is_applied,
    constraints = [RequiresClause, RequiresExpression],
);
