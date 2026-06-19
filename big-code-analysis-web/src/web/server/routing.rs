//! The routing table: content-type guards, the per-resource and
//! app-level fallbacks, the extractor configs, and the
//! [`configure_routes`] entry point the server and the integration tests
//! both build the app from.
#![allow(clippy::wildcard_imports)]

use super::*;

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
pub(crate) fn configure_routes(cfg: &mut web::ServiceConfig) {
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
