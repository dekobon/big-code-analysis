use serde::{Deserialize, Serialize};

use big_code_analysis::{Ast, LANG, MetricsError, Source};

/// Payload containing source code with comments to be removed.
///
/// Unknown fields are rejected with a `400` naming the offender (#633).
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebCommentPayload {
    /// Payload identifier.
    pub id: String,
    /// Source code filename.
    pub file_name: String,
    /// Source code with comments to be removed.
    pub code: String,
}

/// Internal comment-removal result carrying the stripped source as raw
/// bytes.
///
/// Both response variants are built from this: the octet-stream handler
/// streams [`WebCommentResponse::code`] back verbatim, while the JSON
/// handler converts it to a UTF-8 string via [`WebCommentJson`]
/// (issue #629). It is not serialized directly — the wire shapes are the
/// two response types below.
#[derive(Debug)]
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

/// JSON wire shape for the comment-removal response (issue #629).
///
/// The `/comment` JSON variant returns the stripped source as a JSON
/// **string**, matching the request's `code` string and every other
/// JSON endpoint. The octet-stream variant keeps raw bytes, which is the
/// correct home for binary-faithful round-trips. Before #629 the JSON
/// variant serialized `code` as `Vec<u8>`, which serde renders as an
/// array of byte numbers — a serde artifact, never a chosen shape.
#[derive(Debug, Serialize)]
pub struct WebCommentJson {
    /// Server response identifier.
    pub id: String,
    /// Source code programming language, as the canonical lowercase slug.
    pub language: String,
    /// Source code without comments, as a UTF-8 string. Empty when no
    /// comments were found (#558).
    pub code: String,
}

impl TryFrom<WebCommentResponse> for WebCommentJson {
    type Error = std::string::FromUtf8Error;

    /// Reinterpret the stripped bytes as a UTF-8 string for the JSON
    /// wire shape.
    ///
    /// In the JSON handler the request `code` arrives as a JSON string,
    /// hence valid UTF-8; comment removal only deletes byte ranges, so
    /// the stripped output remains valid UTF-8 and this conversion does
    /// not fail in practice. It is fallible rather than a lossy or
    /// `expect`-based cast so a hypothetical decode failure surfaces as a
    /// uniform `500` error body instead of corrupting the payload or
    /// panicking (#629).
    fn try_from(response: WebCommentResponse) -> Result<Self, Self::Error> {
        Ok(Self {
            id: response.id,
            language: response.language,
            code: String::from_utf8(response.code)?,
        })
    }
}

/// Source code information.
///
/// Unknown query parameters are rejected with a `400` naming the offender
/// (#633).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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
    code: Vec<u8>,
    cfg: WebCommentCfg,
) -> Result<WebCommentResponse, MetricsError> {
    // By value so the request buffer moves into the parser (no copy).
    let ast = Ast::parse(Source::from_bytes(language, code))?;
    Ok(WebCommentResponse {
        id: cfg.id,
        language: cfg.language,
        // `strip_comments` yields `None` when there is nothing to
        // strip; collapse that to an empty payload so the response
        // shape is uniform regardless of comment presence (#558).
        code: ast.strip_comments().unwrap_or_default(),
    })
}
