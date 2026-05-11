#![allow(missing_docs)]
use std::thread::available_parallelism;

use clap::Parser;

use big_code_analysis_web::cli::Opts;
use big_code_analysis_web::server::run_with_timeout;

#[actix_web::main]
async fn main() {
    let opts = Opts::parse();

    let num_jobs = opts.num_jobs.unwrap_or_else(|| {
        available_parallelism().map_or_else(
            |e| {
                eprintln!("Failed to get available parallelism: {e}; defaulting to 4 workers");
                4
            },
            std::num::NonZero::get,
        )
    });

    if let Err(e) = run_with_timeout(&opts.host, opts.port, num_jobs, opts.parse_timeout_secs).await
    {
        eprintln!(
            "Cannot run the server at {}:{}: {}",
            opts.host, opts.port, e
        );
    }
}
