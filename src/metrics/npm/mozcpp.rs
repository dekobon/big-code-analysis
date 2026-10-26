//! `Npm` implementation for mozcpp.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::cpp::cpp_family_npm;
use super::*;

// The Cpp twin's walk against this dialect's rows and ids.
cpp_family_npm!(MozcppCode, Mozcpp, crate::roles::mozcpp::func_opener);
