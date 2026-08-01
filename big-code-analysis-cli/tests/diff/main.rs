//! `bca diff` / `bca diff-baseline` driver.
//!
//! Grouped into one binary by #1124 — see
//! `big-code-analysis-cli/tests/check/main.rs` for the rationale.

#[path = "../common/mod.rs"]
mod common;

mod diff_baseline;
mod diff_since;
