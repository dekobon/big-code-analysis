#![allow(missing_docs)]
use clap::Parser;

use tracing_subscriber::{EnvFilter, fmt::format::FmtSpan};

use big_code_analysis_web::cli::Opts;
use big_code_analysis_web::server::run_with_timeout;

#[actix_web::main]
async fn main() {
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

    if let Err(e) = run_with_timeout(&opts.host, opts.port, num_jobs, opts.parse_timeout_secs).await
    {
        tracing::error!(host = %opts.host, port = opts.port, error = %e, "Cannot run the server");
    }
}
