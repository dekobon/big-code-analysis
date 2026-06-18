//! `Npa` implementation for Mozjs.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Npa for MozjsCode {
    js_npa_compute!(Mozjs);
}
