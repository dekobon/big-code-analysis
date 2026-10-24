//! `Cognitive` implementation for mozcpp.
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

use super::cpp::cpp_family_cognitive;
use super::*;

// The Cpp twin's walk against this dialect's rows and ids (#1695).
cpp_family_cognitive!(MozcppCode, Mozcpp, crate::roles::mozcpp);
