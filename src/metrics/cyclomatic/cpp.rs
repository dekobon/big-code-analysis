//! `Cyclomatic` implementation for C++.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// C++ has only `&&` and `||` short-circuit operators.
// Grammar-specific loop kinds (`DoStatement`, `ForRangeLoop`) are NOT
// listed here because the `While` / `For` keyword-token arms above
// already fire inside them; adding the statement nodes would
// double-count (issue #284).
impl_cyclomatic_c_family!(CppCode, Cpp, ConditionalExpression, [AMPAMP, PIPEPIPE]);
