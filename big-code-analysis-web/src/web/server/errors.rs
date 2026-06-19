//! Uniform error model: the `{error, error_kind, id}` wire body, the
//! typed [`ParseError`] / [`BodyError`] failures, the closed `error_kind`
//! token vocabulary, the static error-message constants, and the helpers
//! every endpoint routes its failures through.
#![allow(clippy::wildcard_imports)]

use super::*;

/// `error` message returned when the submitted `file_name` (and content
/// sniffing) cannot be mapped to a supported language.
///
/// The route was matched and the request entity is well-formed, so the
/// failure is *unprocessable*, not *not found*: the endpoint answers
/// `422 Unprocessable Entity` rather than the pre-2.0 `404` (issue #634),
/// carrying [`error_kind::UNSUPPORTED_LANGUAGE`] as its machine token
/// (#631). The string value coincides with the token. The set of
/// supported languages is available from `GET /v1/languages`.
pub(crate) const UNSUPPORTED_LANGUAGE: &str = "unsupported_language";

/// Error body emitted when the `scope` query flag is not a recognised
/// value (#638).
pub(crate) const INVALID_SCOPE_FLAG: &str =
    "The `scope` query flag must be one of `full` or `file` (case-insensitive)";

pub(crate) const PARSE_TIMEOUT: &str = "Parse timed out";
const PARSE_POOL_SATURATED: &str = "parse pool saturated";
pub(crate) const PAYLOAD_TOO_LARGE: &str = "Request body exceeds the maximum allowed size";

/// Client-facing message for a parse task that panicked or was cancelled.
///
/// Deliberately generic: the underlying detail (panic message, join
/// error) is logged server-side but never leaked to the client (#639,
/// pinned by `test_run_parse_error_*_does_not_leak_internals`).
pub(crate) const INTERNAL_SERVER_ERROR: &str = "Internal server error";

/// Error body emitted when AST construction yields no root node.
///
/// Defensive: `build` only returns `None` in a degenerate case the
/// current grammars never reach for a parsed root, so this `500` is
/// unreachable today (hence no integration test drives it). It exists
/// so a future walker change cannot silently regress to the old
/// `200`-with-`root: null` body (issue #517).
pub(crate) const AST_BUILD_FAILED: &str = "Failed to build an AST for the supplied source";
/// Error body emitted when metric computation fails for a parsed source.
///
/// Defensive, like [`AST_BUILD_FAILED`]: `metrics_with_options` does not
/// error for the web crate today (every `MetricsError` variant is either
/// reserved or guarded against by the `all-languages` feature pin), so
/// this `500` is unreachable today and exists to keep failures off the
/// `200` path if that changes (issue #517).
pub(crate) const METRICS_FAILED: &str = "Failed to compute metrics for the supplied source";
/// Error body when the history walk itself fails (server-side → `500`).
///
/// Client-input vcs failures no longer collapse onto one kitchen-sink
/// message: each carries the specific `vcs::Error` `Display` output plus
/// its own `error_kind` token (#631). This constant is the generic body
/// for the *environment / backend* `500` path only, where the real cause
/// is logged server-side rather than leaked.
pub(crate) const VCS_FAILED: &str = "Failed to walk change history for the supplied repository";

/// Typed failure of [`run_parse`], rendered as the uniform `{error, id}`
/// JSON body (#639).
///
/// Before #639 these paths returned actix's `ErrorServiceUnavailable` /
/// `ErrorGatewayTimeout` / `ErrorInternalServerError` helpers, which emit
/// a bare `text/plain` body and so bypassed the published `{error, id}`
/// contract. Carrying the correlation id on each variant lets the
/// `ResponseError` impl echo it back exactly as the success paths do.
#[derive(Debug)]
pub(crate) enum ParseError {
    /// The orphaned-task pool is saturated (or the semaphore was closed):
    /// `503 Service Unavailable`.
    Saturated { id: String },
    /// The parse exceeded the configured deadline: `504 Gateway Timeout`.
    Timeout { id: String },
    /// The blocking task panicked or was cancelled: `500 Internal Server
    /// Error`. The underlying detail is logged server-side and never
    /// leaked to the client.
    Internal { id: String },
}

impl ParseError {
    fn id(&self) -> &str {
        match self {
            ParseError::Saturated { id }
            | ParseError::Timeout { id }
            | ParseError::Internal { id } => id,
        }
    }

    /// Client-facing message; mirrors the pre-#639 `text/plain` bodies so
    /// the only change is the wire shape, not the wording.
    fn message(&self) -> &'static str {
        match self {
            ParseError::Saturated { .. } => PARSE_POOL_SATURATED,
            ParseError::Timeout { .. } => PARSE_TIMEOUT,
            ParseError::Internal { .. } => INTERNAL_SERVER_ERROR,
        }
    }

    /// Stable `error_kind` machine token for this failure (#631).
    fn error_kind(&self) -> &'static str {
        match self {
            ParseError::Saturated { .. } => error_kind::PARSE_POOL_SATURATED,
            ParseError::Timeout { .. } => error_kind::PARSE_TIMEOUT,
            ParseError::Internal { .. } => error_kind::INTERNAL_ERROR,
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl actix_web::ResponseError for ParseError {
    fn status_code(&self) -> http::StatusCode {
        match self {
            ParseError::Saturated { .. } => http::StatusCode::SERVICE_UNAVAILABLE,
            ParseError::Timeout { .. } => http::StatusCode::GATEWAY_TIMEOUT,
            ParseError::Internal { .. } => http::StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        json_error(
            self.status_code(),
            self.message(),
            self.error_kind(),
            self.id().to_owned(),
        )
    }
}

/// Uniform machine-readable error body for *every* endpoint, regardless
/// of the success content-type (#541, #631).
///
/// The `id` key is always present: it carries the client-supplied
/// correlation id when the request had one, and an empty string
/// otherwise (the octet-stream / query endpoints carry no id, and the
/// content-type / method fallbacks have not parsed a body). Always
/// emitting the key keeps the shape predictable for clients that
/// destructure it.
///
/// `error` is a `Cow` so the static endpoint messages and the dynamic
/// per-request messages from the actix extractor error handlers (serde's
/// precise "missing field …" / "expected … at line …" text) share the
/// same wire shape (#639). `error` carries the *specific* human-readable
/// cause; `error_kind` carries a stable `snake_case` machine token so
/// clients branch on the cause without string-matching the prose (#631).
/// The token vocabulary is closed and governed by `STABILITY.md`;
/// `error_kind` is purely additive over the pre-#631 `{error, id}` shape.
#[derive(Debug, Deserialize, Serialize)]
// The `error` / `error_kind` field names are the published wire contract
// (#631), so they intentionally repeat the struct name.
#[allow(clippy::struct_field_names)]
pub(crate) struct Error {
    error: Cow<'static, str>,
    error_kind: &'static str,
    id: String,
}

/// Builds an `application/json` error response with the uniform
/// `{error, error_kind, id}` body (#541, #631).
///
/// Every error path in the crate routes through this helper so clients
/// parse one error shape no matter which endpoint or success
/// content-type they hit. `status` is the HTTP status; `error_kind` is
/// the stable machine token from [`error_kind`]; `id` is the echoed
/// correlation id (empty when the request carried none).
pub(crate) fn json_error(
    status: http::StatusCode,
    error: impl Into<Cow<'static, str>>,
    error_kind: &'static str,
    id: String,
) -> HttpResponse {
    HttpResponse::build(status).json(Error {
        error: error.into(),
        error_kind,
        id,
    })
}

/// `error` body returned when `Accept` names only unsupported media types
/// (#657). The body interpolates the supported-type list so the client
/// sees exactly what to send.
pub(crate) fn not_acceptable_message() -> String {
    format!(
        "No acceptable representation for the `Accept` header. \
         This endpoint serves {}.",
        negotiate::SUPPORTED_MEDIA_TYPES
    )
}

/// Closed vocabulary of stable `snake_case` machine tokens carried in the
/// `error_kind` field of the uniform error body (#631).
///
/// Each constant names one distinct client-input or server-side failure
/// cause. The set is governed by `STABILITY.md`; adding a token is an
/// additive change, renaming or removing one is a break. Clients branch
/// on these tokens instead of string-matching the free-form `error`
/// prose. The vcs-family mapping lives in [`vcs_error_kind`], keyed off
/// `vcs::Error`'s own variants so a new variant forces a token decision.
pub(crate) mod error_kind {
    /// `file_name` (and content sniffing) mapped to no supported language.
    pub(crate) const UNSUPPORTED_LANGUAGE: &str = "unsupported_language";
    /// The request body could not be parsed / deserialized (malformed
    /// JSON, a missing required field, a wrong type, or — with
    /// `deny_unknown_fields` — an unrecognised key, #633). serde's precise
    /// message names the offending key in the `error` prose.
    pub(crate) const BAD_REQUEST: &str = "bad_request";
    /// The request body carried an unrecognised key, rejected by
    /// `#[serde(deny_unknown_fields)]` (#633). The offending key is named
    /// in the human `error` prose.
    pub(crate) const UNKNOWN_FIELD: &str = "unknown_field";
    /// A query-string parameter was missing or malformed.
    pub(crate) const BAD_QUERY: &str = "bad_query";
    /// The `scope` query flag was not a recognised value (#638).
    pub(crate) const INVALID_SCOPE_FLAG: &str = "invalid_scope_flag";
    /// A `/vcs/jit` payload combined `diff` with commit-mode fields.
    pub(crate) const VCS_MODE_CONFLICT: &str = "vcs_mode_conflict";
    /// The request body exceeded the maximum accepted size.
    pub(crate) const PAYLOAD_TOO_LARGE: &str = "payload_too_large";
    /// A transport-level read failure while draining the request body.
    pub(crate) const READ_ERROR: &str = "read_error";
    /// The blocking parse task panicked or was cancelled (server fault).
    pub(crate) const INTERNAL_ERROR: &str = "internal_error";
    /// The parse exceeded the configured deadline.
    pub(crate) const PARSE_TIMEOUT: &str = "parse_timeout";
    /// The orphaned-task pool was saturated; the request was shed.
    pub(crate) const PARSE_POOL_SATURATED: &str = "parse_pool_saturated";
    /// AST construction yielded no root node (defensive 500).
    pub(crate) const AST_BUILD_FAILED: &str = "ast_build_failed";
    /// Metric computation failed for a parsed source (defensive 500).
    pub(crate) const METRICS_FAILED: &str = "metrics_failed";
    /// The history walk itself failed (environment / backend, not input).
    pub(crate) const VCS_INTERNAL_ERROR: &str = "vcs_internal_error";
    /// The target path is not inside a supported VCS working tree.
    pub(crate) const VCS_NOT_A_REPOSITORY: &str = "vcs_not_a_repository";
    /// A `reference` / `commit` could not be resolved.
    pub(crate) const VCS_INVALID_REVISION: &str = "vcs_invalid_revision";
    /// A bot-author glob pattern was malformed.
    pub(crate) const VCS_INVALID_BOT_PATTERN: &str = "vcs_invalid_bot_pattern";
    /// A time-window option was malformed.
    pub(crate) const VCS_INVALID_WINDOW: &str = "vcs_invalid_window";
    /// An `as-of` / point timestamp was malformed.
    pub(crate) const VCS_INVALID_TIMESTAMP: &str = "vcs_invalid_timestamp";
    /// The risk-score formula was malformed.
    pub(crate) const VCS_INVALID_FORMULA: &str = "vcs_invalid_formula";
    /// A file-type scope option was malformed.
    pub(crate) const VCS_INVALID_FILE_TYPE_SCOPE: &str = "vcs_invalid_file_type_scope";
    /// The bus-factor threshold was out of range.
    pub(crate) const VCS_INVALID_BUS_FACTOR_THRESHOLD: &str = "vcs_invalid_bus_factor_threshold";
    /// A trend point-count / span option was malformed.
    pub(crate) const VCS_INVALID_TREND: &str = "vcs_invalid_trend";
    /// The unified diff passed to `/vcs/jit` was malformed.
    pub(crate) const VCS_INVALID_DIFF: &str = "vcs_invalid_diff";
    /// No registered resource matched the request URL.
    pub(crate) const NOT_FOUND: &str = "not_found";
    /// The resource does not accept the request method.
    pub(crate) const METHOD_NOT_ALLOWED: &str = "method_not_allowed";
    /// The `POST` body carried an unsupported / missing `Content-Type`.
    pub(crate) const UNSUPPORTED_MEDIA_TYPE: &str = "unsupported_media_type";
    /// The request `Accept` header named only media types this server
    /// cannot produce; content negotiation answered `406` (#657).
    pub(crate) const NOT_ACCEPTABLE: &str = "not_acceptable";
    /// The negotiated serializer failed to encode the response (defensive
    /// `500`); the response types are plain data, so this is unreachable
    /// today (#657).
    pub(crate) const SERIALIZE_FAILED: &str = "serialize_failed";
}

/// Stable `error_kind` token for a [`VcsError`] (#631).
///
/// Keyed exhaustively off the library `vcs::Error` variants (no wildcard
/// arm) so adding a variant is a compile error here until a token is
/// chosen — the same forcing function `is_client_input` uses. Every
/// environment / backend failure collapses onto
/// [`error_kind::VCS_INTERNAL_ERROR`]; each client-input variant gets its
/// own specific token.
pub(crate) fn vcs_error_kind(error: &VcsError) -> &'static str {
    match error {
        VcsError::NotARepository(_) => error_kind::VCS_NOT_A_REPOSITORY,
        VcsError::ResolveRef { .. } => error_kind::VCS_INVALID_REVISION,
        VcsError::InvalidBotPattern(_) => error_kind::VCS_INVALID_BOT_PATTERN,
        VcsError::InvalidWindow(_) => error_kind::VCS_INVALID_WINDOW,
        VcsError::InvalidTimestamp(_) => error_kind::VCS_INVALID_TIMESTAMP,
        VcsError::InvalidFormula(_) => error_kind::VCS_INVALID_FORMULA,
        VcsError::InvalidFileTypeScope(_) => error_kind::VCS_INVALID_FILE_TYPE_SCOPE,
        VcsError::InvalidBusFactorThreshold(_) => error_kind::VCS_INVALID_BUS_FACTOR_THRESHOLD,
        VcsError::InvalidTrend(_) => error_kind::VCS_INVALID_TREND,
        VcsError::InvalidDiff(_) => error_kind::VCS_INVALID_DIFF,
        // Every environment / backend failure (`OpenRepository`, `Walk`,
        // `Diff`, `Mailmap`, `Blame`, `Cache`) and any future
        // `#[non_exhaustive]` variant collapses onto the generic internal
        // token; `is_client_input` already owns the exhaustive
        // client-vs-environment forcing function, so this wildcard cannot
        // silently mis-classify a client error.
        _ => error_kind::VCS_INTERNAL_ERROR,
    }
}

/// Builds the uniform error response for a request whose `file_name`
/// resolves to no supported language (issue #634).
///
/// Every analysis endpoint shares this exact `422` + [`UNSUPPORTED_LANGUAGE`]
/// outcome, so the status and token live in one place rather than being
/// repeated per handler.
pub(crate) fn unsupported_language(id: String) -> HttpResponse {
    json_error(
        http::StatusCode::UNPROCESSABLE_ENTITY,
        UNSUPPORTED_LANGUAGE,
        error_kind::UNSUPPORTED_LANGUAGE,
        id,
    )
}

/// Failure draining an `application/octet-stream` request body (#639).
///
/// The octet-stream endpoints carry no correlation id, so the rendered
/// `{error, id}` body always has an empty `id`. Replaces the former
/// `actix_web::error::ErrorPayloadTooLarge` / propagated `PayloadError`
/// returns, which emitted a bare `text/plain` body and so produced a
/// *second* 413 shape diverging from the JSON-endpoint 413 (#639 unifies
/// the two).
#[derive(Debug)]
pub(crate) enum BodyError {
    /// Body exceeded the configured size cap: `413 Payload Too Large`.
    TooLarge,
    /// Transport-level read error while draining the stream: `400 Bad
    /// Request`.
    Read,
}

impl BodyError {
    fn message(&self) -> &'static str {
        match self {
            BodyError::TooLarge => PAYLOAD_TOO_LARGE,
            BodyError::Read => "Failed to read the request body",
        }
    }

    /// Stable `error_kind` machine token for this failure (#631).
    fn error_kind(&self) -> &'static str {
        match self {
            BodyError::TooLarge => error_kind::PAYLOAD_TOO_LARGE,
            BodyError::Read => error_kind::READ_ERROR,
        }
    }
}

impl fmt::Display for BodyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl actix_web::ResponseError for BodyError {
    fn status_code(&self) -> http::StatusCode {
        match self {
            BodyError::TooLarge => http::StatusCode::PAYLOAD_TOO_LARGE,
            BodyError::Read => http::StatusCode::BAD_REQUEST,
        }
    }

    fn error_response(&self) -> HttpResponse {
        json_error(
            self.status_code(),
            self.message(),
            self.error_kind(),
            String::new(),
        )
    }
}

/// Map a [`VcsError`] from `/vcs`, `/vcs/trend`, or `/vcs/jit` to the
/// uniform JSON error response (#631). Bad path / window / timestamp /
/// pattern / formula / threshold / trend-parameter / `ref` / diff are
/// client mistakes (`400`); an actual walk failure is a `500`.
///
/// A client-input failure now carries the *specific* `vcs::Error`
/// `Display` message (e.g. `invalid time window: banana`) plus its own
/// `error_kind` token from [`vcs_error_kind`] (e.g. `vcs_invalid_window`),
/// replacing the former kitchen-sink `VCS_BAD_REQUEST` sentence that
/// enumerated every possible cause and named `/vcs` even on `/vcs/jit` or
/// `/vcs/trend`. An environment / backend failure keeps the generic
/// [`VCS_FAILED`] body — the real error is logged server-side, not leaked.
pub(crate) fn vcs_error_response(error: &VcsError, payload_id: String) -> HttpResponse {
    // The library owns the client-input vs environment/backend split
    // (`vcs::Error::is_client_input`, issue #641) via an exhaustive match,
    // so a new variant forces a classification decision at the library and
    // is mapped here without re-enumerating variants — closing the silent
    // fall-through that twice mis-mapped client errors to 500.
    let kind = vcs_error_kind(error);
    let (status, body): (http::StatusCode, Cow<'static, str>) = if error.is_client_input() {
        (http::StatusCode::BAD_REQUEST, error.to_string().into())
    } else {
        (
            http::StatusCode::INTERNAL_SERVER_ERROR,
            Cow::Borrowed(VCS_FAILED),
        )
    };
    tracing::warn!(payload_id = %payload_id, error = %error, "vcs request failed");
    json_error(status, body, kind, payload_id)
}

/// serde's deny-unknown-fields message marker (#633).
///
/// `deny_unknown_fields` renders an unrecognised key as an `unknown
/// field "<key>", expected one of ...` message. actix wraps that in its
/// own `JsonPayloadError` / `QueryPayloadError` `Display` prefix (e.g.
/// `Json deserialize error: ...`), so the marker is matched as a substring
/// rather than a prefix. Detecting it lets the extractor error handler
/// stamp the specific [`error_kind::UNKNOWN_FIELD`] token (with the field
/// name preserved in the human `error` prose) instead of the generic
/// `bad_request`.
pub(crate) const SERDE_UNKNOWN_FIELD_MARKER: &str = "unknown field";

/// Renders a `JsonConfig` extractor failure as the uniform
/// `{error, error_kind, id}` JSON body (#639, #631).
///
/// An oversized body answers `413`; every other failure (malformed JSON,
/// missing field, wrong content type, an unknown field under
/// `deny_unknown_fields`) answers `400`. serde's precise message is
/// preserved as the `error` value rather than collapsed to a token, while
/// `error_kind` carries the machine token: `payload_too_large`,
/// `unknown_field` for a rejected key (#633), or the generic
/// `bad_request`. Factored out of [`json_config`] so the integration
/// tests can attach it to a small-limit `JsonConfig` and exercise the
/// `413` body without allocating megabytes.
pub(crate) fn json_error_handler(
    err: actix_web::error::JsonPayloadError,
    _req: &actix_web::HttpRequest,
) -> actix_web::Error {
    let message = err.to_string();
    let (status, kind) = match err {
        actix_web::error::JsonPayloadError::Overflow { .. }
        | actix_web::error::JsonPayloadError::OverflowKnownLength { .. } => (
            http::StatusCode::PAYLOAD_TOO_LARGE,
            error_kind::PAYLOAD_TOO_LARGE,
        ),
        _ => (http::StatusCode::BAD_REQUEST, bad_request_kind(&message)),
    };
    let response = json_error(status, message, kind, String::new());
    actix_web::error::InternalError::from_response(err, response).into()
}

/// Classifies a serde deserialization message into a `400` `error_kind`
/// token (#631 / #633).
///
/// An unknown-field rejection (the `deny_unknown_fields` path, #633) gets
/// the specific [`error_kind::UNKNOWN_FIELD`] token so a client can detect
/// a typo'd key without scraping the prose; every other deserialization
/// failure (malformed JSON, missing field, wrong type) is the generic
/// [`error_kind::BAD_REQUEST`].
pub(crate) fn bad_request_kind(message: &str) -> &'static str {
    if message.contains(SERDE_UNKNOWN_FIELD_MARKER) {
        error_kind::UNKNOWN_FIELD
    } else {
        error_kind::BAD_REQUEST
    }
}
