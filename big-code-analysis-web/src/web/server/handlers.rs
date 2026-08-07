//! The HTTP endpoint handlers (`ping`, `version`, `languages`, `index`,
//! the analysis `POST` handlers, and the introspection-index plumbing)
//! plus the request/response data shapes they (de)serialize.
#![allow(clippy::wildcard_imports)]

use super::*;

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

/// Swaps any C-family language (`C` since #721, or a C++ dialect — `Cpp`
/// / the Mozilla fork `Mozcpp`, #720) to the `Ccomment` grammar for
/// comment-removal endpoints.
fn comment_language(language: LANG) -> LANG {
    if matches!(language, LANG::C | LANG::Cpp | LANG::Mozcpp) {
        LANG::Ccomment
    } else {
        language
    }
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

pub(crate) async fn ast_parser(
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
            Ast::parse(Source::from_bytes(language, buf)).map(|ast| ast.dump(cfg))
        })
        .await?
        .map_err(|err| ParseError::from_metrics(&payload_id, err))?;
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

pub(crate) async fn comment_removal_json(
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
            strip_comments(language, buf, cfg)
        })
        .await?
        .map_err(|err| ParseError::from_metrics(&payload_id, err))?;
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

pub(crate) async fn comment_removal_plain(
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
        let res = run_parse(&config, "", move || strip_comments(language, buf, cfg))
            .await?
            .map_err(|err| ParseError::from_metrics("", err))?;
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

pub(crate) async fn metrics_json(
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
            compute_metrics(language, buf, cfg)
        })
        .await?
        .map_err(|err| ParseError::from_metrics(&payload_id, err))?;
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

pub(crate) async fn vcs_json(
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

pub(crate) async fn vcs_trend_json(
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

pub(crate) async fn vcs_jit_json(
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

pub(crate) async fn metrics_plain(
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
        let response = run_parse(&config, "", move || compute_metrics(language, buf, cfg))
            .await?
            .map_err(|err| ParseError::from_metrics("", err))?;
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

pub(crate) async fn function_json(
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
            function_spans(language, buf, cfg)
        })
        .await?
        .map_err(|err| ParseError::from_metrics(&payload_id, err))?;
        // `function_spans` returns a `serde_json::Value`, so the echoed
        // correlation id comes from the request, not the response body.
        Ok(negotiated_ok(&req, &result, payload_id))
    } else {
        Ok(unsupported_language(payload.id))
    }
}

pub(crate) async fn function_plain(
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
        let result = run_parse(&config, "", move || function_spans(language, buf, cfg))
            .await?
            .map_err(|err| ParseError::from_metrics("", err))?;
        Ok(negotiated_ok(&req, &result, String::new()))
    } else {
        Ok(unsupported_language(String::new()))
    }
}

pub(crate) async fn ping() -> HttpResponse {
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
pub(crate) async fn version() -> HttpResponse {
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
/// variant is listed when it (a) is not an internal C-family helper —
/// `Ccomment` / `Preproc` are filtered out — and (b) is enabled in the
/// current build. The predicate is *not* "has a registered extension":
/// since #720 the opt-in `Mozcpp` dialect owns zero extensions yet is a
/// public, name-selectable language, so it is listed (with an empty
/// `extensions` array), keeping parity with the Python bindings. The
/// web crate pins `features = ["all-languages"]`, so (b) is always true
/// here; the `is_enabled` filter keeps the listing honest if that pin is
/// ever loosened. The list is never hardcoded: both the names
/// (`LANG::name`) and the extensions (`LANG::extensions`) come from the
/// enum table.
pub(crate) async fn languages() -> HttpResponse {
    let languages = LANG::into_enum_iter()
        .filter(|lang| !matches!(lang, LANG::Ccomment | LANG::Preproc))
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
pub(crate) struct RouteEntry {
    /// Path relative to the API root, including the `/v1` prefix.
    pub(crate) path: &'static str,
    /// HTTP methods this resource accepts.
    pub(crate) methods: &'static [&'static str],
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
pub(crate) const ROUTES: &[RouteEntry] = &[
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
pub(crate) async fn index() -> HttpResponse {
    HttpResponse::Ok().json(RouteIndex {
        service: "bca-web",
        version: env!("CARGO_PKG_VERSION"),
        routes: ROUTES,
    })
}
