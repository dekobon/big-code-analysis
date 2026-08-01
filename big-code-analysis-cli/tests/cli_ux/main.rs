//! Command-surface driver: the smoke tests over every subcommand, help
//! text, flag scoping, deprecated aliases, `bca init`, `bca
//! list-metrics`, and `bca.toml` manifest discovery and merging.
//!
//! Grouped into one binary by #1124 — see
//! `big-code-analysis-cli/tests/check/main.rs` for the rationale.

#[path = "../common/mod.rs"]
mod common;

mod cli_smoke;
mod deprecated_aliases;
mod flag_scoping;
mod help_text;
mod init;
mod list_metrics;
mod manifest;
