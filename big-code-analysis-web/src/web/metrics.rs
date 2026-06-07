// bca: suppress-file(halstead, nargs)
// Metrics HTTP handler glue; file-level halstead and summed nargs are many-fn
// aggregation artifacts, not per-function logic complexity.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use big_code_analysis::{Ast, FuncSpace, LANG, MetricsError, MetricsOptions, Source};

/// Payload containing source code used to compute metrics.
#[derive(Debug, Deserialize, Serialize)]
pub struct WebMetricsPayload {
    /// Payload identifier.
    pub id: String,
    /// Source code filename.
    pub file_name: String,
    /// Source code used to compute metrics.
    pub code: String,
    /// Flag to consider only unit space metrics.
    pub unit: bool,
}

/// Server response containing metrics for every space present in
/// the requested source code.
#[derive(Debug, Serialize)]
pub struct WebMetricsResponse {
    /// Server response identifier.
    pub id: String,
    /// Source code programming language.
    pub language: String,
    /// Metrics for every space contained in the requested source code.
    pub spaces: FuncSpace,
}

/// Source code information.
#[derive(Debug, Deserialize)]
pub struct WebMetricsInfo {
    /// Source code filename.
    pub file_name: String,
    /// Flag to consider only unit space metrics.
    ///
    /// Normal boolean semantics (#541): accepts `true`/`false` and
    /// `1`/`0`, case-insensitively. If `None`, defaults to `false` (the
    /// entire code is considered). Any other value is rejected with an
    /// HTTP 400 carrying the uniform JSON error body.
    pub unit: Option<String>,
}

/// Server request configuration.
///
/// Marked `#[non_exhaustive]` so future config fields can land
/// additively. Downstream embedders construct via
/// [`WebMetricsCfg::new`] plus the `with_*` builder setters rather
/// than struct-literal syntax (rustc E0639).
#[derive(Debug)]
#[non_exhaustive]
pub struct WebMetricsCfg {
    /// Request identifier.
    pub id: String,
    /// Path to the source file.
    pub path: PathBuf,
    /// Flag to consider only unit space metrics.
    pub unit: bool,
    /// Source code programming language.
    pub language: String,
    /// When true, skip language-specific test subtrees (currently
    /// Rust `#[test]` / `#[cfg(test)]`). Defaults to false at every
    /// call site so the REST API keeps emitting the same numbers it
    /// did before #182. Toggle via
    /// [`WebMetricsCfg::with_exclude_tests`].
    pub exclude_tests: bool,
}

impl WebMetricsCfg {
    /// Build a server-request config with `exclude_tests` defaulted
    /// to `false` (pre-#182 behaviour, every node counted). Chain
    /// [`WebMetricsCfg::with_exclude_tests`] to toggle.
    #[must_use]
    pub fn new(id: String, path: PathBuf, unit: bool, language: String) -> Self {
        Self {
            id,
            path,
            unit,
            language,
            exclude_tests: false,
        }
    }

    /// Builder-style setter for [`WebMetricsCfg::exclude_tests`].
    #[must_use]
    pub fn with_exclude_tests(mut self, exclude_tests: bool) -> Self {
        self.exclude_tests = exclude_tests;
        self
    }
}

/// Compute metrics for `code` in `language` under `cfg`.
///
/// `Ok(None)` signals that metric computation failed; the HTTP handler
/// maps it to a `5xx` status (issue #517) rather than the former
/// `200`-with-`spaces: null` body. The display name is the request
/// `file_name` (always valid UTF-8 from the JSON body).
///
/// # Errors
///
/// Returns [`MetricsError::LanguageDisabled`] when `language`'s feature
/// is disabled — impossible in the feature-pinned server build.
pub fn compute_metrics(
    language: LANG,
    code: Vec<u8>,
    cfg: WebMetricsCfg,
) -> Result<Option<WebMetricsResponse>, MetricsError> {
    // Take `code` by value so the request buffer moves into the parser
    // without a copy (matches the CLI's `Source::from_bytes` hot path).
    let ast = Ast::parse(
        Source::from_bytes(language, code).with_name(cfg.path.to_str().map(str::to_owned)),
    )?;
    Ok(ast
        .metrics(MetricsOptions::default().with_exclude_tests(cfg.exclude_tests))
        .ok()
        .map(|mut s| {
            if cfg.unit {
                s.spaces.clear();
            }
            WebMetricsResponse {
                id: cfg.id,
                language: cfg.language,
                spaces: s,
            }
        }))
}
