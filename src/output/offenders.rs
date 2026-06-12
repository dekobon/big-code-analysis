// bca: suppress-file(halstead)
// `OffenderRecord` shape + builders; file-level halstead is a many-fn
// aggregation artifact, not per-function logic complexity.

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

use crate::metric_catalog::{Direction, lookup};
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
///
/// # Ordering contract
///
/// `Severity` is an ordered scale: `Error > Warning`. The derived
/// [`Ord`]/[`PartialOrd`] follow declaration order, so variants are
/// declared least-severe-first (`Warning` then `Error`) to make the
/// derived comparison match the intended severity ranking. Callers can
/// rely on this to pick the worst severity in a set
/// (`severities.iter().max()`) or to gate on `>= Severity::Error`. Any
/// future tier (`Info`/`Note`) must be inserted in the correct severity
/// position to preserve this scale.
// An open severity scale: a future `Info` / `Note` tier (or any tier
// between `Warning` and `Error`) lands as an additive variant rather
// than a 2.0 break, so it carries `#[non_exhaustive]`. Variants stay in
// severity order because the derived `Ord` (added in #552) is keyed on
// declaration order.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
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
    /// their own templating. Renders `"<metric> <value> exceeds limit
    /// <limit>"` for higher-is-worse metrics and `"<metric> <value>
    /// falls below limit <limit>"` for the lower-is-worse `mi.*` family
    /// (#698) — the breach phrasing must match the direction the
    /// [`metric_catalog`](crate::metric_catalog) records, or an MI
    /// offender (value *below* the limit) reads as "exceeds" in
    /// Checkstyle / Clang / MSVC / SARIF output. An unknown metric id
    /// falls back to the higher-is-worse phrasing.
    ///
    /// Values are formatted via `MessageMetric`: integer fast-path for
    /// safe integers, six-decimal rounding for non-integer finites,
    /// `"NaN"` / `"inf"` / `"-inf"` for non-finite values. The Display
    /// adapter writes directly into the format buffer, so this builds
    /// one `String` per call rather than three.
    #[must_use]
    pub fn default_message(&self) -> String {
        format!(
            "{} {} {} {}",
            self.metric,
            MessageMetric(self.value),
            self.breach_phrase(),
            MessageMetric(self.limit),
        )
    }

    /// The direction-appropriate breach phrase for this offender's
    /// metric: `"exceeds limit"` when a higher value is worse,
    /// `"falls below limit"` for the lower-is-worse `mi.*` family.
    /// Unknown metric ids default to `"exceeds limit"` (the common
    /// case), matching the unknown-id fallback the SARIF / Code Climate
    /// descriptions already use.
    fn breach_phrase(&self) -> &'static str {
        match lookup(&self.metric).map(|i| i.direction) {
            Some(Direction::LowerIsWorse) => "falls below limit",
            _ => "exceeds limit",
        }
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

    /// `Severity` is `#[non_exhaustive]` (#551). The attribute is a
    /// compile-time forward-compat contract and must not change the
    /// serialized form: each variant still round-trips through its
    /// lowercase token.
    #[test]
    fn severity_non_exhaustive_serde_roundtrip_unchanged() {
        for (variant, token) in [
            (Severity::Warning, "\"warning\""),
            (Severity::Error, "\"error\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, token);
            let back: Severity = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
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

    #[test]
    fn default_message_lower_is_worse_metric_falls_below() {
        // The `mi.*` Maintainability Index family is lower-is-worse: an
        // offender's value is *below* the limit, so the message must read
        // "falls below limit", not "exceeds limit" (#698). A pre-fix
        // build hardcoded "exceeds limit" for every metric, producing the
        // nonsensical "mi.original 30 exceeds limit 50" for a value that
        // is below 50.
        let r = OffenderRecord {
            path: PathBuf::from("a.rs"),
            function: Some("f".into()),
            start_line: 1,
            end_line: 2,
            start_col: None,
            metric: "mi.original".into(),
            value: 30.0,
            limit: 50.0,
            severity: Severity::Warning,
        };
        assert_eq!(r.default_message(), "mi.original 30 falls below limit 50");
    }

    #[test]
    fn default_message_higher_is_worse_metric_still_exceeds() {
        // The direction lookup must keep the higher-is-worse phrasing for
        // every non-`mi` metric. Guards against an over-broad fix that
        // flipped the wording for the common case.
        let r = OffenderRecord {
            path: PathBuf::from("a.rs"),
            function: Some("f".into()),
            start_line: 1,
            end_line: 2,
            start_col: None,
            metric: "cognitive".into(),
            value: 25.0,
            limit: 20.0,
            severity: Severity::Error,
        };
        assert_eq!(r.default_message(), "cognitive 25 exceeds limit 20");
    }

    #[test]
    fn default_message_unknown_metric_defaults_to_exceeds() {
        // An id the catalog does not know falls back to the
        // higher-is-worse "exceeds limit" phrasing.
        let r = OffenderRecord {
            path: PathBuf::from("a.rs"),
            function: None,
            start_line: 1,
            end_line: 1,
            start_col: None,
            metric: "made.up.metric".into(),
            value: 5.0,
            limit: 1.0,
            severity: Severity::Warning,
        };
        assert_eq!(r.default_message(), "made.up.metric 5 exceeds limit 1");
    }
}
