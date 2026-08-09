//! `bca check` driver: the threshold engine, the baseline filter,
//! exclusions, exemptions, in-source suppression markers, the report
//! scope, the exit-code contract, and `--action` enforcement.
//!
//! Each module below was its own `tests/*.rs` crate root until #1124.
//! An integration binary here statically links the tree-sitter runtime
//! and every grammar, so thirty-seven of them made linking — not
//! compilation — the tail of every incremental `cargo test`. Grouping
//! by subcommand pays that link cost six times instead. Test bodies are
//! unchanged; the only per-module edit is `mod common;` becoming
//! `use crate::common;`.

#[path = "../common/mod.rs"]
mod common;

mod action_enforcement;
mod check_baseline;
mod check_exclude;
mod check_exit_codes;
mod check_explain_threshold;
mod check_report_suppressed_scope;
mod check_skipped_inputs;
mod check_stream_contract;
mod check_suppression;
mod check_thresholds;
mod exemptions;
