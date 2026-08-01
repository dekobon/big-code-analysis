//! Shared fixture builders for integration tests.
//!
//! These helpers eliminate the per-test struct-literal repetition
//! that grew up around `OffenderRecord` while the output-format tests
//! were being written. Used by:
//!
//! - `tests/output_formats/checkstyle_test.rs` (`rec`)
//!
//! Per-`mod tests` blocks inside `src/output/*.rs` carry their own
//! near-identical builders. Those are intentionally not shared
//! because Cargo treats `mod tests` as private to the crate; an
//! integration-test helper module is unreachable from there.

use std::path::PathBuf;

use big_code_analysis::{OffenderRecord, Severity};

/// Build a default offender record. The numeric fields use
/// values chosen to be obvious in test output (`line=42`, `col=5`,
/// `function=Some("f")`); tests that care about specific values can
/// mutate the returned record before passing it to a writer. Severity
/// defaults to `Warning`; tests for the `Error` case mutate
/// `r.severity = Severity::Error`.
pub fn rec(path: &str, metric: &str, value: f64, limit: f64) -> OffenderRecord {
    OffenderRecord {
        path: PathBuf::from(path),
        function: Some("f".into()),
        start_line: 42,
        end_line: 50,
        start_col: Some(5),
        metric: metric.into(),
        value,
        limit,
        severity: Severity::Warning,
    }
}
