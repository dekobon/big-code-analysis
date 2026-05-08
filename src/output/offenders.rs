//! Offender records consumed by CI/IDE output formats.
//!
//! [`OffenderRecord`] is the minimal shape every CI/IDE output format
//! (Checkstyle, SARIF, JUnit, etc.) renders. Producing offender records
//! from metric values vs. configured thresholds is the job of the
//! threshold engine (#96); this module only defines the data shape so
//! the format implementations can land independently.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Severity of a metric-threshold violation.
///
/// Defaults to [`Severity::Warning`] so producers can opt into
/// `Error` explicitly for hard-fail gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    #[default]
    Warning,
    Error,
}

impl Severity {
    /// Lowercase token used by Checkstyle XML and most CI integrations.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// One metric-threshold violation, language-agnostic and format-agnostic.
///
/// Paths are stored as [`PathBuf`] so output writers can decide how to
/// surface non-UTF-8 components (skip, replace, or fail) rather than
/// silently lossy-converting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OffenderRecord {
    /// Source file the violation was reported against.
    pub path: PathBuf,
    /// Function or method name; `None` for file-level violations.
    pub function: Option<String>,
    /// First line covered by the violation (1-based).
    pub start_line: u32,
    /// Last line covered by the violation (1-based, inclusive).
    pub end_line: u32,
    /// Optional starting column (1-based).
    pub start_col: Option<u32>,
    /// Metric identifier, e.g. `"cyclomatic"`, `"loc.lloc"`,
    /// `"halstead.volume"`.
    pub metric: String,
    /// Observed metric value.
    pub value: f64,
    /// Configured threshold the value exceeded.
    pub limit: f64,
    /// Severity assigned by the threshold engine.
    pub severity: Severity,
}

impl OffenderRecord {
    /// Default human-readable message used by formats that do not carry
    /// their own templating. `"<metric> <value> exceeds limit <limit>"`,
    /// with values trimmed to a sensible precision.
    pub fn default_message(&self) -> String {
        format!(
            "{} {} exceeds limit {}",
            self.metric,
            format_metric_number(self.value),
            format_metric_number(self.limit),
        )
    }
}

/// Render a metric number with at most six decimals, trimming trailing
/// zeros so integral values render as `15` rather than `15.000000`.
fn format_metric_number(n: f64) -> String {
    if n.is_finite() && n == n.trunc() && n.abs() < 1e16 {
        format!("{}", n as i64)
    } else {
        let mut s = format!("{n:.6}");
        if s.contains('.') {
            while s.ends_with('0') {
                s.pop();
            }
            if s.ends_with('.') {
                s.pop();
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_default_is_warning() {
        assert_eq!(Severity::default(), Severity::Warning);
    }

    #[test]
    fn severity_as_str_lowercase() {
        assert_eq!(Severity::Warning.as_str(), "warning");
        assert_eq!(Severity::Error.as_str(), "error");
    }

    #[test]
    fn default_message_renders_integral_value() {
        let r = OffenderRecord {
            path: PathBuf::from("a.rs"),
            function: Some("f".into()),
            start_line: 1,
            end_line: 2,
            start_col: None,
            metric: "cyclomatic".into(),
            value: 17.0,
            limit: 15.0,
            severity: Severity::Warning,
        };
        assert_eq!(r.default_message(), "cyclomatic 17 exceeds limit 15");
    }

    #[test]
    fn default_message_renders_fractional_value() {
        let r = OffenderRecord {
            path: PathBuf::from("a.rs"),
            function: None,
            start_line: 1,
            end_line: 1,
            start_col: None,
            metric: "halstead.volume".into(),
            value: 12.5,
            limit: 10.0,
            severity: Severity::Error,
        };
        assert_eq!(r.default_message(), "halstead.volume 12.5 exceeds limit 10");
    }

    #[test]
    fn format_metric_number_handles_non_finite() {
        // NaN / infinity are .trunc() == self for inf but != self for NaN;
        // the predicate `n == n.trunc()` is false for NaN, so we fall into
        // the formatted path. Inf hits trunc but is_finite is false.
        assert_eq!(format_metric_number(f64::NAN), "NaN");
        assert_eq!(format_metric_number(f64::INFINITY), "inf");
    }
}
