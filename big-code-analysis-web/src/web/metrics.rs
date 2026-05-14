use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use std::path::PathBuf;

use big_code_analysis::{Callback, FuncSpace, MetricsOptions, ParserTrait, metrics_with_options};

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
#[derive(Debug)]
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
    /// did before #182.
    pub exclude_tests: bool,
}

/// Unit structure to implement the `Callback` trait.
pub struct WebMetricsCallback;

impl Callback for WebMetricsCallback {
    type Res = Value;
    type Cfg = WebMetricsCfg;

    fn call<T: ParserTrait>(cfg: Self::Cfg, parser: &T) -> Self::Res {
        let spaces = metrics_with_options(
            parser,
            &cfg.path,
            MetricsOptions {
                exclude_tests: cfg.exclude_tests,
            },
        );
        let spaces = if cfg.unit {
            if let Some(mut spaces) = spaces {
                spaces.spaces.clear();
                Some(spaces)
            } else {
                None
            }
        } else {
            spaces
        };

        serde_json::to_value(WebMetricsResponse {
            id: cfg.id,
            language: cfg.language,
            spaces,
        })
        .expect("WebMetricsResponse has a static, infallible Serialize impl")
    }
}
