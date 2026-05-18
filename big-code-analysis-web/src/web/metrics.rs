use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use std::path::PathBuf;

#[allow(deprecated)]
use big_code_analysis::metrics_with_options;
use big_code_analysis::{Callback, FuncSpace, MetricsOptions, ParserTrait};

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
    ///
    /// If `None`, an error occurred processing the request.
    pub spaces: Option<FuncSpace>,
}

/// Source code information.
#[derive(Debug, Deserialize)]
pub struct WebMetricsInfo {
    /// Source code filename.
    pub file_name: String,
    /// Flag to consider only unit space metrics.
    ///
    /// Accepted truthy values: `"1"`, `"true"`, `"yes"`, `"on"` (case-insensitive).
    /// If `None` or a non-truthy value, the entire code is considered.
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

/// Unit structure to implement the `Callback` trait.
pub struct WebMetricsCallback;

impl Callback for WebMetricsCallback {
    type Res = Value;
    type Cfg = WebMetricsCfg;

    fn call<T: ParserTrait>(cfg: Self::Cfg, parser: &T) -> Self::Res {
        // The REST schema carries `spaces: Option<FuncSpace>` — keeping
        // it `Option` is explicitly out of scope of #253 (parallels the
        // `AstResponse.root` decision). Collapse `MetricsError` into
        // `None` here so the REST wire format is unchanged.
        // The web crate has a `ParserTrait` in hand (driven by the
        // shared `action` callback dispatch), not raw bytes, so the
        // path-positional `metrics_with_options` shim is still the
        // right seam here. The deprecation points callers at
        // `analyze(Source { ... }, ...)`; the parser-trait flavour
        // remains until issue #256 reshapes the parser surface.
        #[allow(deprecated)]
        let spaces = metrics_with_options(
            parser,
            &cfg.path,
            MetricsOptions::default().with_exclude_tests(cfg.exclude_tests),
        )
        .ok()
        .map(|mut s| {
            if cfg.unit {
                s.spaces.clear();
            }
            s
        });

        serde_json::to_value(WebMetricsResponse {
            id: cfg.id,
            language: cfg.language,
            spaces,
        })
        .expect("WebMetricsResponse has a static, infallible Serialize impl")
    }
}
