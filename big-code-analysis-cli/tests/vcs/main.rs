//! Change-history driver: `bca vcs` ranking, `--vcs-jit` risk scoring,
//! and `--vcs-trend`.
//!
//! Grouped into one binary by #1124 — see
//! `big-code-analysis-cli/tests/check/main.rs` for the rationale. The
//! ranking module is `vcs_rank` rather than `vcs` only because a module
//! named for the driver that contains it is module inception.

#[path = "../common/mod.rs"]
mod common;

mod vcs_jit;
mod vcs_rank;
mod vcs_trend;
