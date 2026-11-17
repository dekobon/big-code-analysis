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

use big_code_analysis::MetricsError;

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

/// Like [`run_parse`], but for a closure that can itself fail.
///
/// The library entry points the handlers call all return
/// `Result<_, MetricsError>`, so without this every call site repeats
/// the same `.await?.map_err(ParseError::from_metrics)?` pair — seven
/// chances to reach for `expect` instead, which is what #1152 was
/// cleaning up. Folding it here leaves each handler with a single `?`
/// and makes propagation the path of least resistance.
async fn run_parse_fallible<T: Send + 'static>(
    config: &web::Data<ParseConfig>,
    payload_id: &str,
    f: impl FnOnce() -> Result<T, MetricsError> + Send + 'static,
) -> Result<T, ParseError> {
    run_parse(config, payload_id, f)
        .await?
        .map_err(|err| ParseError::from_metrics(payload_id, err))
}

/// Cancels the parse it holds when dropped: by then nobody awaits the
/// task's result, whether `run_parse` returned or its future was dropped.
///
/// actix drops a handler's future when a read from its connection fails,
/// as one does once the connection is reset (RST). A client that closes
/// its sending side (FIN) does not end it: HTTP/1 lets a client half-close
/// and still read the response, so actix reads nothing more from that
/// connection (`read_available` in actix-http's h1 dispatcher), not even
/// a reset that follows, and the handler runs on until it answers or its
/// deadline, if any.
struct CancelOnDrop(big_code_analysis::ParseCancel);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

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

    // A blocking task nobody waits for any more cannot be interrupted, so
    // it is asked to stop instead: the parse and the AST export read this
    // flag and give up, which ends the orphan soon after the deadline
    // rather than after however long they would have taken (#1939). The
    // flag is set on drop, so a handler actix drops before the deadline,
    // as it does when the connection is reset before the client closes its
    // sending side, stops its work too.
    let cancel = CancelOnDrop(big_code_analysis::ParseCancel::new());
    let task_cancel = cancel.0.clone();
    let mut handle = tokio::task::spawn_blocking(move || task_cancel.run(f));

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
                // A timeout orphans the blocking task until it finishes,
                // which `cancel` makes prompt for the parse once this
                // returns. Log it (`warn`, not `error`: it is a deadline /
                // load condition, not an internal fault) so ops can
                // correlate which request timed out and track orphan-pool
                // pressure.
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
/// When a request times out, its blocking task on tokio's pool is asked
/// to stop: the parse and the AST export check for that as they run,
/// while other work already under way, such as a `/vcs` history walk,
/// runs to completion. Until it ends the task counts as orphaned. To prevent unbounded growth from sustained
/// pathological inputs, new requests are rejected with `503` once the
/// orphan count reaches a soft cap. The cap defaults
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
    serve(Listen::Addr(host, port), n_threads, config, cors)?.await
}

/// Where [`serve`] accepts connections.
enum Listen<'a> {
    /// Bind `host:port`, as `bca-web` does.
    Addr(&'a str, u16),
    /// A socket the caller bound, so a test can serve on an ephemeral port
    /// with production's middleware, routes and HTTP settings. OS signal
    /// handling is left off: actix would install process-wide SIGINT /
    /// SIGTERM handlers that outlive the test.
    #[cfg(test)]
    Socket(std::net::TcpListener),
}

/// Builds the HTTP server `run_with_timeout` runs: its middleware, routes
/// and connection settings, one place for production and the transport
/// tests to share.
#[cfg_attr(
    not(test),
    expect(
        clippy::needless_pass_by_value,
        reason = "only the test-only `Listen::Socket` is consumed"
    )
)]
fn serve(
    listen: Listen<'_>,
    n_threads: usize,
    config: web::Data<ParseConfig>,
    cors: CorsPolicy,
) -> std::io::Result<actix_web::dev::Server> {
    // CORS is off by default (#694): the `from_fn` layer is wrapped only
    // when `--cors` is enabled (`Condition::new`), so the default request
    // path carries no extra middleware. The policy is registered as app
    // data so the `cors_middleware` extractor can read it when enabled.
    let cors_enabled = cors != CorsPolicy::Disabled;
    let cors = web::Data::new(cors);

    let server = HttpServer::new(move || {
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
    .workers(n_threads);
    let server = match listen {
        Listen::Addr(host, port) => server.bind((host, port))?,
        #[cfg(test)]
        Listen::Socket(listener) => server.listen(listener)?.disable_signals(),
    };
    Ok(server.run())
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
