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

/// Default parse timeout used by [`run`].
pub const DEFAULT_PARSE_TIMEOUT_SECS: u64 = 30;

async fn run_parse<T: Send + 'static>(
    config: &web::Data<ParseConfig>,
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
                eprintln!("Parse task failed: {e}");
                Err(actix_web::error::ErrorInternalServerError(
                    "Internal server error",
                ))
            }
            Err(_) => {
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
            eprintln!("Parse task failed: {e}");
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
        let cfg = AstCfg {
            id: payload.id,
            comment: payload.comment,
            span: payload.span,
        };
        let result = run_parse(&config, move || {
            action::<AstCallback>(language, buf, Path::new(""), None, cfg).expect(FEATURES_PINNED)
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

async fn comment_removal_json(
    item: web::Json<WebCommentPayload>,
    config: web::Data<ParseConfig>,
) -> Result<HttpResponse, actix_web::Error> {
    let path = PathBuf::from(&item.file_name);
    let payload = item.into_inner();
    let buf = payload.code.into_bytes();
    let (language, _) = guess_language(&buf, path);
    if let Some(language) = language {
        let cfg = WebCommentCfg { id: payload.id };
        let language = comment_language(language);
        let result = run_parse(&config, move || {
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
        let res = run_parse(&config, move || {
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
        let cfg = WebMetricsCfg::new(payload.id, path, payload.unit, name.to_string());
        let result = run_parse(&config, move || {
            action::<WebMetricsCallback>(language, buf, Path::new(""), None, cfg)
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
        let result = run_parse(&config, move || {
            action::<WebMetricsCallback>(language, buf, Path::new(""), None, cfg)
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

async fn function_json(
    item: web::Json<WebFunctionPayload>,
    config: web::Data<ParseConfig>,
) -> Result<HttpResponse, actix_web::Error> {
    let path = PathBuf::from(&item.file_name);
    let payload = item.into_inner();
    let buf = payload.code.into_bytes();
    let (language, _) = guess_language(&buf, path);
    if let Some(language) = language {
        let cfg = WebFunctionCfg { id: payload.id };
        let result = run_parse(&config, move || {
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
        let result = run_parse(&config, move || {
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

/// Paths served by a content-type-guarded `POST` route.
///
/// Used by the default service to distinguish a content-type mismatch
/// (`415`) on a real endpoint from a genuinely unknown URL (`404`).
const GUARDED_POST_PATHS: [&str; 4] = ["/ast", "/comment", "/metrics", "/function"];

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

/// Registers every `bca-web` route plus the diagnostic default service.
///
/// Shared by [`run_with_timeout`] and the integration tests so both
/// exercise the same content-type guards.
fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("/ast")
            .guard(json_guard())
            .route(web::post().to(ast_parser)),
    )
    .service(
        web::resource("/comment")
            .guard(json_guard())
            .route(web::post().to(comment_removal_json)),
    )
    .service(
        web::resource("/comment")
            .guard(octet_guard())
            .route(web::post().to(comment_removal_plain)),
    )
    .service(
        web::resource("/metrics")
            .guard(json_guard())
            .route(web::post().to(metrics_json)),
    )
    .service(
        web::resource("/metrics")
            .guard(octet_guard())
            .route(web::post().to(metrics_plain)),
    )
    .service(
        web::resource("/function")
            .guard(json_guard())
            .route(web::post().to(function_json)),
    )
    .service(
        web::resource("/function")
            .guard(octet_guard())
            .route(web::post().to(function_plain)),
    )
    .service(web::resource("/ping").route(web::get().to(ping)))
    .default_service(web::route().to(unmatched_route));
}

/// Fallback handler for requests that match no guarded route.
///
/// For a known endpoint, distinguishes *why* it fell through: a wrong
/// HTTP method gets `405 Method Not Allowed` (these endpoints are
/// `POST`-only), while a `POST` carrying the wrong/missing
/// `Content-Type` gets a diagnostic `415 Unsupported Media Type`. A URL
/// that is not a known endpoint gets a plain `404`, so a content-type
/// problem, a method problem, and a wrong URL are each distinguishable.
async fn unmatched_route(req: actix_web::HttpRequest) -> HttpResponse {
    if GUARDED_POST_PATHS.contains(&req.path()) {
        if req.method() == actix_web::http::Method::POST {
            HttpResponse::UnsupportedMediaType().body(
                "Unsupported or missing Content-Type. Send 'application/json' \
                 or 'application/octet-stream' (a charset parameter is allowed).",
            )
        } else {
            HttpResponse::MethodNotAllowed().body("Method not allowed. This endpoint accepts POST.")
        }
    } else {
        HttpResponse::NotFound().body("Not found")
    }
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
