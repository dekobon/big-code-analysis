use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use big_code_analysis::{Ast, FuncSpace, LANG, MetricsError, MetricsOptions, Source};

/// Selects how much of the metric space tree `/metrics` returns (#638).
///
/// Replaces the former boolean `unit` request flag, which leaked the
/// `FuncSpace` internal term "unit" into the public contract. `Full`
/// returns the complete nested space tree; `File` returns only the
/// file-level root with its children cleared. Defaults to [`Scope::Full`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Return the full nested space tree (the default).
    #[default]
    Full,
    /// Return only the file-level root space (no nested children).
    File,
}

impl Scope {
    /// Whether only the file-level root should be reported (the children
    /// `spaces` list is cleared). True for [`Scope::File`].
    #[must_use]
    pub fn is_file_only(self) -> bool {
        matches!(self, Scope::File)
    }
}

/// Payload containing source code used to compute metrics.
///
/// Unknown fields are rejected with a `400` naming the offending key
/// (#633): a typo'd field cannot silently change request semantics. The
/// field selecting file-level-only output is `scope` (#638), not the
/// removed boolean `unit`; sending the old `unit` key now 400s as an
/// unknown field.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebMetricsPayload {
    /// Payload identifier. Optional on the wire (#645): an omitted `id`
    /// defaults to the empty string, the "no correlation id" sentinel
    /// every downstream surface already honours.
    #[serde(default)]
    pub id: String,
    /// Source code filename.
    pub file_name: String,
    /// Source code used to compute metrics.
    pub code: String,
    /// How much of the space tree to return: `full` (default) or `file`
    /// (file-level root only). Replaces the pre-2.0 boolean `unit` (#638).
    #[serde(default)]
    pub scope: Scope,
}

/// Server response containing the metric space tree for the requested
/// source code.
#[derive(Debug, Serialize)]
pub struct WebMetricsResponse {
    /// Server response identifier.
    pub id: String,
    /// Source code programming language.
    pub language: String,
    /// The root of the metric space tree. Its own nested `spaces` list
    /// holds the children. Named `root` (not the misleading plural
    /// `spaces`) since it is a single object, and accurate when the web
    /// analyzes in-memory source rather than a file path (#638).
    pub root: FuncSpace,
}

/// Source code information.
///
/// Unknown query parameters are rejected with a `400` naming the offender
/// (#633).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebMetricsInfo {
    /// Source code filename.
    pub file_name: String,
    /// How much of the space tree to return: `full` (default) or `file`.
    /// Replaces the pre-2.0 boolean `unit` query flag (#638). The server
    /// parses this into a [`Scope`]; an unrecognised value is a `400`.
    pub scope: Option<String>,
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
    /// How much of the space tree to report (#638).
    pub scope: Scope,
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
    pub fn new(id: String, path: PathBuf, scope: Scope, language: String) -> Self {
        Self {
            id,
            path,
            scope,
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
/// `200`-with-`root: null` body. The display name is the request
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
            if cfg.scope.is_file_only() {
                s.spaces.clear();
            }
            WebMetricsResponse {
                id: cfg.id,
                language: cfg.language,
                root: s,
            }
        }))
}
