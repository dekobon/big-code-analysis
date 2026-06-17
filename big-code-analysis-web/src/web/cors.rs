//! Opt-in Cross-Origin Resource Sharing (CORS) support (#694).
//!
//! CORS is **off by default**: a `bca-web` instance never emits
//! `Access-Control-Allow-Origin` unless the operator passes `--cors`.
//! Leaving it off keeps a localhost daemon's repository paths and metrics
//! invisible to any website the operator happens to be browsing.
//!
//! When enabled, the policy is an *explicit allow-list*: only the origins
//! named on the command line receive CORS headers, and a wide-open `*` is
//! emitted solely when the operator deliberately passes it (`--cors '*'`).
//! The API has no auth / cookies, so `Access-Control-Allow-Credentials` is
//! never set — which also keeps the `*` origin valid (a wildcard origin
//! plus credentials is forbidden by the Fetch standard).
//!
//! The middleware layers *on top of* the existing RFC 9110 `OPTIONS` → 204
//! + `Allow` handling (#655): it reads the response's own `Allow` header to
//! populate `Access-Control-Allow-Methods`, so a preflight `OPTIONS` request
//! is answered by the same per-resource fallback the non-CORS server uses,
//! merely decorated with the CORS headers. There is no separate preflight
//! route to keep in sync with the routing table.

use actix_web::body::MessageBody;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::http::header::{self, HeaderName, HeaderValue};
use actix_web::middleware::Next;
use actix_web::{Error, web};

/// `Access-Control-Allow-Headers` value emitted when a preflight request
/// carries no `Access-Control-Request-Headers` of its own.
///
/// Covers the headers the API's own clients send: `Content-Type` (every
/// `POST` endpoint is content-type-guarded) plus `Accept`. A preflight that
/// *does* list requested headers has them echoed back verbatim instead, so
/// this static set is only the fallback for a bare probe.
const DEFAULT_ALLOW_HEADERS: &str = "Content-Type, Accept";

/// Parsed `--cors` policy (#694).
///
/// [`Disabled`](CorsPolicy::Disabled) is the default and the safe state: no
/// CORS headers are emitted at all. The other two variants are only reached
/// when the operator passes `--cors`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorsPolicy {
    /// No `--cors` flag: the server emits no CORS headers (default).
    Disabled,
    /// `--cors '*'`: the operator explicitly opted into a wide-open policy;
    /// every origin receives `Access-Control-Allow-Origin: *`.
    Wildcard,
    /// `--cors <origin[,origin…]>`: only the listed origins receive CORS
    /// headers, and the matched origin is echoed back (never `*`).
    AllowList(Vec<String>),
}

impl CorsPolicy {
    /// Parses the raw `--cors` argument into a policy.
    ///
    /// `None` (flag absent) is [`CorsPolicy::Disabled`]. A literal `*` —
    /// alone or anywhere in the comma-separated list — is the explicit
    /// wide-open opt-in and collapses to [`CorsPolicy::Wildcard`]. Otherwise
    /// the comma-separated origins are trimmed and the empty entries
    /// dropped, yielding an [`CorsPolicy::AllowList`]; an argument that is
    /// empty or only separators also disables CORS rather than allow-listing
    /// nothing.
    pub fn parse(raw: Option<&str>) -> Self {
        let Some(raw) = raw else {
            return CorsPolicy::Disabled;
        };
        let origins: Vec<String> = raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        if origins.iter().any(|o| o == "*") {
            CorsPolicy::Wildcard
        } else if origins.is_empty() {
            CorsPolicy::Disabled
        } else {
            CorsPolicy::AllowList(origins)
        }
    }

    /// Resolves the `Access-Control-Allow-Origin` value for a request whose
    /// `Origin` header is `request_origin` (`None` when the request carried
    /// no `Origin`).
    ///
    /// Returns the header value to emit, or `None` when the request must
    /// receive no CORS headers — either because the policy is
    /// [`Disabled`](CorsPolicy::Disabled), the request is same-origin (no
    /// `Origin` header), or the origin is not on the allow-list. A
    /// [`Wildcard`](CorsPolicy::Wildcard) policy answers `*`; an allow-list
    /// echoes the matched origin verbatim so the response is specific to the
    /// caller.
    fn allow_origin(&self, request_origin: Option<&str>) -> Option<AllowOrigin> {
        match self {
            CorsPolicy::Disabled => None,
            CorsPolicy::Wildcard => Some(AllowOrigin::Wildcard),
            CorsPolicy::AllowList(origins) => {
                let origin = request_origin?;
                origins
                    .iter()
                    .any(|allowed| allowed == origin)
                    .then(|| AllowOrigin::Echo(origin.to_owned()))
            }
        }
    }

    /// Whether a response's CORS shape depends on the request `Origin`.
    ///
    /// Only [`AllowList`](CorsPolicy::AllowList) is origin-dependent: it
    /// emits `Access-Control-Allow-Origin` for listed origins and nothing
    /// for the rest, so *every* response under the policy — matched,
    /// unmatched, or same-origin — must carry `Vary: Origin` for a shared
    /// cache to key on it (RFC 9110 §12.5.5). [`Wildcard`](CorsPolicy::Wildcard)
    /// answers `*` identically regardless of origin and
    /// [`Disabled`](CorsPolicy::Disabled) emits no CORS headers, so neither
    /// needs `Vary`.
    fn varies_on_origin(&self) -> bool {
        matches!(self, CorsPolicy::AllowList(_))
    }
}

/// Resolved `Access-Control-Allow-Origin` decision for one request.
#[derive(Debug)]
enum AllowOrigin {
    /// Emit the literal `*` (wildcard policy).
    Wildcard,
    /// Echo the request's own origin (allow-list match).
    Echo(String),
}

/// CORS request fields read off the [`ServiceRequest`] before it is consumed
/// by the inner service.
struct CorsRequest {
    /// The request's `Origin` header, if any.
    origin: Option<String>,
    /// The preflight `Access-Control-Request-Headers`, echoed back verbatim.
    request_headers: Option<String>,
}

impl CorsRequest {
    fn extract(req: &ServiceRequest) -> Self {
        let header_str = |name: HeaderName| {
            req.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        CorsRequest {
            origin: header_str(header::ORIGIN),
            request_headers: header_str(header::ACCESS_CONTROL_REQUEST_HEADERS),
        }
    }
}

/// Decorates `res` with the CORS response headers for the resolved
/// `allow_origin` decision.
///
/// `Access-Control-Allow-Methods` is sourced from the response's own `Allow`
/// header (set by the RFC 9110 `OPTIONS`/`405` fallback, #655) so the
/// advertised methods always match the resource's real routing table. No
/// `Access-Control-Allow-Credentials` is ever emitted.
fn apply_cors_headers<B>(
    res: &mut ServiceResponse<B>,
    allow_origin: AllowOrigin,
    request_headers: Option<&str>,
) {
    let headers = res.headers_mut();
    match allow_origin {
        AllowOrigin::Wildcard => {
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"),
            );
        }
        AllowOrigin::Echo(origin) => {
            if let Ok(value) = HeaderValue::from_str(&origin) {
                headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
            }
        }
    }

    // Mirror the `Allow` header the OPTIONS/405 fallback already set so the
    // preflight advertises exactly the resource's real methods.
    if let Some(allow) = headers.get(header::ALLOW).cloned() {
        headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS, allow);
    }

    // Echo the requested headers, or fall back to the documented static set
    // for a bare preflight probe.
    let allow_headers = request_headers
        .and_then(|h| HeaderValue::from_str(h).ok())
        .unwrap_or_else(|| HeaderValue::from_static(DEFAULT_ALLOW_HEADERS));
    headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, allow_headers);
}

/// `from_fn` CORS middleware (#694).
///
/// Reads the active [`CorsPolicy`] from app data (installed by
/// `run_with_timeout` only when `--cors` is enabled, so the default request
/// path never carries this layer) and decorates the response with the CORS
/// headers the policy resolves for the request's `Origin`. A free generic
/// async fn — rather than a stateful closure — so it satisfies `from_fn`'s
/// `Fn(ServiceRequest, Next<B>)` bound for any response body type `B`.
///
/// # Errors
///
/// Propagates any error the inner service (`next.call`) returns; this layer
/// adds no failure mode of its own.
pub async fn cors_middleware<B: MessageBody>(
    policy: web::Data<CorsPolicy>,
    req: ServiceRequest,
    next: Next<B>,
) -> Result<ServiceResponse<B>, Error> {
    let cors_req = CorsRequest::extract(&req);
    let allow_origin = policy.allow_origin(cors_req.origin.as_deref());
    let mut res = next.call(req).await?;
    // Under an allow-list the response shape depends on `Origin` whether or
    // not it matched, so a shared cache must key on it for *every* response —
    // not just the matched ones `apply_cors_headers` decorates (RFC 9110
    // §12.5.5). `append` rather than `insert` so any `Vary` an inner handler
    // set is preserved.
    if policy.varies_on_origin() {
        res.headers_mut()
            .append(header::VARY, HeaderValue::from_static("Origin"));
    }
    if let Some(allow_origin) = allow_origin {
        apply_cors_headers(&mut res, allow_origin, cors_req.request_headers.as_deref());
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_absent_is_disabled() {
        assert_eq!(CorsPolicy::parse(None), CorsPolicy::Disabled);
    }

    #[test]
    fn parse_empty_is_disabled() {
        // An empty value or one that is only separators allow-lists nothing,
        // which is indistinguishable from off — collapse it to Disabled so
        // the operator is never silently left with a no-op allow-list.
        assert_eq!(CorsPolicy::parse(Some("")), CorsPolicy::Disabled);
        assert_eq!(CorsPolicy::parse(Some("  , ,")), CorsPolicy::Disabled);
    }

    #[test]
    fn parse_wildcard() {
        assert_eq!(CorsPolicy::parse(Some("*")), CorsPolicy::Wildcard);
        // A `*` anywhere in the list wins — the operator opted wide open.
        assert_eq!(
            CorsPolicy::parse(Some("https://a.example, *")),
            CorsPolicy::Wildcard
        );
    }

    #[test]
    fn parse_allow_list_trims_and_drops_empties() {
        assert_eq!(
            CorsPolicy::parse(Some(" https://a.example , ,https://b.example ")),
            CorsPolicy::AllowList(vec![
                "https://a.example".to_owned(),
                "https://b.example".to_owned(),
            ])
        );
    }

    #[test]
    fn allow_origin_disabled_emits_nothing() {
        let policy = CorsPolicy::Disabled;
        assert!(policy.allow_origin(Some("https://a.example")).is_none());
    }

    #[test]
    fn allow_origin_wildcard_always_matches() {
        let policy = CorsPolicy::Wildcard;
        assert!(matches!(
            policy.allow_origin(Some("https://a.example")),
            Some(AllowOrigin::Wildcard)
        ));
        // Wildcard answers even a same-origin (no Origin header) request,
        // though browsers only act on the header for cross-origin reads.
        assert!(matches!(
            policy.allow_origin(None),
            Some(AllowOrigin::Wildcard)
        ));
    }

    #[test]
    fn allow_list_echoes_only_listed_origin() {
        let policy = CorsPolicy::AllowList(vec!["https://a.example".to_owned()]);
        match policy.allow_origin(Some("https://a.example")) {
            Some(AllowOrigin::Echo(origin)) => assert_eq!(origin, "https://a.example"),
            other => panic!("expected echo of the listed origin, got {other:?}"),
        }
        // An unlisted origin receives no header → the browser blocks the read.
        assert!(policy.allow_origin(Some("https://evil.example")).is_none());
        // A request with no Origin against an allow-list is same-origin: no
        // header, and no panic from the `?` short-circuit.
        assert!(policy.allow_origin(None).is_none());
    }

    #[test]
    fn varies_on_origin_only_for_allow_list() {
        // Only the allow-list's CORS output depends on the request `Origin`,
        // so only it must drive a `Vary: Origin` (#859). Wildcard answers `*`
        // uniformly and Disabled emits nothing — both are origin-independent.
        assert!(CorsPolicy::AllowList(vec!["https://a.example".to_owned()]).varies_on_origin());
        assert!(!CorsPolicy::Wildcard.varies_on_origin());
        assert!(!CorsPolicy::Disabled.varies_on_origin());
    }
}
