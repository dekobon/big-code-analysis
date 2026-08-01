//! Per-grammar metric driver: suites that pin one tree-sitter grammar's
//! parse trees against the metric impls written for it, plus the
//! alterator's string-flattening rules.
//!
//! Grouped into one binary by #1124 — see `tests/api/main.rs`.

mod alterator_string_flattening;
mod c_grammar_metrics;
mod mozcpp_grammar_metrics;
