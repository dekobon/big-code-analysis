//! Offender records consumed by CI/IDE output formats.
//!
//! [`OffenderRecord`] is the minimal shape every CI/IDE output format
//! (Checkstyle, SARIF, JUnit, etc.) renders. Producing offender records
//! from metric values vs. configured thresholds is the job of the
//! threshold engine (#96); this module only defines the data shape so
//! the format implementations can land independently.

#![allow(clippy::doc_markdown)]

use std::path::Path;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::output::numfmt::MessageMetric;

/// Tool identifier carried in the rule-id / source-prefix field of every
/// CI/IDE output format (Checkstyle `<error source="...">`, Clang/MSVC
/// warning rule prefix, SARIF `tool.driver.name`). Single source of
/// truth so a future tool rename is one edit, not three.
pub const TOOL_ID: &str = "big-code-analysis";

/// `path.to_str()`, or emit a stderr warning and return `None`. Used
/// by every output format that turns offender paths into UTF-8
/// identifiers (Checkstyle attribute, SARIF URI, warning-line column,
/// HTML / CSV cell). Centralizing the warning text keeps the
/// `format` label consistent across formats.
pub(crate) fn warn_non_utf8_path<'a>(format: &str, path: &'a Path) -> Option<&'a str> {
    if let Some(s) = path.to_str() {
        Some(s)
    } else {
        eprintln!(
            "Warning: skipping non-UTF-8 path in {format} output: {}",
            path.display()
        );
        None
    }
}

/// Severity of a metric-threshold violation.
///
/// Defaults to [`Severity::Warning`] so producers can opt into
/// `Error` explicitly for hard-fail gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Soft severity: report the violation but do not fail.
    #[default]
    Warning,
    /// Hard severity: report the violation and fail any gate keyed off it.
    Error,
}

impl Severity {
    /// Lowercase token used by Checkstyle XML and most CI integrations.
    #[must_use]
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
    /// with values formatted via `MessageMetric`: integer fast-path
    /// for safe integers, six-decimal rounding for non-integer finites,
    /// `"NaN"` / `"inf"` / `"-inf"` for non-finite values. The Display
    /// adapter writes directly into the format buffer, so this builds
    /// one `String` per call rather than three.
    #[must_use]
    pub fn default_message(&self) -> String {
        format!(
            "{} {} exceeds limit {}",
            self.metric,
            MessageMetric(self.value),
            MessageMetric(self.limit),
        )
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]
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
    fn default_message_renders_non_finite_values() {
        let mut r = OffenderRecord {
            path: PathBuf::from("a.rs"),
            function: None,
            start_line: 1,
            end_line: 1,
            start_col: None,
            metric: "halstead.volume".into(),
            value: f64::NAN,
            limit: 10.0,
            severity: Severity::Warning,
        };
        assert_eq!(r.default_message(), "halstead.volume NaN exceeds limit 10");

        r.value = f64::INFINITY;
        assert_eq!(r.default_message(), "halstead.volume inf exceeds limit 10");

        r.value = f64::NEG_INFINITY;
        assert_eq!(r.default_message(), "halstead.volume -inf exceeds limit 10");
    }
}
