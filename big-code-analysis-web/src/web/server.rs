// bca: suppress-file(halstead, nargs, exit, abc, nom)
// Actix server setup + handlers; the file-aggregate halstead/nargs/exit/nom and
// the route-registration closure's abc (one `.service()`/`.route()` per endpoint)
// are declarative many-fn aggregation artifacts, not per-function logic
// complexity. The per-endpoint handlers plus the content-type guard helpers
// (#515) push the file's method count past the per-file nom cap.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use actix_web::{
    App, HttpResponse, HttpServer, guard,
    guard::GuardContext,
    http,
    http::header::ContentType,
    mime,
    web::{self, BytesMut, Query},
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use super::comment::{WebCommentCallback, WebCommentCfg, WebCommentInfo, WebCommentPayload};
use super::function::{WebFunctionCallback, WebFunctionCfg, WebFunctionInfo, WebFunctionPayload};
use super::metrics::{WebMetricsCallback, WebMetricsCfg, WebMetricsInfo, WebMetricsPayload};

use big_code_analysis::{AstCallback, AstCfg, AstPayload, LANG, action, guess_language};

const INVALID_LANGUAGE: &str = "The file extension doesn't correspond to a valid language";

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

/// Default parse timeout used by [`run`].
pub const DEFAULT_PARSE_TIMEOUT_SECS: u64 = 30;

/// Runs `f` on the blocking pool under the timeout / orphan-pool policy.
///
/// `payload_id` is the client-supplied request id from the JSON payload
/// (empty for the octet-stream endpoints, which carry none). It is logged
/// on the failure path under a distinct field name so it does not collide
/// with `tracing-actix-web`'s own span-level `request_id` (a per-request
/// UUID).
async fn run_parse<T: Send + 'static>(
    config: &web::Data<ParseConfig>,
    payload_id: &str,
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, actix_web::Error> {
    // Reject when the orphaned-task pool has saturated. `Acquire` pairs with
    // the `AcqRel` RMW ops on the timeout path so newly admitted requests
    // observe orphan counts published by any prior orphaning task.
    let pool_saturated =
        || config.orphaned_tasks.load(Ordering::Acquire) >= config.max_orphaned_tasks;
    let saturated_err = || actix_web::error::ErrorServiceUnavailable(PARSE_POOL_SATURATED);

    // Fast-path admission check: cheap rejection before acquiring a semaphore
    // permit. A burst of concurrent requests may still pass this check while
    // the counter is briefly low, so the post-admission re-check below is the
    // hard gate.
    if pool_saturated() {
        return Err(saturated_err());
    }

    let permit = Arc::clone(&config.semaphore)
        .acquire_owned()
        .await
        .map_err(|_| actix_web::error::ErrorServiceUnavailable("parse pool shut down"))?;

    // Re-check after semaphore admission. A queued burst can all pass the
    // pre-admission check while the orphan count is still low, then drain the
    // semaphore one at a time. Without this second check each admitted request
    // would spawn another blocking task and grow the orphan pool past the cap.
    // `permit` is dropped by RAII on early return, returning its slot to the
    // semaphore.
    if pool_saturated() {
        return Err(saturated_err());
    }

    let mut handle = tokio::task::spawn_blocking(f);

    let result = if let Some(deadline) = config.timeout {
        match tokio::time::timeout(deadline, &mut handle).await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(e)) => {
                // Log the full error server-side for ops diagnostics; the
                // client only sees the generic "Internal server error" string.
                tracing::error!(payload_id = %payload_id, error = %e, "Parse task failed");
                Err(actix_web::error::ErrorInternalServerError(
                    "Internal server error",
                ))
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
                Err(actix_web::error::ErrorGatewayTimeout(PARSE_TIMEOUT))
            }
        }
    } else {
        handle.await.map_err(|e| {
            tracing::error!(payload_id = %payload_id, error = %e, "Parse task failed");
            actix_web::error::ErrorInternalServerError("Internal server error")
        })
    };
    drop(permit);
    result
}

#[derive(Debug, Deserialize, Serialize)]
struct Error {
    id: String,
    error: &'static str,
}

/// Drains a streaming request body into a byte buffer, enforcing `max_size`.
///
/// The `web::Payload` extractor ignores `web::PayloadConfig`, so the size
/// limit must be enforced here. The running total is checked against
/// `max_size` *before* each chunk is appended, so an oversized body is
/// rejected with 413 as soon as it would exceed the limit rather than being
/// fully buffered first. A body whose total length equals `max_size` is
/// accepted; one byte over is rejected.
async fn get_code(mut body: web::Payload, max_size: usize) -> Result<Vec<u8>, actix_web::Error> {
    let mut code = BytesMut::new();
    while let Some(item) = body.next().await {
        let chunk = item?;
        if code.len() + chunk.len() > max_size {
            return Err(actix_web::error::ErrorPayloadTooLarge(PAYLOAD_TOO_LARGE));
        }
        code.extend_from_slice(&chunk);
    }

    Ok(code.to_vec())
}

async fn ast_parser(
    item: web::Json<AstPayload>,
    config: web::Data<ParseConfig>,
) -> Result<HttpResponse, actix_web::Error> {
    let path = PathBuf::from(&item.file_name);
    let payload = item.into_inner();
    let buf = payload.code.into_bytes();
    let (language, _) = guess_language(&buf, path);
    if let Some(language) = language {
        // Clone the (small) correlation id for server-side log correlation;
        // the original is moved into `cfg` and consumed by `action` below.
        let payload_id = payload.id.clone();
        let cfg = AstCfg {
            id: payload.id,
            comment: payload.comment,
            span: payload.span,
        };
        let result = run_parse(&config, &payload_id, move || {
            action::<AstCallback>(language, buf, Path::new(""), None, cfg).expect(FEATURES_PINNED)
        })
        .await?;
        // `root == None` previously surfaced as a `200` carrying
        // `root: null` (an error signalled inside a success body); map it
        // to an explicit `500` with an error body instead (issue #517).
        if result.root.is_some() {
            Ok(HttpResponse::Ok().json(result))
        } else {
            Ok(HttpResponse::InternalServerError().json(Error {
                id: result.id,
                error: AST_BUILD_FAILED,
            }))
        }
    } else {
        Ok(HttpResponse::NotFound().json(Error {
            id: payload.id,
            error: INVALID_LANGUAGE,
        }))
    }
}

async fn comment_removal_json(
    item: web::Json<WebCommentPayload>,
    config: web::Data<ParseConfig>,
) -> Result<HttpResponse, actix_web::Error> {
    let path = PathBuf::from(&item.file_name);
    let payload = item.into_inner();
    let buf = payload.code.into_bytes();
    let (language, _) = guess_language(&buf, path);
    if let Some(language) = language {
        let payload_id = payload.id.clone();
        let cfg = WebCommentCfg { id: payload.id };
        let language = comment_language(language);
        let result = run_parse(&config, &payload_id, move || {
            action::<WebCommentCallback>(language, buf, Path::new(""), None, cfg)
                .expect(FEATURES_PINNED)
        })
        .await?;
        Ok(HttpResponse::Ok().json(result))
    } else {
        Ok(HttpResponse::NotFound().json(Error {
            id: payload.id,
            error: INVALID_LANGUAGE,
        }))
    }
}

async fn comment_removal_plain(
    body: web::Payload,
    info: Query<WebCommentInfo>,
    config: web::Data<ParseConfig>,
) -> Result<HttpResponse, actix_web::Error> {
    let buf = get_code(body, config.max_body_size).await?;
    let path = PathBuf::from(&info.file_name);
    let (language, _) = guess_language(&buf, path);
    if let Some(language) = language {
        let language = comment_language(language);
        let cfg = WebCommentCfg { id: String::new() };
        // The octet-stream variants carry no request id in the body, so log
        // correlation falls back to the `TracingLogger` request span.
        let res = run_parse(&config, "", move || {
            action::<WebCommentCallback>(language, buf, Path::new(""), None, cfg)
                .expect(FEATURES_PINNED)
        })
        .await?;
        if let Some(res_code) = res.code {
            Ok(HttpResponse::Ok()
                .append_header((http::header::CONTENT_TYPE, "application/octet-stream"))
                .body(res_code))
        } else {
            Ok(HttpResponse::NoContent()
                .append_header((http::header::CONTENT_TYPE, "application/octet-stream"))
                .body(()))
        }
    } else {
        Ok(HttpResponse::NotFound()
            .append_header((http::header::CONTENT_TYPE, "text/plain"))
            .body(format!("error: {INVALID_LANGUAGE}")))
    }
}

async fn metrics_json(
    item: web::Json<WebMetricsPayload>,
    config: web::Data<ParseConfig>,
) -> Result<HttpResponse, actix_web::Error> {
    let path = PathBuf::from(&item.file_name);
    let payload = item.into_inner();
    let buf = payload.code.into_bytes();
    let (language, name) = guess_language(&buf, &path);
    if let Some(language) = language {
        // `exclude_tests` defaults to `false` via `WebMetricsCfg::new`,
        // preserving the pre-#182 numbers for every existing REST
        // client. A future change can thread the flag through the
        // request payload and chain `.with_exclude_tests(...)` here.
        let payload_id = payload.id.clone();
        let cfg = WebMetricsCfg::new(payload.id, path, payload.unit, name.to_string());
        let response = run_parse(&config, &payload_id, move || {
            action::<WebMetricsCallback>(language, buf, Path::new(""), None, cfg)
                .expect(FEATURES_PINNED)
        })
        .await?;
        // `None` means metric computation failed: answer with an explicit
        // `500` instead of the former `200`-with-`spaces: null` (issue #517).
        match response {
            Some(resp) => Ok(HttpResponse::Ok().json(resp)),
            None => Ok(HttpResponse::InternalServerError().json(Error {
                id: payload_id,
                error: METRICS_FAILED,
            })),
        }
    } else {
        Ok(HttpResponse::NotFound().json(Error {
            id: payload.id,
            error: INVALID_LANGUAGE,
        }))
    }
}

async fn metrics_plain(
    body: web::Payload,
    info: Query<WebMetricsInfo>,
    config: web::Data<ParseConfig>,
) -> Result<HttpResponse, actix_web::Error> {
    let buf = get_code(body, config.max_body_size).await?;
    let path = PathBuf::from(&info.file_name);
    let (language, name) = guess_language(&buf, &path);
    if let Some(language) = language {
        let unit = info.unit.as_ref().is_some_and(|s| {
            s == "1"
                || s.eq_ignore_ascii_case("true")
                || s.eq_ignore_ascii_case("yes")
                || s.eq_ignore_ascii_case("on")
        });
        // Same `exclude_tests` rationale as the JSON variant above.
        let cfg = WebMetricsCfg::new(String::new(), path, unit, name.to_string());
        let response = run_parse(&config, "", move || {
            action::<WebMetricsCallback>(language, buf, Path::new(""), None, cfg)
                .expect(FEATURES_PINNED)
        })
        .await?;
        // Same error mapping as the JSON variant (issue #517); the plain
        // endpoint reports failure as a `text/plain` body to match its
        // invalid-language response.
        match response {
            Some(resp) => Ok(HttpResponse::Ok().json(resp)),
            None => Ok(HttpResponse::InternalServerError()
                .append_header((http::header::CONTENT_TYPE, "text/plain"))
                .body(format!("error: {METRICS_FAILED}"))),
        }
    } else {
        Ok(HttpResponse::NotFound()
            .append_header((http::header::CONTENT_TYPE, "text/plain"))
            .body(format!("error: {INVALID_LANGUAGE}")))
    }
}

async fn function_json(
    item: web::Json<WebFunctionPayload>,
    config: web::Data<ParseConfig>,
) -> Result<HttpResponse, actix_web::Error> {
    let path = PathBuf::from(&item.file_name);
    let payload = item.into_inner();
    let buf = payload.code.into_bytes();
    let (language, _) = guess_language(&buf, path);
    if let Some(language) = language {
        let payload_id = payload.id.clone();
        let cfg = WebFunctionCfg { id: payload.id };
        let result = run_parse(&config, &payload_id, move || {
            action::<WebFunctionCallback>(language, buf, Path::new(""), None, cfg)
                .expect(FEATURES_PINNED)
        })
        .await?;
        Ok(HttpResponse::Ok().json(result))
    } else {
        Ok(HttpResponse::NotFound().json(Error {
            id: payload.id,
            error: INVALID_LANGUAGE,
        }))
    }
}

async fn function_plain(
    body: web::Payload,
    info: Query<WebFunctionInfo>,
    config: web::Data<ParseConfig>,
) -> Result<HttpResponse, actix_web::Error> {
    let buf = get_code(body, config.max_body_size).await?;
    let path = PathBuf::from(&info.file_name);
    let (language, _) = guess_language(&buf, path);
    if let Some(language) = language {
        let cfg = WebFunctionCfg { id: String::new() };
        let result = run_parse(&config, "", move || {
            action::<WebFunctionCallback>(language, buf, Path::new(""), None, cfg)
                .expect(FEATURES_PINNED)
        })
        .await?;
        Ok(HttpResponse::Ok().json(result))
    } else {
        Ok(HttpResponse::NotFound()
            .append_header((http::header::CONTENT_TYPE, "text/plain"))
            .body(format!("error: {INVALID_LANGUAGE}")))
    }
}

async fn ping() -> HttpResponse {
    HttpResponse::Ok().body(())
}

/// Runs an HTTP server with the default parse timeout (30 s).
///
/// Convenience wrapper around [`run_with_timeout`]. Each service corresponds
/// to a functionality of the main library and can be accessed through a
/// different route.
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
    run_with_timeout(host, port, n_threads, DEFAULT_PARSE_TIMEOUT_SECS).await
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

/// Registers every `bca-web` endpoint into `cfg`.
///
/// Each content-type-guarded `POST` resource and the `GET`-only `/ping`
/// resource carry their *own* `default_service`, so a request that
/// reaches a known resource but matches none of its routes (wrong
/// `Content-Type` or wrong method) is answered with a diagnostic
/// `415`/`405` *by the resource itself*. This is the route table acting
/// as its own source of truth: there is no parallel path constant to
/// keep in sync (the former `GUARDED_POST_PATHS`), so a newly added
/// endpoint can never silently regress to a bodyless `404` (#515).
/// A URL matching no resource at all falls through to the app-level
/// default service in [`configure_routes`], which answers `404`.
///
/// Shared by the root registration (the deprecated unprefixed aliases)
/// and the `/v1` scope, so both expose identical routing.
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
        web::resource("/function")
            .route(web::post().guard(json_guard()).to(function_json))
            .route(web::post().guard(octet_guard()).to(function_plain))
            .default_service(web::route().to(guarded_post_fallback)),
    )
    .service(
        web::resource("/ping")
            .route(web::get().to(ping))
            .default_service(web::route().to(ping_method_not_allowed)),
    );
}

/// Registers the versioned (`/v1/...`) routes plus, for one deprecation
/// cycle, the original unprefixed paths as aliases (issue #517).
///
/// Shared by [`run_with_timeout`] and the integration tests so both
/// exercise the same routing, content-type guards, and per-resource
/// fallbacks. The app-level default service answers `404` for any URL
/// that matches no registered resource under either prefix.
fn configure_routes(cfg: &mut web::ServiceConfig) {
    // Deprecated unprefixed aliases, kept for one release cycle.
    register_endpoints(cfg);
    cfg.service(web::scope("/v1").configure(register_endpoints))
        .default_service(web::route().to(not_found));
}

/// Resource-level fallback for the content-type-guarded `POST` endpoints.
///
/// Reached when a request hits a known endpoint but matches none of its
/// routes: a `POST` carrying an unsupported/missing `Content-Type` gets a
/// diagnostic `415`, any other method gets `405` (these endpoints are
/// `POST`-only).
async fn guarded_post_fallback(req: actix_web::HttpRequest) -> HttpResponse {
    if req.method() == http::Method::POST {
        HttpResponse::UnsupportedMediaType().body(
            "Unsupported or missing Content-Type. Send 'application/json' \
             or 'application/octet-stream' (a charset parameter is allowed).",
        )
    } else {
        HttpResponse::MethodNotAllowed().body("Method not allowed. This endpoint accepts POST.")
    }
}

/// Resource-level fallback for `/ping`, which only accepts `GET`.
async fn ping_method_not_allowed() -> HttpResponse {
    HttpResponse::MethodNotAllowed().body("Method not allowed. This endpoint accepts GET.")
}

/// App-level fallback for URLs that match no registered resource.
async fn not_found() -> HttpResponse {
    HttpResponse::NotFound().body("Not found")
}

/// Runs an HTTP server with a configurable parse timeout.
///
/// `parse_timeout_secs = 0` disables the deadline (no timeout).
///
/// ## Orphaned-task admission control
///
/// When a parse times out, its blocking thread keeps running on tokio's
/// blocking pool until the work itself completes. To prevent unbounded
/// growth from sustained pathological inputs, new requests are rejected
/// with `503` once the orphan count reaches a soft cap. The cap defaults
/// to `max(n_threads * 2, 4)` and can be overridden by the
/// `BCA_MAX_ORPHANED_TASKS` environment variable (parsed as `usize`;
/// invalid or zero values fall back to the default).
///
/// # Errors
///
/// Returns an error if the server fails to bind or encounters an I/O error.
pub async fn run_with_timeout(
    host: &str,
    port: u16,
    n_threads: usize,
    parse_timeout_secs: u64,
) -> std::io::Result<()> {
    let max_size = 1_024 * 1_024 * 4;
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
        max_body_size: max_size,
    });

    HttpServer::new(move || {
        App::new()
            .wrap(tracing_actix_web::TracingLogger::default())
            .app_data(config.clone())
            .app_data(web::JsonConfig::default().limit(max_size))
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
