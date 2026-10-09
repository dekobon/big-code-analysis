//! `Cyclomatic` implementation for Mozilla C++.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// `and` / `or` are the ISO alternative spellings of `&&` / `||`; the
// derivation is on the Cpp twin (#1522).
impl_cyclomatic_c_family!(
    MozcppCode,
    Mozcpp,
    ConditionalExpression,
    [AMPAMP, PIPEPIPE, And, Or]
);
