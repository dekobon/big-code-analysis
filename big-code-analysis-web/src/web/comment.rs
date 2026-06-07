use serde::{Deserialize, Serialize};

use big_code_analysis::{Ast, LANG, MetricsError, Source};

/// Payload containing source code with comments to be removed.
#[derive(Debug, Deserialize, Serialize)]
pub struct WebCommentPayload {
    /// Payload identifier.
    pub id: String,
    /// Source code filename.
    pub file_name: String,
    /// Source code with comments to be removed.
    pub code: String,
}

/// Server response containing the source code without comments.
#[derive(Debug, Serialize)]
pub struct WebCommentResponse {
    /// Server response identifier.
    pub id: String,
    /// Source code programming language, as the canonical lowercase
    /// slug (#540 / #541), reporting which grammar was selected.
    pub language: String,
    /// Source code without comments.
    ///
    /// When no comments were found, this is the empty byte sequence so
    /// the "no comments" outcome shares the success envelope of a
    /// non-empty result (`200` with an empty payload) across both the
    /// JSON and octet-stream variants — see issue #558.
    pub code: Vec<u8>,
}

/// Source code information.
#[derive(Debug, Deserialize)]
pub struct WebCommentInfo {
    /// Source code filename.
    pub file_name: String,
}

/// Server request configuration.
#[derive(Debug)]
pub struct WebCommentCfg {
    /// Request identifier.
    pub id: String,
    /// Canonical language slug to echo back in the response envelope.
    pub language: String,
}

/// Strip comments from `code` in `language` under `cfg`.
///
/// # Errors
///
/// Returns [`MetricsError::LanguageDisabled`] when `language`'s feature
/// is disabled — impossible in the feature-pinned server build.
pub fn strip_comments(
    language: LANG,
    code: &[u8],
    cfg: WebCommentCfg,
) -> Result<WebCommentResponse, MetricsError> {
    let ast = Ast::parse(Source::new(language, code))?;
    Ok(WebCommentResponse {
        id: cfg.id,
        language: cfg.language,
        // `strip_comments` yields `None` when there is nothing to
        // strip; collapse that to an empty payload so the response
        // shape is uniform regardless of comment presence (#558).
        code: ast.strip_comments().unwrap_or_default(),
    })
}
