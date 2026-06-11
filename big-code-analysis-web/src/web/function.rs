use serde::{Deserialize, Serialize};
use serde_json::{self, Value};

use big_code_analysis::{Ast, FunctionSpan, LANG, MetricsError, Source};

/// Payload containing source code with function spans to be retrieved.
///
/// Unknown fields are rejected with a `400` naming the offender (#633).
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebFunctionPayload {
    /// Payload identifier.
    pub id: String,
    /// Source code filename.
    pub file_name: String,
    /// Source code with function spans to be retrieved.
    pub code: String,
}

/// Server response containing function spans for the requested source code.
#[derive(Debug, Serialize)]
pub struct WebFunctionResponse {
    /// Server response identifier.
    pub id: String,
    /// Source code programming language, as the canonical lowercase
    /// slug (#540 / #541), reporting which grammar was selected.
    pub language: String,
    /// Function spans for the requested source code.
    pub spans: Vec<FunctionSpan>,
}

/// Source code information.
///
/// Unknown query parameters are rejected with a `400` naming the offender
/// (#633).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebFunctionInfo {
    /// Source code filename.
    pub file_name: String,
}

/// Server request configuration.
#[derive(Debug)]
pub struct WebFunctionCfg {
    /// Request identifier.
    pub id: String,
    /// Canonical language slug to echo back in the response envelope.
    pub language: String,
}

/// Retrieve function spans for `code` in `language` under `cfg`,
/// serialized to a JSON [`Value`].
///
/// # Errors
///
/// Returns [`MetricsError::LanguageDisabled`] when `language`'s feature
/// is disabled — impossible in the feature-pinned server build.
///
/// # Panics
///
/// Does not panic in practice: the only `expect` guards the static,
/// infallible [`WebFunctionResponse`] `Serialize` impl.
pub fn function_spans(
    language: LANG,
    code: Vec<u8>,
    cfg: WebFunctionCfg,
) -> Result<Value, MetricsError> {
    // By value so the request buffer moves into the parser (no copy).
    let ast = Ast::parse(Source::from_bytes(language, code))?;
    let spans = ast.functions();
    Ok(serde_json::to_value(WebFunctionResponse {
        id: cfg.id,
        language: cfg.language,
        spans,
    })
    .expect("WebFunctionResponse has a static, infallible Serialize impl"))
}
