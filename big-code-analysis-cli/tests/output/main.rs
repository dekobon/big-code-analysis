//! Output-rendering driver: every emitted format (JSON/YAML/TOML/CBOR,
//! Markdown, HTML), the shared metric-selection and header plumbing,
//! and colour handling.
//!
//! Grouped into one binary by #1124 — see
//! `big-code-analysis-cli/tests/check/main.rs` for the rationale.

#[path = "../common/mod.rs"]
mod common;

mod color_output;
mod dump_headers;
mod format_smoke;
mod html_report;
mod markdown_format;
mod metric_selection;
mod output_unification;
