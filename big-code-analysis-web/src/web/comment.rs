use serde::{Deserialize, Serialize};

use big_code_analysis::{Callback, ParserTrait, rm_comments};

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
    /// If `None`, no comments were found in the source code.
    pub code: Option<Vec<u8>>,
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

/// Unit structure to implement the `Callback` trait.
#[derive(Debug)]
pub struct WebCommentCallback;

impl Callback for WebCommentCallback {
    type Res = WebCommentResponse;
    type Cfg = WebCommentCfg;

    fn call<T: ParserTrait>(cfg: Self::Cfg, parser: &T) -> Self::Res {
        WebCommentResponse {
            id: cfg.id,
            language: cfg.language,
            code: rm_comments(parser),
        }
    }
}
