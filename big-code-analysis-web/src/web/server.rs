use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use actix_web::{
    App, HttpResponse, HttpServer, guard, http,
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
}

const PARSE_TIMEOUT: &str = "Parse timed out";

/// Default parse timeout used by [`run`].
pub const DEFAULT_PARSE_TIMEOUT_SECS: u64 = 30;

/// Offloads a CPU-bound parse to the blocking thread pool, bounded by a
/// semaphore and wrapped in a deadline.
///
/// The permit is held for the lifetime of this future (until the task
/// completes or the deadline fires), then released. A timed-out blocking task
/// continues running on the thread pool but no longer holds a permit; its
/// existence is tracked via `config.orphaned_tasks`. When that counter reaches
/// `config.max_orphaned_tasks`, new requests are rejected with 503 rather than
/// spawning another thread that would never be reclaimed in time.
async fn run_parse<T: Send + 'static>(
    config: &web::Data<ParseConfig>,
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, actix_web::Error> {
    // Soft limit: Relaxed ordering means concurrent requests may briefly exceed
    // this threshold before the counter stabilises, but that is acceptable —
    // the check is a heuristic admission gate, not a hard guarantee.
    if config.orphaned_tasks.load(Ordering::Relaxed) >= config.max_orphaned_tasks {
        return Err(actix_web::error::ErrorServiceUnavailable(
            "parse pool saturated",
        ));
    }

    let permit = Arc::clone(&config.semaphore)
        .acquire_owned()
        .await
        .map_err(|_| actix_web::error::ErrorServiceUnavailable("parse pool shut down"))?;

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
                counter.fetch_add(1, Ordering::Relaxed);
                tokio::spawn(async move {
                    let _ = handle.await;
                    counter.fetch_sub(1, Ordering::Relaxed);
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

async fn get_code(mut body: web::Payload) -> Result<Vec<u8>, actix_web::Error> {
    let mut code = BytesMut::new();
    while let Some(item) = body.next().await {
        code.extend_from_slice(&item?);
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
            action::<AstCallback>(&language, buf, Path::new(""), None, cfg)
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
            action::<WebCommentCallback>(&language, buf, Path::new(""), None, cfg)
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
    let buf = get_code(body).await?;
    let path = PathBuf::from(&info.file_name);
    let (language, _) = guess_language(&buf, path);
    if let Some(language) = language {
        let language = comment_language(language);
        let cfg = WebCommentCfg { id: String::new() };
        let res = run_parse(&config, move || {
            action::<WebCommentCallback>(&language, buf, Path::new(""), None, cfg)
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
        let cfg = WebMetricsCfg {
            id: payload.id,
            path,
            unit: payload.unit,
            language: name.to_string(),
        };
        let result = run_parse(&config, move || {
            action::<WebMetricsCallback>(&language, buf, Path::new(""), None, cfg)
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
    let buf = get_code(body).await?;
    let path = PathBuf::from(&info.file_name);
    let (language, name) = guess_language(&buf, &path);
    if let Some(language) = language {
        let cfg = WebMetricsCfg {
            id: String::new(),
            path,
            unit: info.unit.as_ref().is_some_and(|s| {
                s == "1"
                    || s.eq_ignore_ascii_case("true")
                    || s.eq_ignore_ascii_case("yes")
                    || s.eq_ignore_ascii_case("on")
            }),
            language: name.to_string(),
        };
        let result = run_parse(&config, move || {
            action::<WebMetricsCallback>(&language, buf, Path::new(""), None, cfg)
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
            action::<WebFunctionCallback>(&language, buf, Path::new(""), None, cfg)
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
    let buf = get_code(body).await?;
    let path = PathBuf::from(&info.file_name);
    let (language, _) = guess_language(&buf, path);
    if let Some(language) = language {
        let cfg = WebFunctionCfg { id: String::new() };
        let result = run_parse(&config, move || {
            action::<WebFunctionCallback>(&language, buf, Path::new(""), None, cfg)
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
    });

    HttpServer::new(move || {
        App::new()
            .app_data(config.clone())
            .app_data(web::JsonConfig::default().limit(max_size))
            .service(
                web::resource("/ast")
                    .guard(guard::Header("content-type", "application/json"))
                    .app_data(web::Json::<AstPayload>)
                    .route(web::post().to(ast_parser)),
            )
            .service(
                web::resource("/comment")
                    .guard(guard::Header("content-type", "application/json"))
                    .app_data(web::Json::<WebCommentPayload>)
                    .route(web::post().to(comment_removal_json)),
            )
            .service(
                web::resource("/comment")
                    .guard(guard::Header("content-type", "application/octet-stream"))
                    .app_data(web::PayloadConfig::default().limit(max_size))
                    .route(web::post().to(comment_removal_plain)),
            )
            .service(
                web::resource("/metrics")
                    .guard(guard::Header("content-type", "application/json"))
                    .app_data(web::Json::<WebMetricsPayload>)
                    .route(web::post().to(metrics_json)),
            )
            .service(
                web::resource("/metrics")
                    .guard(guard::Header("content-type", "application/octet-stream"))
                    .app_data(web::PayloadConfig::default().limit(max_size))
                    .route(web::post().to(metrics_plain)),
            )
            .service(
                web::resource("/function")
                    .guard(guard::Header("content-type", "application/json"))
                    .app_data(web::Json::<WebFunctionPayload>)
                    .route(web::post().to(function_json)),
            )
            .service(
                web::resource("/function")
                    .guard(guard::Header("content-type", "application/octet-stream"))
                    .app_data(web::PayloadConfig::default().limit(max_size))
                    .route(web::post().to(function_plain)),
            )
            .service(web::resource("/ping").route(web::get().to(ping)))
    })
    .workers(n_threads)
    .bind((host, port))?
    .run()
    .await
}

// curl --header "Content-Type: application/json" --request POST --data '{"id": "1234", "file_name": "prova.cpp", "code": "int x = 1;", "comment": true, "span": true}' http://127.0.0.1:8081/ast

#[cfg(test)]
mod tests {
    use actix_web::web::Bytes;
    use actix_web::{http::StatusCode, http::header::ContentType, test};
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use serde_json::value::Value;

    use super::*;

    fn test_config() -> web::Data<ParseConfig> {
        web::Data::new(ParseConfig {
            timeout: None,
            semaphore: Arc::new(Semaphore::new(4)),
            orphaned_tasks: Arc::new(AtomicUsize::new(0)),
            max_orphaned_tasks: 64,
        })
    }

    fn test_config_with_timeout(d: Duration) -> web::Data<ParseConfig> {
        web::Data::new(ParseConfig {
            timeout: Some(d),
            semaphore: Arc::new(Semaphore::new(4)),
            orphaned_tasks: Arc::new(AtomicUsize::new(0)),
            max_orphaned_tasks: 64,
        })
    }

    async fn assert_error_sanitized(result: Result<String, actix_web::Error>) {
        let err = result.unwrap_err();
        let resp = err.error_response();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
        let body_str = String::from_utf8_lossy(&body);
        assert!(
            !body_str.contains("BlockingError"),
            "response body must not contain BlockingError: {body_str}"
        );
        assert!(
            !body_str.contains("panicked"),
            "response body must not contain panic details: {body_str}"
        );
        assert!(
            !body_str.contains("secret internal detail"),
            "response body must not contain the panic message: {body_str}"
        );
        assert_eq!(body_str, "Internal server error");
    }

    #[actix_rt::test]
    async fn test_web_ping() {
        let app = test::init_service(
            App::new().service(web::resource("/ping").route(web::get().to(ping))),
        )
        .await;
        let req = test::TestRequest::with_uri("/ping").to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_rt::test]
    async fn test_web_ast() {
        let app = test::init_service(
            App::new()
                .app_data(test_config())
                .service(web::resource("/ast").route(web::post().to(ast_parser))),
        )
        .await;
        let req = test::TestRequest::post()
            .uri("/ast")
            .set_json(AstPayload {
                id: "1234".to_string(),
                file_name: "foo.c".to_string(),
                code: "int x = 1;".to_string(),
                comment: false,
                span: true,
            })
            .to_request();

        let res: Value = test::call_and_read_body_json(&app, req).await;
        let expected = json!({
            "id": "1234",
            "root": {
                "Type": "translation_unit",
                "TextValue": "",
                "Span": [1, 1, 1, 11],
                "Children": [
                    {
                        "Type": "declaration",
                        "TextValue": "",
                        "Span": [1, 1, 1, 11],
                        "Children": [
                            {
                                "Type": "primitive_type",
                                "TextValue": "int",
                                "Span": [1, 1, 1, 4],
                                "Children": []
                            },
                            {
                                "Type": "init_declarator",
                                "TextValue": "",
                                "Span": [1, 5, 1, 10],
                                "Children": [
                                    {
                                        "Type": "identifier",
                                        "TextValue": "x",
                                        "Span": [1, 5, 1, 6],
                                        "Children": []
                                    },
                                    {
                                        "Type": "=",
                                        "TextValue": "=",
                                        "Span": [1, 7, 1, 8],
                                        "Children": []
                                    },
                                    {
                                        "Type": "number_literal",
                                        "TextValue": "1",
                                        "Span": [1, 9, 1, 10],
                                        "Children": []
                                    }
                                ]
                            },
                            {
                                "Type": ";",
                                "TextValue": ";",
                                "Span": [1, 10, 1, 11],
                                "Children": []
                            }
                        ]
                    }
                ]
            }
        });
        assert_eq!(res, expected);
    }

    #[actix_rt::test]
    async fn test_web_ast_string() {
        let app = test::init_service(
            App::new()
                .app_data(test_config())
                .service(web::resource("/ast").route(web::post().to(ast_parser))),
        )
        .await;
        let req = test::TestRequest::post()
            .uri("/ast")
            .set_json(AstPayload {
                id: "1234".to_string(),
                file_name: "foo.js".to_string(),
                code: "var x = \"hello world\";".to_string(),
                comment: false,
                span: true,
            })
            .to_request();

        let res: Value = test::call_and_read_body_json(&app, req).await;
        let expected = json!({
            "id": "1234",
            "root": {"Children": [{"Children": [{"Children": [],
                                                 "Span": [1, 1, 1, 4],
                                                 "TextValue": "var",
                                                 "Type": "var"},
                                                {"Children": [{"Children": [],
                                                               "Span": [1, 5, 1, 6],
                                                               "TextValue": "x",
                                                               "Type": "identifier"},
                                                              {"Children": [],
                                                               "Span": [1, 7, 1, 8],
                                                               "TextValue": "=",
                                                               "Type": "="},
                                                              {"Children": [],
                                                               "Span": [1, 9, 1, 22],
                                                               "TextValue": "\"hello world\"",
                                                               "Type": "string"}],
                                                 "Span": [1, 5, 1, 22],
                                                 "TextValue": "",
                                                 "Type": "variable_declarator"},
                                                {"Children": [],
                                                 "Span": [1, 22, 1, 23],
                                                 "TextValue": ";",
                                                 "Type": ";"}],
                                   "Span": [1, 1, 1, 23],
                                   "TextValue": "",
                                   "Type": "variable_declaration"}],
                     "Span": [1, 1, 1, 23],
                     "TextValue": "",
                     "Type": "program"}
        });
        assert_eq!(res, expected);
    }

    #[actix_rt::test]
    async fn test_web_comment_json() {
        let app = test::init_service(
            App::new()
                .app_data(test_config())
                .service(web::resource("/comment").route(web::post().to(comment_removal_json))),
        )
        .await;
        let req = test::TestRequest::post()
            .uri("/comment")
            .set_json(WebCommentPayload {
                id: "1234".to_string(),
                file_name: "foo.c".to_string(),
                code: "int x = 1; // hello".to_string(),
            })
            .to_request();

        let res: Value = test::call_and_read_body_json(&app, req).await;
        let expected = json!({
            "id": "1234",
            "code": b"int x = 1; ",
        });

        assert_eq!(res, expected);
    }

    #[actix_rt::test]
    async fn test_web_comment_json_invalid() {
        let app = test::init_service(
            App::new()
                .app_data(test_config())
                .service(web::resource("/comment").route(web::post().to(comment_removal_json))),
        )
        .await;
        let req = test::TestRequest::post()
            .uri("/comment")
            .set_json(WebCommentPayload {
                id: "1234".to_string(),
                file_name: "foo.unexisting_extension".to_string(),
                code: "int x = 1; // hello".to_string(),
            })
            .to_request();

        let res: Value = test::call_and_read_body_json(&app, req).await;
        let expected = json!({
            "id": "1234",
            "error": INVALID_LANGUAGE,
        });

        assert_eq!(res, expected);
    }

    #[actix_rt::test]
    async fn test_web_comment_json_no_comment() {
        let app = test::init_service(
            App::new()
                .app_data(test_config())
                .service(web::resource("/comment").route(web::post().to(comment_removal_json))),
        )
        .await;
        let req = test::TestRequest::post()
            .uri("/comment")
            .set_json(WebCommentPayload {
                id: "1234".to_string(),
                file_name: "foo.c".to_string(),
                code: "int x = 1;".to_string(),
            })
            .to_request();

        let res: Value = test::call_and_read_body_json(&app, req).await;

        // No comment in the code so the code is null
        let expected = json!({
            "id": "1234",
            "code": (),
        });

        assert_eq!(res, expected);
    }

    #[actix_rt::test]
    async fn test_web_comment_plain() {
        let app = test::init_service(
            App::new()
                .app_data(test_config())
                .service(web::resource("/comment").route(web::post().to(comment_removal_plain))),
        )
        .await;
        let req = test::TestRequest::post()
            .uri("/comment?file_name=foo.c")
            .insert_header(ContentType::plaintext())
            .set_payload("int x = 1; // hello")
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let res = test::read_body(resp).await;
        let expected = Bytes::from_static(b"int x = 1; ");

        assert_eq!(res, expected);
    }

    #[actix_rt::test]
    async fn test_web_comment_plain_invalid() {
        let app = test::init_service(
            App::new()
                .app_data(test_config())
                .service(web::resource("/comment").route(web::post().to(comment_removal_plain))),
        )
        .await;
        let req = test::TestRequest::post()
            .uri("/comment?file_name=foo.unexisting_extension")
            .insert_header(ContentType::plaintext())
            .set_payload("int x = 1; // hello")
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        let res = test::read_body(resp).await;
        let expected = Bytes::from(format!("error: {INVALID_LANGUAGE}"));

        assert_eq!(res, expected);
    }

    #[actix_rt::test]
    async fn test_web_comment_plain_no_comment() {
        let app = test::init_service(
            App::new()
                .app_data(test_config())
                .service(web::resource("/comment").route(web::post().to(comment_removal_plain))),
        )
        .await;
        let req = test::TestRequest::post()
            .uri("/comment?file_name=foo.c")
            .insert_header(ContentType::plaintext())
            .set_payload("int x = 1;")
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let res = test::read_body(resp).await;

        // No comment in the code so the code is empty
        let expected = Bytes::from_static(b"");

        assert_eq!(res, expected);
    }

    // Inspired from https://hg.mozilla.org/mozilla-central/file/9b2a99adc05e53cd4010de512f50118594756650/extensions/java/xpcom/tests/testparams/TestParams.java#l64.
    #[actix_rt::test]
    async fn test_web_comment_plain_bad_chars() {
        let bad_bytes: &[u8] = &[142, 137, 138, 136, 140, 141, 10];
        let input_vec = [b"/*char*/s: ", bad_bytes].concat();
        let output_vec = [b"s: ", bad_bytes].concat();

        let app = test::init_service(
            App::new()
                .app_data(test_config())
                .service(web::resource("/comment").route(web::post().to(comment_removal_plain))),
        )
        .await;
        let req = test::TestRequest::post()
            .uri("/comment?file_name=foo.java")
            .insert_header(ContentType::plaintext())
            .set_payload(input_vec)
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let res = test::read_body(resp).await;

        assert_eq!(res, output_vec);
    }

    #[actix_rt::test]
    async fn test_web_comment_plain_cpp() {
        let app = test::init_service(
            App::new()
                .app_data(test_config())
                .service(web::resource("/comment").route(web::post().to(comment_removal_plain))),
        )
        .await;
        let req = test::TestRequest::post()
            .uri("/comment?file_name=foo.cpp")
            .insert_header(ContentType::plaintext())
            .set_payload("int x = 1; // hello")
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let res = test::read_body(resp).await;
        let expected = Bytes::from_static(b"int x = 1; ");

        assert_eq!(res, expected);
    }

    #[actix_rt::test]
    async fn test_web_metrics_json() {
        let app = test::init_service(
            App::new()
                .app_data(test_config())
                .service(web::resource("/metrics").route(web::post().to(metrics_json))),
        )
        .await;
        let req = test::TestRequest::post()
            .uri("/metrics")
            .set_json(WebMetricsPayload {
                id: "1234".to_string(),
                file_name: "test.py".to_string(),
                code: "# -*- Mode: Objective-C++; tab-width: 2; indent-tabs-mode: nil; c-basic-offset: 2 -*-\n\ndef foo():\n    pass\n".to_string(),
                unit: false,
            })
            .to_request();

        let res: Value = test::call_and_read_body_json(&app, req).await;
        let expected = json!({
            "id": "1234",
            "language": "python",
            "spaces": {"kind": "unit",
                       "start_line": 1,
                       "end_line": 4,
                       "metrics": {"cyclomatic": {"sum": 2.0, "average": 1.0, "min":1.0, "max":1.0, "modified": {"sum": 2.0, "average": 1.0, "min":1.0, "max":1.0}},
                                   "cognitive": {"sum": 0.0, "average": 0.0, "min":0.0, "max":0.0},
                                   "nargs": {"total_functions": 0.0, "average_functions": 0.0, "total_closures": 0.0, "average_closures": 0.0, "total": 0.0, "average": 0.0, "closures_max": 0.0, "closures_min": 0.0, "functions_max": 0.0, "functions_min": 0.0},
                                   "nexits": {"sum": 0.0, "average": 0.0,"min":0.0,"max":0.0},
                                   "halstead": {"bugs": 0.000_942_552_557_372_941_4,
                                                "difficulty": 1.0,
                                                "effort": 4.754_887_502_163_468,
                                                "length": 3.0,
                                                "estimated_program_length": 2.0,
                                                "purity_ratio": 0.666_666_666_666_666_6,
                                                "level": 1.0,
                                                "N2": 1.0,
                                                "N1": 2.0,
                                                "vocabulary": 3.0,
                                                "time": 0.264_160_416_786_859_36,
                                                "n2": 1.0,
                                                "n1": 2.0,
                                                "volume": 4.754_887_502_163_468},
                                   "loc": {"cloc": 1.0, "ploc": 2.0, "lloc": 1.0, "sloc": 4.0, "blank": 1.0, "cloc_average": 0.5, "ploc_average": 1.0, "lloc_average": 0.5, "sloc_average": 2.0, "blank_average": 0.5, "cloc_min": 0.0, "ploc_min": 2.0, "lloc_min": 1.0, "sloc_min": 2.0, "blank_min": 0.0, "cloc_max": 0.0, "ploc_max": 2.0, "lloc_max": 1.0, "sloc_max": 2.0, "blank_max": 0.0},
                                   "nom": {"functions": 1.0, "closures": 0.0, "functions_average": 0.5, "closures_average": 0.0, "total": 1.0, "average": 0.5, "closures_min": 0.0, "closures_max": 0.0, "functions_min": 0.0, "functions_max": 1.0},
                                   "tokens": {"tokens": 6.0, "tokens_average": 3.0, "tokens_min": 0.0, "tokens_max": 6.0},
                                   "mi": {"mi_original": 139.974_331_558_152_1,
                                          "mi_sei": 161.414_455_240_662_22,
                                          "mi_visual_studio": 81.856_334_244_533_39},
                                   "abc": {"assignments": 0.0, "branches": 0.0, "conditions": 0.0, "magnitude": 0.0, "assignments_average": 0.0, "branches_average": 0.0, "conditions_average": 0.0, "assignments_min": 0.0, "assignments_max": 0.0, "branches_min": 0.0, "branches_max": 0.0, "conditions_min": 0.0, "conditions_max": 0.0}},
                       "name": "test.py",
                       "spaces": [{"kind": "function",
                                   "start_line": 3,
                                   "end_line": 4,
                                   "metrics": {"cyclomatic": {"sum": 1.0, "average": 1.0, "min":1.0, "max":1.0, "modified": {"sum": 1.0, "average": 1.0, "min":1.0, "max":1.0}},
                                               "cognitive": {"sum": 0.0, "average": 0.0 , "min":0.0, "max":0.0},
                                               "nargs": {"total_functions": 0.0, "average_functions": 0.0, "total_closures": 0.0, "average_closures": 0.0, "total": 0.0, "average": 0.0, "closures_max": 0.0, "closures_min": 0.0, "functions_max": 0.0, "functions_min": 0.0},
                                               "nexits": {"sum": 0.0, "average": 0.0,"min":0.0,"max":0.0},
                                               "halstead": {"bugs": 0.000_942_552_557_372_941_4,
                                                            "difficulty": 1.0,
                                                            "effort": 4.754_887_502_163_468,
                                                            "length": 3.0,
                                                            "estimated_program_length": 2.0,
                                                            "purity_ratio": 0.666_666_666_666_666_6,
                                                            "level": 1.0,
                                                            "N2": 1.0,
                                                            "N1": 2.0,
                                                            "vocabulary": 3.0,
                                                            "time": 0.264_160_416_786_859_36,
                                                            "n2": 1.0,
                                                            "n1": 2.0,
                                                            "volume": 4.754_887_502_163_468},
                                               "loc": {"cloc": 0.0, "ploc": 2.0, "lloc": 1.0, "sloc": 2.0, "blank": 0.0, "cloc_average": 0.0, "ploc_average": 2.0, "lloc_average": 1.0, "sloc_average": 2.0, "blank_average": 0.0, "cloc_min": 0.0, "ploc_min": 2.0, "lloc_min": 1.0, "sloc_min": 2.0, "blank_min": 0.0, "cloc_max": 0.0, "ploc_max": 2.0, "lloc_max": 1.0, "sloc_max": 2.0, "blank_max": 0.0},
                                               "nom": {"functions": 1.0, "closures": 0.0, "functions_average": 1.0, "closures_average": 0.0, "total": 1.0, "average": 1.0, "closures_min": 0.0, "closures_max": 0.0, "functions_min": 1.0, "functions_max": 1.0},
                                               "tokens": {"tokens": 6.0, "tokens_average": 6.0, "tokens_min": 6.0, "tokens_max": 6.0},
                                               "mi": {"mi_original": 151.433_315_883_223_23,
                                                      "mi_sei": 142.873_061_717_489_78,
                                                      "mi_visual_studio": 88.557_494_668_551_6},
                                               "abc": {"assignments": 0.0, "branches": 0.0, "conditions": 0.0, "magnitude": 0.0, "assignments_average": 0.0, "branches_average": 0.0, "conditions_average": 0.0, "assignments_min": 0.0, "assignments_max": 0.0, "branches_min": 0.0, "branches_max": 0.0, "conditions_min": 0.0, "conditions_max": 0.0}},
                                   "name": "foo",
                                   "spaces": []}]}
        });

        assert_eq!(res, expected);
    }

    #[actix_rt::test]
    async fn test_web_metrics_json_unit() {
        let app = test::init_service(
            App::new()
                .app_data(test_config())
                .service(web::resource("/metrics").route(web::post().to(metrics_json))),
        )
        .await;
        let req = test::TestRequest::post()
            .uri("/metrics")
            .set_json(WebMetricsPayload {
                id: "1234".to_string(),
                file_name: "test.py".to_string(),
                code: "def foo():\n    pass\n".to_string(),
                unit: true,
            })
            .to_request();

        let res: Value = test::call_and_read_body_json(&app, req).await;
        let expected = json!({
            "id": "1234",
            "language": "python",
            "spaces": {"kind": "unit",
                       "start_line": 1,
                       "end_line": 2,
                       "metrics": {"cyclomatic": {"sum": 2.0, "average": 1.0, "min":1.0, "max":1.0, "modified": {"sum": 2.0, "average": 1.0, "min":1.0, "max":1.0}},
                                   "cognitive": {"sum": 0.0, "average": 0.0 , "min":0.0, "max":0.0},
                                   "nargs": {"total_functions": 0.0, "average_functions": 0.0, "total_closures": 0.0, "average_closures": 0.0, "total": 0.0, "average": 0.0, "closures_max": 0.0, "closures_min": 0.0, "functions_max": 0.0, "functions_min": 0.0},
                                   "nexits": {"sum": 0.0, "average": 0.0,"min":0.0,"max":0.0},
                                   "halstead": {"bugs": 0.000_942_552_557_372_941_4,
                                                "difficulty": 1.0,
                                                "effort": 4.754_887_502_163_468,
                                                "length": 3.0,
                                                "estimated_program_length": 2.0,
                                                "purity_ratio": 0.666_666_666_666_666_6,
                                                "level": 1.0,
                                                "N2": 1.0,
                                                "N1": 2.0,
                                                "vocabulary": 3.0,
                                                "time": 0.264_160_416_786_859_36,
                                                "n2": 1.0,
                                                "n1": 2.0,
                                                "volume": 4.754_887_502_163_468},
                                   "loc": {"cloc": 0.0, "ploc": 2.0, "lloc": 1.0, "sloc": 2.0, "blank": 0.0, "cloc_average": 0.0, "ploc_average": 1.0, "lloc_average": 0.5, "sloc_average": 1.0, "blank_average": 0.0, "cloc_min": 0.0, "ploc_min": 2.0, "lloc_min": 1.0, "sloc_min": 2.0, "blank_min": 0.0, "cloc_max": 0.0, "ploc_max": 2.0, "lloc_max": 1.0, "sloc_max": 2.0, "blank_max": 0.0},
                                   "nom": {"functions": 1.0, "closures": 0.0, "functions_average": 0.5, "closures_average": 0.0, "total": 1.0, "average": 0.5, "closures_min": 0.0, "closures_max": 0.0, "functions_min": 0.0, "functions_max": 1.0},
                                   "tokens": {"tokens": 6.0, "tokens_average": 3.0, "tokens_min": 0.0, "tokens_max": 6.0},
                                   "mi": {"mi_original": 151.203_315_883_223_2,
                                          "mi_sei": 142.643_061_717_489_76,
                                          "mi_visual_studio": 88.422_991_744_574_97},
                                   "abc": {"assignments": 0.0, "branches": 0.0, "conditions": 0.0, "magnitude": 0.0, "assignments_average": 0.0, "branches_average": 0.0, "conditions_average": 0.0, "assignments_min": 0.0, "assignments_max": 0.0, "branches_min": 0.0, "branches_max": 0.0, "conditions_min": 0.0, "conditions_max": 0.0}},
                       "name": "test.py",
                       "spaces": []}
        });

        assert_eq!(res, expected);
    }

    #[actix_rt::test]
    async fn test_web_metrics_plain() {
        let app = test::init_service(
            App::new()
                .app_data(test_config())
                .service(web::resource("/metrics").route(web::post().to(metrics_plain))),
        )
        .await;
        let req = test::TestRequest::post()
            .uri("/metrics?file_name=test.py")
            .insert_header(ContentType::plaintext())
            .set_payload("def foo():\n    pass\n")
            .to_request();

        let res: Value = test::call_and_read_body_json(&app, req).await;
        let expected = json!({
            "id": "",
            "language": "python",
            "spaces": {"kind": "unit",
                       "start_line": 1,
                       "end_line": 2,
                       "metrics": {"cyclomatic": {"sum": 2.0, "average": 1.0, "min": 1.0,"max": 1.0, "modified": {"sum": 2.0, "average": 1.0, "min": 1.0,"max": 1.0}},
                                   "cognitive": {"sum": 0.0, "average": 0.0 , "min":0.0, "max":0.0},
                                   "nargs": {"total_functions": 0.0, "average_functions": 0.0, "total_closures": 0.0, "average_closures": 0.0, "total": 0.0, "average": 0.0, "closures_max": 0.0, "closures_min": 0.0, "functions_max": 0.0, "functions_min": 0.0},
                                   "nexits": {"sum": 0.0, "average": 0.0,"min":0.0,"max":0.0},
                                   "halstead": {"bugs": 0.000_942_552_557_372_941_4,
                                                "difficulty": 1.0,
                                                "effort": 4.754_887_502_163_468,
                                                "length": 3.0,
                                                "estimated_program_length": 2.0,
                                                "purity_ratio": 0.666_666_666_666_666_6,
                                                "level": 1.0,
                                                "N2": 1.0,
                                                "N1": 2.0,
                                                "vocabulary": 3.0,
                                                "time": 0.264_160_416_786_859_36,
                                                "n2": 1.0,
                                                "n1": 2.0,
                                                "volume": 4.754_887_502_163_468},
                                   "loc": {"cloc": 0.0, "ploc": 2.0, "lloc": 1.0, "sloc": 2.0, "blank": 0.0, "cloc_average": 0.0, "ploc_average": 1.0, "lloc_average": 0.5, "sloc_average": 1.0, "blank_average": 0.0, "cloc_min": 0.0, "ploc_min": 2.0, "lloc_min": 1.0, "sloc_min": 2.0, "blank_min": 0.0, "cloc_max": 0.0, "ploc_max": 2.0, "lloc_max": 1.0, "sloc_max": 2.0, "blank_max": 0.0},
                                   "nom": {"functions": 1.0, "closures": 0.0, "functions_average": 0.5, "closures_average": 0.0, "total": 1.0, "average": 0.5, "closures_min": 0.0, "closures_max": 0.0, "functions_min": 0.0, "functions_max": 1.0},
                                   "tokens": {"tokens": 6.0, "tokens_average": 3.0, "tokens_min": 0.0, "tokens_max": 6.0},
                                   "mi": {"mi_original": 151.203_315_883_223_2,
                                          "mi_sei": 142.643_061_717_489_76,
                                          "mi_visual_studio": 88.422_991_744_574_97},
                                   "abc": {"assignments": 0.0, "branches": 0.0, "conditions": 0.0, "magnitude": 0.0, "assignments_average": 0.0, "branches_average": 0.0, "conditions_average": 0.0, "assignments_min": 0.0, "assignments_max": 0.0, "branches_min": 0.0, "branches_max": 0.0, "conditions_min": 0.0, "conditions_max": 0.0}},
                       "name": "test.py",
                       "spaces": [{"kind": "function",
                                   "start_line": 1,
                                   "end_line": 2,
                                   "metrics": {"cyclomatic": {"sum": 1.0, "average": 1.0, "min": 1.0,"max": 1.0, "modified": {"sum": 1.0, "average": 1.0, "min": 1.0,"max": 1.0}},
                                               "cognitive": {"sum": 0.0, "average": 0.0 , "min":0.0, "max":0.0},
                                               "nargs": {"total_functions": 0.0, "average_functions": 0.0, "total_closures": 0.0, "average_closures": 0.0, "total": 0.0, "average": 0.0, "closures_max": 0.0, "closures_min": 0.0, "functions_max": 0.0, "functions_min": 0.0},
                                               "nexits": {"sum": 0.0, "average": 0.0,"min":0.0,"max":0.0},
                                               "halstead": {"bugs": 0.000_942_552_557_372_941_4,
                                                            "difficulty": 1.0,
                                                            "effort": 4.754_887_502_163_468,
                                                            "length": 3.0,
                                                            "estimated_program_length": 2.0,
                                                            "purity_ratio": 0.666_666_666_666_666_6,
                                                            "level": 1.0,
                                                            "N2": 1.0,
                                                            "N1": 2.0,
                                                            "vocabulary": 3.0,
                                                            "time": 0.264_160_416_786_859_36,
                                                            "n2": 1.0,
                                                            "n1": 2.0,
                                                            "volume": 4.754_887_502_163_468},
                                               "loc": {"cloc": 0.0, "ploc": 2.0, "lloc": 1.0, "sloc": 2.0, "blank": 0.0, "cloc_average": 0.0, "ploc_average": 2.0, "lloc_average": 1.0, "sloc_average": 2.0, "blank_average": 0.0, "cloc_min": 0.0, "ploc_min": 2.0, "lloc_min": 1.0, "sloc_min": 2.0, "blank_min": 0.0, "cloc_max": 0.0, "ploc_max": 2.0, "lloc_max": 1.0, "sloc_max": 2.0, "blank_max": 0.0},
                                               "nom": {"functions": 1.0, "closures": 0.0, "functions_average": 1.0, "closures_average": 0.0, "total": 1.0, "average": 1.0, "closures_min": 0.0, "closures_max": 0.0, "functions_min": 1.0, "functions_max": 1.0},
                                               "tokens": {"tokens": 6.0, "tokens_average": 6.0, "tokens_min": 6.0, "tokens_max": 6.0},
                                               "mi": {"mi_original": 151.433_315_883_223_23,
                                                      "mi_sei": 142.873_061_717_489_78,
                                                      "mi_visual_studio": 88.557_494_668_551_6},
                                               "abc": {"assignments": 0.0, "branches": 0.0, "conditions": 0.0, "magnitude": 0.0, "assignments_average": 0.0, "branches_average": 0.0, "conditions_average": 0.0, "assignments_min": 0.0, "assignments_max": 0.0, "branches_min": 0.0, "branches_max": 0.0, "conditions_min": 0.0, "conditions_max": 0.0}},
                                   "name": "foo",
                                   "spaces": []}]}
        });

        assert_eq!(res, expected);
    }

    #[actix_rt::test]
    async fn test_web_function_json() {
        let app = test::init_service(
            App::new()
                .app_data(test_config())
                .service(web::resource("/function").route(web::post().to(function_json))),
        )
        .await;
        let req = test::TestRequest::post()
            .uri("/function")
            .set_json(WebCommentPayload {
                id: "1234".to_string(),
                file_name: "test.py".to_string(),
                code: "def foo():\n    pass\n\ndef bar():\n    pass".to_string(),
            })
            .to_request();

        let res: Value = test::call_and_read_body_json(&app, req).await;
        let expected = json!({
            "id": "1234",
            "spans": [
                {
                    "end_line": 2,
                    "error": false,
                    "name": "foo",
                    "start_line": 1
                },
                {
                    "end_line": 5,
                    "error": false,
                    "name": "bar",
                    "start_line": 4
                }
            ]
        });

        assert_eq!(res, expected);
    }

    #[actix_rt::test]
    async fn test_web_function_plain() {
        let app = test::init_service(
            App::new()
                .app_data(test_config())
                .service(web::resource("/function").route(web::post().to(function_plain))),
        )
        .await;
        let req = test::TestRequest::post()
            .uri("/function?file_name=test.py")
            .insert_header(ContentType::plaintext())
            .set_payload("def foo():\n    pass\n\ndef bar():\n    pass")
            .to_request();

        let res: Value = test::call_and_read_body_json(&app, req).await;
        let expected = json!({
            "id": "",
            "spans": [
                {
                    "end_line": 2,
                    "error": false,
                    "name": "foo",
                    "start_line": 1
                },
                {
                    "end_line": 5,
                    "error": false,
                    "name": "bar",
                    "start_line": 4
                }
            ]
        });

        assert_eq!(res, expected);
    }

    #[actix_rt::test]
    async fn test_run_parse_error_does_not_leak_internals() {
        let config = test_config();
        let result = run_parse(&config, || -> String { panic!("secret internal detail") }).await;
        assert_error_sanitized(result).await;
    }

    #[actix_rt::test]
    async fn test_run_parse_error_with_timeout_does_not_leak_internals() {
        let config = test_config_with_timeout(Duration::from_secs(5));
        let result = run_parse(&config, || -> String { panic!("secret internal detail") }).await;
        assert_error_sanitized(result).await;
    }

    #[actix_rt::test]
    async fn test_run_parse_timeout_returns_504() {
        let config = test_config_with_timeout(Duration::from_millis(50));
        // The blocking task outlives the timeout, but exits shortly after to keep
        // the test fast (the cleanup task awaits the JoinHandle).
        let result = run_parse(&config, || {
            std::thread::sleep(Duration::from_millis(200));
            "completed"
        })
        .await;

        let err = result.unwrap_err();
        let resp = err.error_response();
        assert_eq!(resp.status(), StatusCode::GATEWAY_TIMEOUT);
        let body = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
        assert_eq!(String::from_utf8_lossy(&body), PARSE_TIMEOUT);
    }

    #[actix_rt::test]
    async fn test_run_parse_timeout_increments_orphan_counter_and_decrements_on_completion() {
        let orphaned = Arc::new(AtomicUsize::new(0));
        // Use a channel so the blocking task exits quickly after the timeout fires.
        let (tx, rx) = std::sync::mpsc::channel::<()>();

        let config = web::Data::new(ParseConfig {
            timeout: Some(Duration::from_millis(50)),
            semaphore: Arc::new(Semaphore::new(4)),
            orphaned_tasks: Arc::clone(&orphaned),
            max_orphaned_tasks: 64,
        });

        let err = run_parse(&config, move || {
            // Block until the test signals completion.
            let _ = rx.recv();
        })
        .await
        .unwrap_err();
        assert_eq!(err.error_response().status(), StatusCode::GATEWAY_TIMEOUT);

        // Counter must be 1 immediately after timeout.
        assert_eq!(orphaned.load(Ordering::Relaxed), 1);

        // Unblock the orphaned task so it can finish.
        let _ = tx.send(());

        // Poll until the cleanup task has decremented the counter.
        for _ in 0..200 {
            tokio::time::sleep(Duration::from_millis(10)).await;
            if orphaned.load(Ordering::Relaxed) == 0 {
                break;
            }
        }
        assert_eq!(orphaned.load(Ordering::Relaxed), 0);
    }

    #[actix_rt::test]
    async fn test_run_parse_rejects_with_503_when_orphan_threshold_exceeded() {
        // Pre-fill the counter to the threshold.
        let orphaned = Arc::new(AtomicUsize::new(10));
        let config = web::Data::new(ParseConfig {
            timeout: Some(Duration::from_secs(5)),
            semaphore: Arc::new(Semaphore::new(4)),
            orphaned_tasks: Arc::clone(&orphaned),
            max_orphaned_tasks: 10,
        });

        // The closure should never run because the threshold check fires first.
        let result = run_parse(&config, || "should not run").await;
        let err = result.unwrap_err();
        let resp = err.error_response();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
        assert_eq!(String::from_utf8_lossy(&body), "parse pool saturated");
    }
}
