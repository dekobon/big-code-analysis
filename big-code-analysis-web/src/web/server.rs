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

mod errors;
mod handlers;
mod routing;

// Re-export each bucket's items into the parent namespace so the entry
// points here and the `#[cfg(test)] mod tests` below reach them, and so
// each child's own `use super::*` sees its siblings' items.
#[allow(clippy::wildcard_imports)]
use errors::*;
#[allow(clippy::wildcard_imports)]
use handlers::*;
#[allow(clippy::wildcard_imports)]
use routing::*;

/// `expect` message used at every `action::<_>` call site below.
///
/// The web crate pins `big-code-analysis` with `features =
/// ["all-languages"]`, so a `LANG` value that reached this point must
/// be enabled at compile time. Any future caller that loosens the
/// feature pin must change this invariant explicitly.
const FEATURES_PINNED: &str = "web crate pins big-code-analysis features = [\"all-languages\"]";

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
