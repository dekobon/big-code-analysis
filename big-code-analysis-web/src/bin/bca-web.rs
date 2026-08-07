#![allow(missing_docs)]
// Production-only `unwrap()` ban. See `[workspace.lints.clippy]` in the
// root `Cargo.toml` for why this is a per-root attribute and not a
// Cargo lint (#1227).
#![cfg_attr(not(test), warn(clippy::unwrap_used))]
use std::process::ExitCode;

use clap::Parser;

use tracing_subscriber::{EnvFilter, fmt::format::FmtSpan};

use big_code_analysis_web::cli::Opts;
use big_code_analysis_web::cors::CorsPolicy;
use big_code_analysis_web::server::run_with_timeout;

#[actix_web::main]
async fn main() -> ExitCode {
    // Initialise the tracing subscriber before the server is spawned so
    // every worker thread inherits it (the global dispatcher is
    // process-wide). `RUST_LOG` drives the filter (default `info`).
    // `try_init` returns an error rather than panicking if a global
    // subscriber is already set, keeping the binary panic-free.
    // `FmtSpan::CLOSE` emits a line when any span closes; in this daemon the
    // only spans are `TracingLogger`'s per-request root spans, so this
    // yields one access-log line per completed request (method, route,
    // status, latency).
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_span_events(FmtSpan::CLOSE)
        .try_init();

    let opts = Opts::parse();

    // `NumJobs::resolve` is the single shared path with the `bca` CLI:
    // `auto` honors Linux cgroup CPU quotas / cpusets via
    // `available_parallelism`, falling back to 1 if that syscall errors,
    // and an explicit value is passed through unchanged (#560).
    let num_jobs = opts.num_jobs.resolve();

    // Off by default (#694): `CorsPolicy::parse(None)` is `Disabled`, so the
    // server emits no CORS headers unless the operator passed `--cors`.
    let cors = CorsPolicy::parse(opts.cors.as_deref());

    // A daemon that fails to bind or dies on an I/O error must exit
    // non-zero so a supervisor (systemd, a container orchestrator, a CI
    // smoke check) sees the failure and can restart or alert; logging the
    // error and exiting `0` (the pre-#707 behaviour) reports success on a
    // server that never served. `ExitCode::FAILURE` is the portable
    // non-zero exit.
    if let Err(e) = run_with_timeout(
        &opts.host,
        opts.port,
        num_jobs,
        opts.parse_timeout_secs,
        cors,
    )
    .await
    {
        tracing::error!(host = %opts.host, port = opts.port, error = %e, "Cannot run the server");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
