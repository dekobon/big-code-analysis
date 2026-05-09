//! Shared fixture builders for integration tests.
//!
//! These helpers eliminate the per-test struct-literal repetition
//! that grew up around `OffenderRecord` and `FuncSpace` while the
//! output-format tests were being written. Used by:
//!
//! - `tests/sarif_test.rs`
//! - `tests/checkstyle_test.rs`
//! - `tests/html_test.rs`
//!
//! Per-`mod tests` blocks inside `src/output/*.rs` carry their own
//! near-identical builders. Those are intentionally not shared
//! because Cargo treats `mod tests` as private to the crate; an
//! integration-test helper module is unreachable from there.

use std::path::PathBuf;

use big_code_analysis::{CodeMetrics, FuncSpace, OffenderRecord, Severity, SpaceKind};

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

/// Build a `FuncSpace` with no children and default-initialized
/// metrics. Useful for tests that exercise writer behavior on a
/// known shape without needing a real parse tree.
pub fn empty_space(name: &str, kind: SpaceKind, start: usize, end: usize) -> FuncSpace {
    FuncSpace {
        name: Some(name.into()),
        name_was_lossy: false,
        start_line: start,
        end_line: end,
        kind,
        spaces: Vec::new(),
        metrics: CodeMetrics::default(),
    }
}
