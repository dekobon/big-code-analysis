//! Build-time generator for the per-language token-kind enums.
//!
//! Every grammar in [`Lang`] is loaded through its tree-sitter crate,
//! its node kinds are enumerated, and the result is rendered through
//! one of the `templates/` files into a Rust, Go, or JSON module. The
//! Rust output is what lands in the parent crate's `src/languages/`;
//! `make enums-codegen-drift` fails when the checked-in files no longer
//! match what this crate emits.

// Production-only `unwrap()` ban. See `[workspace.lints.clippy]` in the
// root `Cargo.toml` for why this is a per-root attribute and not a
// Cargo lint (#1227). `enums` is excluded from that workspace, so it
// carries the attribute for the same reason and gets it separately.
#![cfg_attr(not(test), warn(clippy::unwrap_used))]

#[macro_use]
mod macros;

mod common;
pub use crate::common::*;

mod languages;
pub use crate::languages::*;

mod rust;
pub use crate::rust::*;

mod go;
pub use crate::go::*;

mod json;
pub use crate::json::*;
