//! "Did you mean?" suggester for the CLI's unknown-name errors: the
//! `--threshold` / `[thresholds]` key, the `[thresholds.lang]` slug and
//! the `diff --metric` / `metrics --metrics` name.
//!
//! The algorithm lives in the library since #1614, so the Python
//! bindings' `metrics=` error suggests exactly what `bca metrics
//! --metrics` does; its unit tests moved with it. This module keeps the
//! CLI's call sites on one import path. End-to-end behaviour is still
//! exercised here, by `tests/check/check_thresholds.rs` and the
//! `build_unknown_metric_error_*` tests in `thresholds_tests.rs`.

pub(crate) use big_code_analysis::format_suggestion;
