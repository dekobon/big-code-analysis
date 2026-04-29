use std::thread::available_parallelism;

use clap::Parser;

use big_code_analysis_web::server::{DEFAULT_PARSE_TIMEOUT_SECS, run_with_timeout};

#[derive(Parser, Debug)]
#[clap(
    name = "big-code-analysis-web",
    version,
    author,
    about = "Run a web server."
)]
struct Opts {
    /// Number of jobs.
    #[clap(long, short = 'j')]
    num_jobs: Option<usize>,
    /// Host for the web server.
    #[clap(long, default_value = "127.0.0.1")]
    host: String,
    /// Port for the web server.
    #[clap(long, short, default_value = "8080")]
    port: u16,
    /// Timeout in seconds for each parse operation (0 = no timeout).
    #[clap(long, default_value_t = DEFAULT_PARSE_TIMEOUT_SECS)]
    parse_timeout_secs: u64,
}

#[actix_web::main]
async fn main() {
    let opts = Opts::parse();

    let num_jobs = opts.num_jobs.unwrap_or_else(|| {
        available_parallelism()
            .map(|n| n.get())
            .unwrap_or_else(|e| {
                eprintln!("Failed to get available parallelism: {e}; defaulting to 4 workers");
                4
            })
    });

    if let Err(e) = run_with_timeout(&opts.host, opts.port, num_jobs, opts.parse_timeout_secs).await
    {
        eprintln!(
            "Cannot run the server at {}:{}: {}",
            opts.host, opts.port, e
        );
    }
}
