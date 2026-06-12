// bca: suppress-file(halstead, nargs, nexits, abc, nom, loc)
// Actix server setup + handlers; the file-aggregate halstead/nargs/exit/nom/loc and
// the route-registration closure's abc (one `.service()`/`.route()` per endpoint)
// are declarative many-fn aggregation artifacts, not per-function logic
// complexity. The per-endpoint handlers plus the content-type guard helpers
// (#515) push the file's method count past the per-file nom cap.

use std::borrow::Cow;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use actix_web::{
    App, HttpResponse, HttpServer, guard,
    guard::GuardContext,
    http,
    http::header::ContentType,
    middleware::Condition,
    mime,
    web::{self, BytesMut, Query},
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use super::comment::{
    WebCommentCfg, WebCommentInfo, WebCommentJson, WebCommentPayload, strip_comments,
};
use super::cors::{CorsPolicy, cors_middleware};
use super::function::{WebFunctionCfg, WebFunctionInfo, WebFunctionPayload, function_spans};
use super::metrics::{Scope, WebMetricsCfg, WebMetricsInfo, WebMetricsPayload, compute_metrics};
use super::negotiate;
use super::vcs::{
    WebVcsJitPayload, WebVcsPayload, WebVcsTrendPayload, compute_vcs, compute_vcs_jit,
    compute_vcs_trend,
};

use big_code_analysis::vcs::Error as VcsError;
use big_code_analysis::{Ast, AstCfg, AstPayload, LANG, Source, guess_language, normalize_eol};

/// `error` message returned when the submitted `file_name` (and content
/// sniffing) cannot be mapped to a supported language.
///
/// The route was matched and the request entity is well-formed, so the
/// failure is *unprocessable*, not *not found*: the endpoint answers
/// `422 Unprocessable Entity` rather than the pre-2.0 `404` (issue #634),
/// carrying [`error_kind::UNSUPPORTED_LANGUAGE`] as its machine token
/// (#631). The string value coincides with the token. The set of
/// supported languages is available from `GET /v1/languages`.
const UNSUPPORTED_LANGUAGE: &str = "unsupported_language";

/// Error body emitted when the `scope` query flag is not a recognised
/// value (#638).
const INVALID_SCOPE_FLAG: &str =
    "The `scope` query flag must be one of `full` or `file` (case-insensitive)";

/// Parses the optional `scope` query flag into a [`Scope`] (#638).
///
/// Absent (`None`) defaults to [`Scope::Full`] (the entire space tree).
/// Present values accept `full`/`file`, case-insensitively, mirroring the
/// JSON-payload `scope` enum (`WebMetricsPayload.scope`). Replaces the
/// pre-2.0 boolean `unit` flag — the old `true`/`false`/`1`/`0` truthy set
/// is no longer accepted. Any other value is rejected so the caller can
/// answer `400` with the uniform JSON error body.
fn parse_scope_flag(raw: Option<&str>) -> Result<Scope, &'static str> {
    match raw {
        None => Ok(Scope::Full),
        Some(s) if s.eq_ignore_ascii_case("full") => Ok(Scope::Full),
        Some(s) if s.eq_ignore_ascii_case("file") => Ok(Scope::File),
        Some(_) => Err(INVALID_SCOPE_FLAG),
    }
}

/// `expect` message used at every `action::<_>` call site below.
///
/// The web crate pins `big-code-analysis` with `features =
/// ["all-languages"]`, so a `LANG` value that reached this point must
/// be enabled at compile time. Any future caller that loosens the
/// feature pin must change this invariant explicitly.
const FEATURES_PINNED: &str = "web crate pins big-code-analysis features = [\"all-languages\"]";

/// Swaps C++ to the `Ccomment` grammar for comment-removal endpoints.
fn comment_language(language: LANG) -> LANG {
    if language == LANG::Cpp {
        LANG::Ccomment
    } else {
        language
    }
}

struct ParseConfig {
    /// `None` means no timeout (`parse_timeout_secs = 0`).
    timeout: Option<Duration>,
    semaphore: Arc<Semaphore>,
    /// Running count of blocking tasks that timed out but have not yet finished.
    orphaned_tasks: Arc<AtomicUsize>,
    /// Reject new requests with 503 once orphaned task count reaches this limit.
    max_orphaned_tasks: usize,
    /// Maximum accepted request-body size in bytes for the streaming
    /// octet-stream handlers. Enforced incrementally in [`get_code`] so an
    /// oversized body is rejected with 413 before it is fully buffered.
    max_body_size: usize,
}

const PARSE_TIMEOUT: &str = "Parse timed out";
const PARSE_POOL_SATURATED: &str = "parse pool saturated";
const PAYLOAD_TOO_LARGE: &str = "Request body exceeds the maximum allowed size";

/// Client-facing message for a parse task that panicked or was cancelled.
///
/// Deliberately generic: the underlying detail (panic message, join
/// error) is logged server-side but never leaked to the client (#639,
/// pinned by `test_run_parse_error_*_does_not_leak_internals`).
const INTERNAL_SERVER_ERROR: &str = "Internal server error";

/// Error body emitted when AST construction yields no root node.
///
/// Defensive: `build` only returns `None` in a degenerate case the
/// current grammars never reach for a parsed root, so this `500` is
/// unreachable today (hence no integration test drives it). It exists
/// so a future walker change cannot silently regress to the old
/// `200`-with-`root: null` body (issue #517).
const AST_BUILD_FAILED: &str = "Failed to build an AST for the supplied source";
/// Error body emitted when metric computation fails for a parsed source.
///
/// Defensive, like [`AST_BUILD_FAILED`]: `metrics_with_options` does not
/// error for the web crate today (every `MetricsError` variant is either
/// reserved or guarded against by the `all-languages` feature pin), so
/// this `500` is unreachable today and exists to keep failures off the
/// `200` path if that changes (issue #517).
const METRICS_FAILED: &str = "Failed to compute metrics for the supplied source";
/// Error body when the history walk itself fails (server-side → `500`).
///
/// Client-input vcs failures no longer collapse onto one kitchen-sink
/// message: each carries the specific `vcs::Error` `Display` output plus
/// its own `error_kind` token (#631). This constant is the generic body
/// for the *environment / backend* `500` path only, where the real cause
/// is logged server-side rather than leaked.
const VCS_FAILED: &str = "Failed to walk change history for the supplied repository";

/// Default parse timeout used by [`run`].
pub const DEFAULT_PARSE_TIMEOUT_SECS: u64 = 30;

/// Maximum accepted request-body size, in bytes (4 MiB).
///
/// Applied uniformly to the JSON extractor (via `JsonConfig::limit`) and
/// the streaming octet-stream handlers (via [`get_code`]) so both
/// content types reject oversized bodies at the same threshold and with
/// the same `413` JSON body (#639).
const MAX_BODY_SIZE: usize = 1_024 * 1_024 * 4;

/// Runs `f` on the blocking pool under the timeout / orphan-pool policy.
///
/// `payload_id` is the client-supplied request id from the JSON payload
/// (empty for the octet-stream endpoints, which carry none). It is logged
/// on the failure path under a distinct field name so it does not collide
/// with `tracing-actix-web`'s own span-level `request_id` (a per-request
/// UUID), and it is echoed back in the `{error, id}` body of the typed
/// [`ParseError`] returned on every failure (#639).
async fn run_parse<T: Send + 'static>(
    config: &web::Data<ParseConfig>,
    payload_id: &str,
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, ParseError> {
    // Reject when the orphaned-task pool has saturated. `Acquire` pairs with
    // the `AcqRel` RMW ops on the timeout path so newly admitted requests
    // observe orphan counts published by any prior orphaning task.
    let pool_saturated =
        || config.orphaned_tasks.load(Ordering::Acquire) >= config.max_orphaned_tasks;

    // Fast-path admission check: cheap rejection before acquiring a semaphore
    // permit. A burst of concurrent requests may still pass this check while
    // the counter is briefly low, so the post-admission re-check below is the
    // hard gate.
    if pool_saturated() {
        return Err(ParseError::Saturated {
            id: payload_id.to_owned(),
        });
    }

    let permit = Arc::clone(&config.semaphore)
        .acquire_owned()
        .await
        .map_err(|_| ParseError::Saturated {
            id: payload_id.to_owned(),
        })?;

    // Re-check after semaphore admission. A queued burst can all pass the
    // pre-admission check while the orphan count is still low, then drain the
    // semaphore one at a time. Without this second check each admitted request
    // would spawn another blocking task and grow the orphan pool past the cap.
    // `permit` is dropped by RAII on early return, returning its slot to the
    // semaphore.
    if pool_saturated() {
        return Err(ParseError::Saturated {
            id: payload_id.to_owned(),
        });
    }

    let mut handle = tokio::task::spawn_blocking(f);

    let result = if let Some(deadline) = config.timeout {
        match tokio::time::timeout(deadline, &mut handle).await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(e)) => {
                // Log the full error server-side for ops diagnostics; the
                // client only sees the generic "Internal server error" string.
                tracing::error!(payload_id = %payload_id, error = %e, "Parse task failed");
                Err(ParseError::Internal {
                    id: payload_id.to_owned(),
                })
            }
            Err(_) => {
                // A timeout orphans the blocking task until it finishes on
                // its own. Log it (`warn`, not `error`: it is a deadline /
                // load condition, not an internal fault) so ops can correlate
                // which request timed out and track orphan-pool pressure.
                tracing::warn!(
                    payload_id = %payload_id,
                    timeout_secs = deadline.as_secs(),
                    "Parse timed out; blocking task orphaned"
                );
                let counter = Arc::clone(&config.orphaned_tasks);
                // AcqRel: load+publish so admission re-checks observe
                // the latest count. Pairs with the `Acquire` loads in
                // the admission checks above.
                counter.fetch_add(1, Ordering::AcqRel);
                tokio::spawn(async move {
                    let _ = handle.await;
                    // AcqRel: load+publish so admission re-checks
                    // observe the latest count.
                    counter.fetch_sub(1, Ordering::AcqRel);
                });
                Err(ParseError::Timeout {
                    id: payload_id.to_owned(),
                })
            }
        }
    } else {
        handle.await.map_err(|e| {
            tracing::error!(payload_id = %payload_id, error = %e, "Parse task failed");
            ParseError::Internal {
                id: payload_id.to_owned(),
            }
        })
    };
    drop(permit);
    result
}

/// Typed failure of [`run_parse`], rendered as the uniform `{error, id}`
/// JSON body (#639).
///
/// Before #639 these paths returned actix's `ErrorServiceUnavailable` /
/// `ErrorGatewayTimeout` / `ErrorInternalServerError` helpers, which emit
/// a bare `text/plain` body and so bypassed the published `{error, id}`
/// contract. Carrying the correlation id on each variant lets the
/// `ResponseError` impl echo it back exactly as the success paths do.
#[derive(Debug)]
enum ParseError {
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
struct Error {
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
fn json_error(
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
fn not_acceptable_message() -> String {
    format!(
        "No acceptable representation for the `Accept` header. \
         This endpoint serves {}.",
        negotiate::SUPPORTED_MEDIA_TYPES
    )
}

/// Renders `value` in the format negotiated from `req`'s `Accept` header
/// (#657), or the matching `406` / `500` when negotiation or serialization
/// fails.
///
/// JSON stays the default (absent `Accept`, `*/*`, or `application/json`)
/// and carries the exact `application/json` content type the pre-#657
/// `.json(...)` path emitted, so existing clients see no change. `Accept:
/// application/yaml` / `application/cbor` get that format with the matching
/// `Content-Type`; any other concrete type answers `406 Not Acceptable`.
/// Every failure routes through [`json_error`] so the `{error, error_kind,
/// id}` envelope is identical to every other endpoint. `id` is the echoed
/// correlation id (empty for the id-less octet-stream / query endpoints).
fn negotiated_ok<T: Serialize>(
    req: &actix_web::HttpRequest,
    value: &T,
    id: String,
) -> HttpResponse {
    let Some(format) = negotiate::from_request(req) else {
        return json_error(
            http::StatusCode::NOT_ACCEPTABLE,
            not_acceptable_message(),
            error_kind::NOT_ACCEPTABLE,
            id,
        );
    };
    let Ok(body) = format.encode(value) else {
        // Defensive: the response types are plain data, so encoding never
        // fails today — but a future field that breaks a serializer must
        // not surface as a partially-written `200` body (mirrors the
        // METRICS_FAILED / AST_BUILD_FAILED 500 discipline, #517). Leave a
        // server-side breadcrumb like every other 500 in this module; the
        // concrete serializer error was already collapsed to `()` in
        // `Format::encode`, so only the negotiated format is recoverable.
        tracing::error!(
            id = %id,
            format = ?format,
            "Failed to serialize the response in the negotiated format"
        );
        return json_error(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to serialize the response in the negotiated format",
            error_kind::SERIALIZE_FAILED,
            id,
        );
    };
    HttpResponse::Ok()
        .append_header((http::header::CONTENT_TYPE, format.content_type()))
        .body(body)
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
mod error_kind {
    /// `file_name` (and content sniffing) mapped to no supported language.
    pub(super) const UNSUPPORTED_LANGUAGE: &str = "unsupported_language";
    /// The request body could not be parsed / deserialized (malformed
    /// JSON, a missing required field, a wrong type, or — with
    /// `deny_unknown_fields` — an unrecognised key, #633). serde's precise
    /// message names the offending key in the `error` prose.
    pub(super) const BAD_REQUEST: &str = "bad_request";
    /// The request body carried an unrecognised key, rejected by
    /// `#[serde(deny_unknown_fields)]` (#633). The offending key is named
    /// in the human `error` prose.
    pub(super) const UNKNOWN_FIELD: &str = "unknown_field";
    /// A query-string parameter was missing or malformed.
    pub(super) const BAD_QUERY: &str = "bad_query";
    /// The `scope` query flag was not a recognised value (#638).
    pub(super) const INVALID_SCOPE_FLAG: &str = "invalid_scope_flag";
    /// A `/vcs/jit` payload combined `diff` with commit-mode fields.
    pub(super) const VCS_MODE_CONFLICT: &str = "vcs_mode_conflict";
    /// The request body exceeded the maximum accepted size.
    pub(super) const PAYLOAD_TOO_LARGE: &str = "payload_too_large";
    /// A transport-level read failure while draining the request body.
    pub(super) const READ_ERROR: &str = "read_error";
    /// The blocking parse task panicked or was cancelled (server fault).
    pub(super) const INTERNAL_ERROR: &str = "internal_error";
    /// The parse exceeded the configured deadline.
    pub(super) const PARSE_TIMEOUT: &str = "parse_timeout";
    /// The orphaned-task pool was saturated; the request was shed.
    pub(super) const PARSE_POOL_SATURATED: &str = "parse_pool_saturated";
    /// AST construction yielded no root node (defensive 500).
    pub(super) const AST_BUILD_FAILED: &str = "ast_build_failed";
    /// Metric computation failed for a parsed source (defensive 500).
    pub(super) const METRICS_FAILED: &str = "metrics_failed";
    /// The history walk itself failed (environment / backend, not input).
    pub(super) const VCS_INTERNAL_ERROR: &str = "vcs_internal_error";
    /// The target path is not inside a supported VCS working tree.
    pub(super) const VCS_NOT_A_REPOSITORY: &str = "vcs_not_a_repository";
    /// A `reference` / `commit` could not be resolved.
    pub(super) const VCS_INVALID_REVISION: &str = "vcs_invalid_revision";
    /// A bot-author glob pattern was malformed.
    pub(super) const VCS_INVALID_BOT_PATTERN: &str = "vcs_invalid_bot_pattern";
    /// A time-window option was malformed.
    pub(super) const VCS_INVALID_WINDOW: &str = "vcs_invalid_window";
    /// An `as-of` / point timestamp was malformed.
    pub(super) const VCS_INVALID_TIMESTAMP: &str = "vcs_invalid_timestamp";
    /// The risk-score formula was malformed.
    pub(super) const VCS_INVALID_FORMULA: &str = "vcs_invalid_formula";
    /// A file-type scope option was malformed.
    pub(super) const VCS_INVALID_FILE_TYPE_SCOPE: &str = "vcs_invalid_file_type_scope";
    /// The bus-factor threshold was out of range.
    pub(super) const VCS_INVALID_BUS_FACTOR_THRESHOLD: &str = "vcs_invalid_bus_factor_threshold";
    /// A trend point-count / span option was malformed.
    pub(super) const VCS_INVALID_TREND: &str = "vcs_invalid_trend";
    /// The unified diff passed to `/vcs/jit` was malformed.
    pub(super) const VCS_INVALID_DIFF: &str = "vcs_invalid_diff";
    /// No registered resource matched the request URL.
    pub(super) const NOT_FOUND: &str = "not_found";
    /// The resource does not accept the request method.
    pub(super) const METHOD_NOT_ALLOWED: &str = "method_not_allowed";
    /// The `POST` body carried an unsupported / missing `Content-Type`.
    pub(super) const UNSUPPORTED_MEDIA_TYPE: &str = "unsupported_media_type";
    /// The request `Accept` header named only media types this server
    /// cannot produce; content negotiation answered `406` (#657).
    pub(super) const NOT_ACCEPTABLE: &str = "not_acceptable";
    /// The negotiated serializer failed to encode the response (defensive
    /// `500`); the response types are plain data, so this is unreachable
    /// today (#657).
    pub(super) const SERIALIZE_FAILED: &str = "serialize_failed";
}

/// Stable `error_kind` token for a [`VcsError`] (#631).
///
/// Keyed exhaustively off the library `vcs::Error` variants (no wildcard
/// arm) so adding a variant is a compile error here until a token is
/// chosen — the same forcing function `is_client_input` uses. Every
/// environment / backend failure collapses onto
/// [`error_kind::VCS_INTERNAL_ERROR`]; each client-input variant gets its
/// own specific token.
fn vcs_error_kind(error: &VcsError) -> &'static str {
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
fn unsupported_language(id: String) -> HttpResponse {
    json_error(
        http::StatusCode::UNPROCESSABLE_ENTITY,
        UNSUPPORTED_LANGUAGE,
        error_kind::UNSUPPORTED_LANGUAGE,
        id,
    )
}

/// Drains a streaming request body into a byte buffer, enforcing `max_size`.
///
/// The `web::Payload` extractor ignores `web::PayloadConfig`, so the size
/// limit must be enforced here. The running total is checked against
/// `max_size` *before* each chunk is appended, so an oversized body is
/// rejected with 413 as soon as it would exceed the limit rather than being
/// fully buffered first. A body whose total length equals `max_size` is
/// accepted; one byte over is rejected. Both the oversize and the
/// transport-read failure are returned as a typed [`BodyError`] so the
/// octet-stream endpoints answer the uniform `{error, id}` JSON body
/// instead of actix's default plaintext (#639).
async fn get_code(mut body: web::Payload, max_size: usize) -> Result<Vec<u8>, BodyError> {
    let mut code = BytesMut::new();
    while let Some(item) = body.next().await {
        // A transport-level read failure is a client/connection fault, not a
        // server fault: render it as the uniform 400 JSON body rather than
        // actix's default plaintext (#639).
        let chunk = item.map_err(|_| BodyError::Read)?;
        if code.len() + chunk.len() > max_size {
            return Err(BodyError::TooLarge);
        }
        code.extend_from_slice(&chunk);
    }

    Ok(code.to_vec())
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
enum BodyError {
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

async fn ast_parser(
    req: actix_web::HttpRequest,
    item: web::Json<AstPayload>,
    config: web::Data<ParseConfig>,
) -> Result<HttpResponse, actix_web::Error> {
    let path = PathBuf::from(&item.file_name);
    let payload = item.into_inner();
    // Deliberately NOT normalised (issue #640): the AST endpoint reports
    // tree-sitter node coordinates (rows/columns) over the exact bytes the
    // client submitted. Appending a trailing newline or rewriting EOLs would
    // shift every node position relative to the input the client holds. The
    // #640 fix targets the *metric* surfaces, whose derived numbers must match
    // the CLI; AST coordinates are byte-faithful by contract.
    let buf = payload.code.into_bytes();
    let (language, _) = guess_language(&buf, path);
    if let Some(language) = language {
        // Clone the (small) correlation id for server-side log correlation;
        // the original is moved into `cfg` and consumed by `action` below.
        let payload_id = payload.id.clone();
        // Echo the resolved #540 canonical slug in the envelope, matching
        // /comment, /function, and /metrics (#654). Captured before
        // `language` moves into the parse closure below.
        let cfg = AstCfg {
            id: payload.id,
            language: language.name().to_string(),
            comment: payload.comment,
            span: payload.span,
        };
        let result = run_parse(&config, &payload_id, move || {
            Ast::parse(Source::from_bytes(language, buf))
                .expect(FEATURES_PINNED)
                .dump(cfg)
        })
        .await?;
        // `root == None` previously surfaced as a `200` carrying
        // `root: null` (an error signalled inside a success body); map it
        // to an explicit `500` with an error body instead (issue #517).
        if result.root.is_some() {
            let id = result.id.clone();
            Ok(negotiated_ok(&req, &result, id))
        } else {
            Ok(json_error(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                AST_BUILD_FAILED,
                error_kind::AST_BUILD_FAILED,
                result.id,
            ))
        }
    } else {
        Ok(unsupported_language(payload.id))
    }
}

async fn comment_removal_json(
    req: actix_web::HttpRequest,
    item: web::Json<WebCommentPayload>,
    config: web::Data<ParseConfig>,
) -> Result<HttpResponse, actix_web::Error> {
    let path = PathBuf::from(&item.file_name);
    let payload = item.into_inner();
    // Deliberately NOT normalised (issue #640): comment removal round-trips
    // the source bytes back to the caller, so normalising would silently
    // append a trailing newline to content the client never asked us to
    // touch. The parity fix targets the metric surfaces (metrics / function
    // spans), which emit only derived numbers — not the byte-faithful AST and
    // comment endpoints.
    let buf = payload.code.into_bytes();
    let (language, _) = guess_language(&buf, path);
    if let Some(language) = language {
        let payload_id = payload.id.clone();
        // Report the *guessed* language slug, not the comment-removal
        // grammar swap (`Cpp` -> `Ccomment`): the client cares which
        // source language was detected, not the internal helper grammar.
        let cfg = WebCommentCfg {
            id: payload.id,
            language: language.name().to_string(),
        };
        let language = comment_language(language);
        let result = run_parse(&config, &payload_id, move || {
            strip_comments(language, buf, cfg).expect(FEATURES_PINNED)
        })
        .await?;
        // The JSON variant returns `code` as a string (#629). The request
        // `code` arrived as a JSON string and comment removal only deletes
        // byte ranges, so the stripped source stays valid UTF-8; a decode
        // failure would be a logic error, surfaced as a uniform 500 rather
        // than panicking or emitting a lossy payload.
        match WebCommentJson::try_from(result) {
            Ok(json) => {
                let id = json.id.clone();
                Ok(negotiated_ok(&req, &json, id))
            }
            Err(_) => Ok(json_error(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                INTERNAL_SERVER_ERROR,
                error_kind::INTERNAL_ERROR,
                payload_id,
            )),
        }
    } else {
        Ok(unsupported_language(payload.id))
    }
}

async fn comment_removal_plain(
    body: web::Payload,
    info: Query<WebCommentInfo>,
    config: web::Data<ParseConfig>,
) -> Result<HttpResponse, actix_web::Error> {
    // Deliberately NOT normalised (issue #640): the octet-stream comment
    // endpoint returns the stripped source bytes verbatim, so EOL/trailing-
    // newline rewriting would corrupt the client's content.
    let buf = get_code(body, config.max_body_size).await?;
    let path = PathBuf::from(&info.file_name);
    let (language, _) = guess_language(&buf, path);
    if let Some(language) = language {
        let cfg = WebCommentCfg {
            id: String::new(),
            language: language.name().to_string(),
        };
        let language = comment_language(language);
        // The octet-stream variants carry no request id in the body, so log
        // correlation falls back to the `TracingLogger` request span.
        let res = run_parse(&config, "", move || {
            strip_comments(language, buf, cfg).expect(FEATURES_PINNED)
        })
        .await?;
        // The "no comments to strip" outcome is the empty byte
        // sequence; both content types report it as `200` with an empty
        // payload rather than the JSON variant `200` diverging from a
        // `204 No Content` here (#558).
        Ok(HttpResponse::Ok()
            .append_header((http::header::CONTENT_TYPE, "application/octet-stream"))
            .body(res.code))
    } else {
        // Even on the octet-stream endpoint, errors use the uniform JSON
        // body so clients parse one error shape everywhere (#541).
        Ok(unsupported_language(String::new()))
    }
}

async fn metrics_json(
    req: actix_web::HttpRequest,
    item: web::Json<WebMetricsPayload>,
    config: web::Data<ParseConfig>,
) -> Result<HttpResponse, actix_web::Error> {
    let path = PathBuf::from(&item.file_name);
    let payload = item.into_inner();
    // Match the CLI's `read_file_with_eol` normalisation so the same bytes
    // report identical metrics whether they arrive over the wire or from a
    // file (issue #640). An unterminated buffer otherwise reports `sloc: 0`
    // and an impossible `end_line: 0` for the unit space.
    let buf = normalize_eol(payload.code.into_bytes());
    let (language, name) = guess_language(&buf, &path);
    if let Some(language) = language {
        // `exclude_tests` defaults to `false` via `WebMetricsCfg::new`,
        // preserving the pre-#182 numbers for every existing REST
        // client. A future change can thread the flag through the
        // request payload and chain `.with_exclude_tests(...)` here.
        let payload_id = payload.id.clone();
        let cfg = WebMetricsCfg::new(payload.id, path, payload.scope, name.to_string());
        let response = run_parse(&config, &payload_id, move || {
            compute_metrics(language, buf, cfg).expect(FEATURES_PINNED)
        })
        .await?;
        // `None` means metric computation failed: answer with an explicit
        // `500` instead of the former `200`-with-`spaces: null` (issue #517).
        match response {
            Some(resp) => {
                let id = resp.id.clone();
                Ok(negotiated_ok(&req, &resp, id))
            }
            None => Ok(json_error(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                METRICS_FAILED,
                error_kind::METRICS_FAILED,
                payload_id,
            )),
        }
    } else {
        Ok(unsupported_language(payload.id))
    }
}

async fn vcs_json(
    req: actix_web::HttpRequest,
    item: web::Json<WebVcsPayload>,
    config: web::Data<ParseConfig>,
) -> Result<HttpResponse, actix_web::Error> {
    let payload = item.into_inner();
    let payload_id = payload.id.clone();
    // The history walk is blocking I/O, so it runs on the same
    // timeout-guarded blocking pool as the parse endpoints.
    let result = run_parse(&config, &payload_id, move || compute_vcs(payload)).await?;
    match result {
        Ok(response) => Ok(negotiated_ok(&req, &response, payload_id)),
        Err(error) => Ok(vcs_error_response(&error, payload_id)),
    }
}

async fn vcs_trend_json(
    req: actix_web::HttpRequest,
    item: web::Json<WebVcsTrendPayload>,
    config: web::Data<ParseConfig>,
) -> Result<HttpResponse, actix_web::Error> {
    let payload = item.into_inner();
    let payload_id = payload.id.clone();
    // The repeated history walks are blocking I/O, so they run on the same
    // timeout-guarded blocking pool as the other endpoints.
    let result = run_parse(&config, &payload_id, move || compute_vcs_trend(payload)).await?;
    match result {
        Ok(response) => Ok(negotiated_ok(&req, &response, payload_id)),
        Err(error) => Ok(vcs_error_response(&error, payload_id)),
    }
}

async fn vcs_jit_json(
    req: actix_web::HttpRequest,
    item: web::Json<WebVcsJitPayload>,
    config: web::Data<ParseConfig>,
) -> Result<HttpResponse, actix_web::Error> {
    let payload = item.into_inner();
    let payload_id = payload.id.clone();
    // Reject a payload that mixes diff mode with commit-mode fields up front,
    // before the mode branch silently honors `diff` and drops the rest — the
    // two modes are not comparable, so the combination is a client mistake
    // (issue #632). This is a payload-shape error in the web layer, so it
    // 400s directly rather than routing through `vcs::Error`.
    if let Err(message) = payload.validate() {
        return Ok(json_error(
            http::StatusCode::BAD_REQUEST,
            message,
            error_kind::VCS_MODE_CONFLICT,
            payload_id,
        ));
    }
    // Commit scoring is blocking I/O (a history walk); diff scoring is pure
    // CPU. Both run on the same timeout-guarded blocking pool as the other
    // endpoints for one uniform guard.
    let result = run_parse(&config, &payload_id, move || compute_vcs_jit(payload)).await?;
    match result {
        Ok(response) => Ok(negotiated_ok(&req, &response, payload_id)),
        Err(error) => Ok(vcs_error_response(&error, payload_id)),
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
fn vcs_error_response(error: &VcsError, payload_id: String) -> HttpResponse {
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

async fn metrics_plain(
    req: actix_web::HttpRequest,
    body: web::Payload,
    info: Query<WebMetricsInfo>,
    config: web::Data<ParseConfig>,
) -> Result<HttpResponse, actix_web::Error> {
    // Normalise EOL + trailing newline for CLI parity (issue #640); the
    // octet-stream path shares the JSON variant's divergence.
    let buf = normalize_eol(get_code(body, config.max_body_size).await?);
    // Validate the `scope` flag up front so a bad value is a clear `400`
    // regardless of whether the language resolves (#638).
    let scope = match parse_scope_flag(info.scope.as_deref()) {
        Ok(scope) => scope,
        Err(error) => {
            return Ok(json_error(
                http::StatusCode::BAD_REQUEST,
                error,
                error_kind::INVALID_SCOPE_FLAG,
                String::new(),
            ));
        }
    };
    let path = PathBuf::from(&info.file_name);
    let (language, name) = guess_language(&buf, &path);
    if let Some(language) = language {
        // Same `exclude_tests` rationale as the JSON variant above.
        let cfg = WebMetricsCfg::new(String::new(), path, scope, name.to_string());
        let response = run_parse(&config, "", move || {
            compute_metrics(language, buf, cfg).expect(FEATURES_PINNED)
        })
        .await?;
        // Same error mapping as the JSON variant (issue #517); errors use
        // the uniform JSON body even on the octet-stream endpoint (#541).
        match response {
            Some(resp) => Ok(negotiated_ok(&req, &resp, String::new())),
            None => Ok(json_error(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                METRICS_FAILED,
                error_kind::METRICS_FAILED,
                String::new(),
            )),
        }
    } else {
        Ok(unsupported_language(String::new()))
    }
}

async fn function_json(
    req: actix_web::HttpRequest,
    item: web::Json<WebFunctionPayload>,
    config: web::Data<ParseConfig>,
) -> Result<HttpResponse, actix_web::Error> {
    let path = PathBuf::from(&item.file_name);
    let payload = item.into_inner();
    // Function spans are byte/line-position based, so they inherit the same
    // CLI-parity gap as metrics on unterminated input (issue #640).
    let buf = normalize_eol(payload.code.into_bytes());
    let (language, _) = guess_language(&buf, path);
    if let Some(language) = language {
        let payload_id = payload.id.clone();
        let cfg = WebFunctionCfg {
            id: payload.id,
            language: language.name().to_string(),
        };
        let result = run_parse(&config, &payload_id, move || {
            function_spans(language, buf, cfg).expect(FEATURES_PINNED)
        })
        .await?;
        // `function_spans` returns a `serde_json::Value`, so the echoed
        // correlation id comes from the request, not the response body.
        Ok(negotiated_ok(&req, &result, payload_id))
    } else {
        Ok(unsupported_language(payload.id))
    }
}

async fn function_plain(
    req: actix_web::HttpRequest,
    body: web::Payload,
    info: Query<WebFunctionInfo>,
    config: web::Data<ParseConfig>,
) -> Result<HttpResponse, actix_web::Error> {
    // Normalise EOL + trailing newline for CLI parity (issue #640).
    let buf = normalize_eol(get_code(body, config.max_body_size).await?);
    let path = PathBuf::from(&info.file_name);
    let (language, _) = guess_language(&buf, path);
    if let Some(language) = language {
        let cfg = WebFunctionCfg {
            id: String::new(),
            language: language.name().to_string(),
        };
        let result = run_parse(&config, "", move || {
            function_spans(language, buf, cfg).expect(FEATURES_PINNED)
        })
        .await?;
        Ok(negotiated_ok(&req, &result, String::new()))
    } else {
        Ok(unsupported_language(String::new()))
    }
}

async fn ping() -> HttpResponse {
    HttpResponse::Ok().body(())
}

/// Server (`bca-web`) and underlying library versions (#541).
#[derive(Debug, Serialize)]
struct VersionResponse {
    /// Version of the `bca-web` server crate.
    server: &'static str,
    /// Version of the `big-code-analysis` library crate it is built against.
    library: &'static str,
}

/// `GET /v1/version`: reports the server and library versions.
///
/// The server version is this crate's `CARGO_PKG_VERSION`; the library
/// version is sourced from [`big_code_analysis::VERSION`] so the two are
/// reported independently even though they currently share a workspace
/// version.
async fn version() -> HttpResponse {
    HttpResponse::Ok().json(VersionResponse {
        server: env!("CARGO_PKG_VERSION"),
        library: big_code_analysis::VERSION,
    })
}

/// One supported language plus its registered file extensions (#541).
#[derive(Debug, Serialize)]
struct LanguageEntry {
    /// Canonical lowercase language slug (#540).
    name: &'static str,
    /// File extensions that resolve to this language.
    extensions: &'static [&'static str],
}

/// Supported-language listing returned by `GET /v1/languages` (#541).
#[derive(Debug, Serialize)]
struct LanguagesResponse {
    languages: Vec<LanguageEntry>,
}

/// `GET /v1/languages`: lists the supported languages and their file
/// extensions, sourced from the library `LANG` table (#541).
///
/// "Supported" mirrors the Python `supported_languages()` surface: a
/// variant is listed when it (a) has at least one registered file
/// extension — internal helper variants (`Ccomment`, `Preproc`) carry
/// none and are filtered out — and (b) is enabled in the current build.
/// The web crate pins `features = ["all-languages"]`, so (b) is always
/// true here; the `is_enabled` filter keeps the listing honest if that
/// pin is ever loosened. The list is never hardcoded: both the names
/// (`LANG::name`) and the extensions (`LANG::extensions`) come from the
/// enum table.
async fn languages() -> HttpResponse {
    let languages = LANG::into_enum_iter()
        .filter(|lang| !lang.extensions().is_empty())
        .filter(LANG::is_enabled)
        .map(|lang| LanguageEntry {
            name: lang.name(),
            extensions: lang.extensions(),
        })
        .collect();
    HttpResponse::Ok().json(LanguagesResponse { languages })
}

/// One entry in the `GET /v1` route index: a registered path, the HTTP
/// methods it accepts, and a one-line description (#643).
#[derive(Debug, Serialize)]
struct RouteEntry {
    /// Path relative to the API root, including the `/v1` prefix.
    path: &'static str,
    /// HTTP methods this resource accepts.
    methods: &'static [&'static str],
    /// One-line summary of what the route does.
    description: &'static str,
}

/// The machine-readable route index served by `GET /v1` (#643).
///
/// This table is the single source of truth for the index body. Every
/// resource registered in [`register_endpoints`] has exactly one entry
/// here, and `test_web_v1_index_lists_every_registered_route` cross-checks
/// the two so the index cannot silently drift from the routing table when a
/// new endpoint is added. `POST` is listed before the `415`/`405` fallback
/// methods because the analysis endpoints are `POST`-only; the
/// introspection routes (`/v1`, `/v1/ping`, `/v1/version`, `/v1/languages`)
/// accept `GET`/`HEAD` (#644).
const ROUTES: &[RouteEntry] = &[
    RouteEntry {
        path: "/v1",
        methods: &["GET", "HEAD"],
        description: "This route index.",
    },
    RouteEntry {
        path: "/v1/ping",
        methods: &["GET", "HEAD"],
        description: "Liveness check; empty 200 body.",
    },
    RouteEntry {
        path: "/v1/version",
        methods: &["GET", "HEAD"],
        description: "Server and library versions.",
    },
    RouteEntry {
        path: "/v1/languages",
        methods: &["GET", "HEAD"],
        description: "Supported languages and their file extensions.",
    },
    RouteEntry {
        path: "/v1/ast",
        methods: &["POST"],
        description: "Parse source into an abstract syntax tree.",
    },
    RouteEntry {
        path: "/v1/comment",
        methods: &["POST"],
        description: "Remove comments from source code.",
    },
    RouteEntry {
        path: "/v1/function",
        methods: &["POST"],
        description: "Report function spans for the source.",
    },
    RouteEntry {
        path: "/v1/metrics",
        methods: &["POST"],
        description: "Compute maintainability metrics for the source.",
    },
    RouteEntry {
        path: "/v1/vcs",
        methods: &["POST"],
        description: "Rank files by change-history risk over a server-side git repository.",
    },
    RouteEntry {
        path: "/v1/vcs/trend",
        methods: &["POST"],
        description: "Sample change-history metrics across time as a series.",
    },
    RouteEntry {
        path: "/v1/vcs/jit",
        methods: &["POST"],
        description: "Score the just-in-time risk of one commit or an arbitrary diff.",
    },
];

/// Body of the `GET /v1` route index (#643).
#[derive(Debug, Serialize)]
struct RouteIndex {
    /// The server crate (`bca-web`).
    service: &'static str,
    /// The server crate version, matching `GET /v1/version`'s `server`.
    version: &'static str,
    /// Every registered route, sourced from [`ROUTES`].
    routes: &'static [RouteEntry],
}

/// `GET /v1`: machine-readable discovery index of every registered route
/// (#643).
///
/// Returns the [`ROUTES`] table verbatim so clients can enumerate the API
/// surface — path, accepted methods, and a one-line description per route —
/// without scraping the book. Like the other introspection routes it also
/// answers `HEAD` (#644). The former unprefixed `/` alias was removed at
/// 2.0 (#637).
async fn index() -> HttpResponse {
    HttpResponse::Ok().json(RouteIndex {
        service: "bca-web",
        version: env!("CARGO_PKG_VERSION"),
        routes: ROUTES,
    })
}

/// Builds a `GET`/`HEAD` introspection resource at `path`, routed to
/// `handler` (#647).
///
/// This is the single registration helper for every read-only
/// introspection route (`/v1`, `/v1/ping`, `/v1/version`, `/v1/languages`).
/// It is a *forcing function*: a route registered through it cannot omit
/// the [`get_or_head_guard`] (so `HEAD` is answered wherever `GET` is —
/// #644, RFC 9110 §9.3.2) or the [`get_only_method_not_allowed`] fallback
/// (so an unsupported method gets a diagnostic `405` with an `Allow`
/// header — #655). Before this helper the same three lines were hand-copied
/// per resource; a future GET route added with a bare `web::get()` compiled
/// fine and silently `405`'d `HEAD` again — the discipline gap observation
/// #3 of #647 closes.
fn get_resource<F, Args>(path: &str, handler: F) -> actix_web::Resource
where
    F: actix_web::Handler<Args>,
    Args: actix_web::FromRequest + 'static,
    F::Output: actix_web::Responder + 'static,
{
    web::resource(path)
        .route(web::route().guard(get_or_head_guard()).to(handler))
        .default_service(web::route().to(get_only_method_not_allowed))
}

/// Builds the route-index resource at the scope-root `path` (#643).
///
/// `path` is `""` for the `/v1` scope (which matches `/v1`) — actix
/// resolves a scope root only with a scope-appropriately rooted path.
/// GET/HEAD only, with the same `405` fallback as the other introspection
/// resources, via the shared [`get_resource`] forcing function (#647).
/// (The unprefixed `/` alias was removed at 2.0 — #637.)
fn index_resource(path: &str) -> actix_web::Resource {
    get_resource(path, index)
}

/// Runs an HTTP server with the default parse timeout (30 s) and CORS off.
///
/// Convenience wrapper around [`run_with_timeout`]. Each service corresponds
/// to a functionality of the main library and can be accessed through a
/// different route. CORS is disabled ([`CorsPolicy::Disabled`]); callers that
/// need browser cross-origin access pass an explicit policy to
/// [`run_with_timeout`] (#694).
///
/// # Errors
///
/// Returns an error if the server fails to bind or encounters an I/O error.
///
/// # Examples
///
/// ```no_run
/// use big_code_analysis_web::server::run;
///
/// #[actix_web::main]
/// async fn main() {
///     let host = "127.0.0.1";
///     let port = 8080;
///     let num_threads = 4;
///
///     if let Err(e) = run(host, port, num_threads).await {
///        eprintln!("Cannot run the server at {host}:{port}: {e}");
///     }
/// }
/// ```
pub async fn run(host: &str, port: u16, n_threads: usize) -> std::io::Result<()> {
    run_with_timeout(
        host,
        port,
        n_threads,
        DEFAULT_PARSE_TIMEOUT_SECS,
        CorsPolicy::Disabled,
    )
    .await
}

/// Matches a request whose `Content-Type` media type *essence*
/// (type/subtype) equals `expected`, ignoring parameters such as
/// `; charset=utf-8` and ASCII case.
///
/// `guard::Header` matches the raw header byte string exactly, so
/// `application/json; charset=utf-8` — emitted by browsers, Python
/// `requests` with `json=`, and many `fetch` configs — fell through to
/// a bodyless 404 (#515). Parsing the header into a [`mime::Mime`] and
/// comparing the essence accepts those well-formed variants.
fn content_type_essence_matches(ctx: &GuardContext<'_>, expected: &mime::Mime) -> bool {
    // `ContentType` wraps a parsed `Mime`; `mime`'s `Name` comparison is
    // ASCII-case-insensitive, so `APPLICATION/JSON` matches too. A
    // missing or unparseable header yields `None` and fails the guard.
    ctx.header::<ContentType>()
        .is_some_and(|ct| ct.0.type_() == expected.type_() && ct.0.subtype() == expected.subtype())
}

/// Guard accepting any `application/json` body, charset suffix and all.
fn json_guard() -> impl guard::Guard {
    guard::fn_guard(|ctx| content_type_essence_matches(ctx, &mime::APPLICATION_JSON))
}

/// Guard accepting any `application/octet-stream` body, parameters and all.
fn octet_guard() -> impl guard::Guard {
    guard::fn_guard(|ctx| content_type_essence_matches(ctx, &mime::APPLICATION_OCTET_STREAM))
}

/// Guard matching both `GET` and `HEAD`.
///
/// actix-web does not auto-register `HEAD` on a `GET` route, so a bare
/// `web::get()` resource lets `HEAD` fall through to the resource's
/// `default_service` and answer `405` (#644). RFC 9110 §9.3.2 requires
/// `HEAD` wherever `GET` is supported — load balancers and uptime monitors
/// commonly probe with `HEAD`. Routing both methods to the `GET` handler
/// lets actix strip the body for `HEAD` while preserving identical status
/// and headers, and reserves the `405` fallback for genuinely unsupported
/// methods (`POST`, `PUT`, …).
fn get_or_head_guard() -> impl guard::Guard {
    guard::Any(guard::Get()).or(guard::Head())
}

/// Registers every `bca-web` endpoint into `cfg`.
///
/// Each content-type-guarded `POST` resource and the `GET`-only
/// introspection resources (`/ping`, `/version`, `/languages`) carry
/// their *own* `default_service`, so a request that
/// reaches a known resource but matches none of its routes (wrong
/// `Content-Type` or wrong method) is answered with a diagnostic
/// `415`/`405` *by the resource itself*. This is the route table acting
/// as its own source of truth: there is no parallel path constant to
/// keep in sync (the former `GUARDED_POST_PATHS`), so a newly added
/// endpoint can never silently regress to a bodyless `404` (#515).
/// A URL matching no resource at all falls through to the app-level
/// default service in [`configure_routes`], which answers `404`.
///
/// Registered once, under the `/v1` scope (the unprefixed aliases were
/// removed at 2.0 — #637).
fn register_endpoints(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("/ast")
            .route(web::post().guard(json_guard()).to(ast_parser))
            .default_service(web::route().to(guarded_post_fallback)),
    )
    .service(
        web::resource("/comment")
            .route(web::post().guard(json_guard()).to(comment_removal_json))
            .route(web::post().guard(octet_guard()).to(comment_removal_plain))
            .default_service(web::route().to(guarded_post_fallback)),
    )
    .service(
        web::resource("/metrics")
            .route(web::post().guard(json_guard()).to(metrics_json))
            .route(web::post().guard(octet_guard()).to(metrics_plain))
            .default_service(web::route().to(guarded_post_fallback)),
    )
    .service(
        web::resource("/vcs")
            .route(web::post().guard(json_guard()).to(vcs_json))
            // Carry the same per-resource fallback as every other POST
            // endpoint (#515): a wrong `Content-Type` or wrong method on
            // `/vcs` answers a diagnostic 415/405 here rather than
            // falling through to the app-level 404.
            .default_service(web::route().to(guarded_post_fallback)),
    )
    .service(
        // Historical metric trend (issue #333). A distinct resource from
        // `/vcs` (its response is a time series, not a ranked snapshot).
        web::resource("/vcs/trend")
            .route(web::post().guard(json_guard()).to(vcs_trend_json))
            .default_service(web::route().to(guarded_post_fallback)),
    )
    .service(
        // Just-in-time commit (or arbitrary-diff) risk scoring (issues
        // #331 / #580). A distinct resource from `/vcs` (it scores one
        // commit / diff, not a file ranking).
        web::resource("/vcs/jit")
            .route(web::post().guard(json_guard()).to(vcs_jit_json))
            .default_service(web::route().to(guarded_post_fallback)),
    )
    .service(
        web::resource("/function")
            .route(web::post().guard(json_guard()).to(function_json))
            .route(web::post().guard(octet_guard()).to(function_plain))
            .default_service(web::route().to(guarded_post_fallback)),
    )
    // The GET/HEAD introspection resources route through the `get_resource`
    // forcing function (#647) so each carries the GET-or-HEAD guard and the
    // `405` fallback by construction; a future GET route cannot silently
    // regress to a 405 on HEAD by being registered with a bare `web::get()`.
    .service(get_resource("/ping", ping))
    .service(get_resource("/version", version))
    .service(get_resource("/languages", languages));
}

fn json_config() -> web::JsonConfig {
    web::JsonConfig::default()
        .limit(MAX_BODY_SIZE)
        .error_handler(json_error_handler)
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
const SERDE_UNKNOWN_FIELD_MARKER: &str = "unknown field";

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
fn json_error_handler(
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
fn bad_request_kind(message: &str) -> &'static str {
    if message.contains(SERDE_UNKNOWN_FIELD_MARKER) {
        error_kind::UNKNOWN_FIELD
    } else {
        error_kind::BAD_REQUEST
    }
}

/// Builds the `QueryConfig` shared by the query-string endpoints.
///
/// Mirrors [`json_config`]: a malformed or missing query parameter
/// answers `400` with the uniform `{error, id}` body carrying serde's
/// message, instead of actix's default plaintext (#639). Query endpoints
/// carry no correlation id, so `id` is always empty.
fn query_config() -> web::QueryConfig {
    web::QueryConfig::default().error_handler(|err, _req| {
        let message = err.to_string();
        // An unknown query parameter (deny_unknown_fields, #633) gets the
        // specific `unknown_field` token; every other query failure
        // (missing/malformed parameter) is the generic `bad_query`.
        let kind = if message.contains(SERDE_UNKNOWN_FIELD_MARKER) {
            error_kind::UNKNOWN_FIELD
        } else {
            error_kind::BAD_QUERY
        };
        let response = json_error(http::StatusCode::BAD_REQUEST, message, kind, String::new());
        actix_web::error::InternalError::from_response(err, response).into()
    })
}

/// Registers the versioned (`/v1/...`) routes (issue #517).
///
/// Shared by [`run_with_timeout`] and the integration tests so both
/// exercise the same routing, content-type guards, and per-resource
/// fallbacks. The app-level default service answers `404` for any URL
/// that matches no registered resource.
///
/// The original unprefixed aliases (`/metrics`, `/comment`, …) were
/// removed at the 2.0 release cut (#517 / #637): for one cycle they
/// carried `Deprecation: true` / `Sunset` / `Link rel="successor-version"`
/// headers signalling the migration, and they now `404` like any other
/// unknown URL. Clients must use the `/v1` prefix.
fn configure_routes(cfg: &mut web::ServiceConfig) {
    // Install the extractor configs as app-wide data so every JSON / query
    // endpoint renders extractor failures through the uniform `{error, id}`
    // body instead of actix's default plaintext (#639). Set here rather than
    // in `run_with_timeout` so the integration tests (which build the app via
    // this function) exercise the same error handlers production does.
    cfg.app_data(json_config());
    cfg.app_data(query_config());

    // The route index sits at the `/v1` scope root. actix matches a scope
    // root only with an explicitly-rooted resource path: `resource("")`
    // matches `/v1`. It is registered here rather than in the shared
    // `register_endpoints` so the scope can supply its own root path
    // (#643). GET/HEAD only, mirroring the other introspection routes
    // (#644).
    cfg.service(
        web::scope("/v1")
            .service(index_resource(""))
            .configure(register_endpoints),
    );
    cfg.default_service(web::route().to(not_found));
}

/// `Allow` header value advertising the methods of the `POST`-only
/// resources (#655). RFC 9110 §15.5.6 requires a `405` to name the
/// resource's supported methods; the same set answers an `OPTIONS`
/// method-discovery probe.
const POST_ALLOW_METHODS: &str = "POST, OPTIONS";

/// `Allow` header value advertising the methods of the `GET`-only
/// introspection / `/ping` resources (#655). `HEAD` is served wherever
/// `GET` is (#644), so both appear alongside the `OPTIONS` discovery verb.
const GET_ALLOW_METHODS: &str = "GET, HEAD, OPTIONS";

/// Builds the method-fallback response for a resource that advertises
/// `allow_methods` (#655).
///
/// `OPTIONS` is the method-discovery verb, so it answers `204 No Content`
/// with the `Allow` header and no body. Every other unsupported method
/// answers `405 Method Not Allowed` carrying both the `Allow` header
/// (RFC 9110 §15.5.6, a MUST) and the diagnostic JSON `{error, id}` body.
fn method_fallback(
    method: &http::Method,
    allow_methods: &'static str,
    not_allowed_message: &'static str,
) -> HttpResponse {
    let allow = http::header::HeaderValue::from_static(allow_methods);
    if method == http::Method::OPTIONS {
        return HttpResponse::NoContent()
            .insert_header((http::header::ALLOW, allow))
            .finish();
    }
    let mut resp = json_error(
        http::StatusCode::METHOD_NOT_ALLOWED,
        not_allowed_message,
        error_kind::METHOD_NOT_ALLOWED,
        String::new(),
    );
    resp.headers_mut().insert(http::header::ALLOW, allow);
    resp
}

/// Resource-level fallback for the content-type-guarded `POST` endpoints.
///
/// Reached when a request hits a known endpoint but matches none of its
/// routes: a `POST` carrying an unsupported/missing `Content-Type` gets a
/// diagnostic `415`, `OPTIONS` gets a `204` advertising the resource's
/// methods, and any other method gets `405` with an `Allow` header (these
/// endpoints are `POST`-only).
async fn guarded_post_fallback(req: actix_web::HttpRequest) -> HttpResponse {
    if req.method() == http::Method::POST {
        json_error(
            http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Unsupported or missing Content-Type. Send 'application/json' \
             or 'application/octet-stream' (a charset parameter is allowed).",
            error_kind::UNSUPPORTED_MEDIA_TYPE,
            String::new(),
        )
    } else {
        method_fallback(
            req.method(),
            POST_ALLOW_METHODS,
            "Method not allowed. This endpoint accepts POST.",
        )
    }
}

/// Resource-level fallback for the `GET`-only introspection / `/ping`
/// resources.
///
/// `OPTIONS` gets a `204` advertising the resource's methods; any other
/// unsupported method gets `405` with an `Allow` header (#655).
async fn get_only_method_not_allowed(req: actix_web::HttpRequest) -> HttpResponse {
    method_fallback(
        req.method(),
        GET_ALLOW_METHODS,
        "Method not allowed. This endpoint accepts GET.",
    )
}

/// App-level fallback for URLs that match no registered resource.
async fn not_found() -> HttpResponse {
    json_error(
        http::StatusCode::NOT_FOUND,
        "Not found",
        error_kind::NOT_FOUND,
        String::new(),
    )
}

/// Runs an HTTP server with a configurable parse timeout.
///
/// `parse_timeout_secs = 0` disables the deadline (no timeout). Note this
/// also disables the orphaned-task admission gate below: with no deadline a
/// parse never times out, so no task is ever orphaned and the `503`
/// back-pressure never engages. `0` therefore means "no deadline *and* no
/// load-shedding" — defensible as "unlimited", but a deliberate coupling to
/// be aware of (issue #707).
///
/// ## Orphaned-task admission control
///
/// When a parse times out, its blocking thread keeps running on tokio's
/// blocking pool until the work itself completes. To prevent unbounded
/// growth from sustained pathological inputs, new requests are rejected
/// with `503` once the orphan count reaches a soft cap. The cap defaults
/// to `max(n_threads * 2, 4)` and can be overridden by the
/// `BCA_MAX_ORPHANED_TASKS` environment variable (parsed as `usize`;
/// invalid or zero values fall back to the default). A `parse_timeout_secs`
/// of `0` short-circuits this whole mechanism (see above).
///
/// # Errors
///
/// Returns an error if the server fails to bind or encounters an I/O error.
pub async fn run_with_timeout(
    host: &str,
    port: u16,
    n_threads: usize,
    parse_timeout_secs: u64,
    cors: CorsPolicy,
) -> std::io::Result<()> {
    let default_max_orphaned = n_threads.saturating_mul(2).max(4);
    let max_orphaned_tasks = std::env::var("BCA_MAX_ORPHANED_TASKS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(default_max_orphaned);
    let config = web::Data::new(ParseConfig {
        timeout: if parse_timeout_secs == 0 {
            None
        } else {
            Some(Duration::from_secs(parse_timeout_secs))
        },
        semaphore: Arc::new(Semaphore::new(n_threads)),
        orphaned_tasks: Arc::new(AtomicUsize::new(0)),
        max_orphaned_tasks,
        max_body_size: MAX_BODY_SIZE,
    });

    // CORS is off by default (#694): the `from_fn` layer is wrapped only
    // when `--cors` is enabled (`Condition::new`), so the default request
    // path carries no extra middleware. The policy is registered as app
    // data so the `cors_middleware` extractor can read it when enabled.
    let cors_enabled = cors != CorsPolicy::Disabled;
    let cors = web::Data::new(cors);

    HttpServer::new(move || {
        App::new()
            .wrap(Condition::new(
                cors_enabled,
                actix_web::middleware::from_fn(cors_middleware),
            ))
            .wrap(tracing_actix_web::TracingLogger::default())
            .app_data(config.clone())
            .app_data(cors.clone())
            // `JsonConfig` / `QueryConfig` (with the `{error, id}` error
            // handlers) are installed inside `configure_routes` so the
            // integration tests share them (#639).
            .configure(configure_routes)
    })
    .workers(n_threads)
    .bind((host, port))?
    .run()
    .await
}

// curl --header "Content-Type: application/json" --request POST --data '{"id": "1234", "file_name": "prova.cpp", "code": "int x = 1;", "comment": true, "span": true}' http://127.0.0.1:8081/ast

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
#[path = "server_tests.rs"]
mod tests;
