#![allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]

//! Format-validity helpers for the CLI integration suite.
//!
//! Submodule `validators` carries the same three helpers as
//! `tests/common/validators.rs` in the lib crate (validate_sarif,
//! assert_checkstyle_well_formed_and_structural, assert_html_well_formed).
//! Cargo `[dev-dependencies]` and shared modules do not propagate
//! across workspace members, so the duplication is unavoidable
//! without a separate test-helpers crate. Three small helpers don't
//! merit that indirection today.

#[allow(dead_code)]
pub mod validators;
