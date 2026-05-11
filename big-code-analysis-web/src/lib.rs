//! REST API surface for `big-code-analysis`. Run via the `bca-web` binary.

// The deeply nested `json!` literals in server.rs tests exceed the default
// recursion limit (128) during `json_internal!` macro expansion.
#![recursion_limit = "256"]

/// HTTP endpoints and request handlers.
pub mod web;
pub use web::*;

/// `bca-web` command-line parser. Lifted out of the binary so the
/// workspace `xtask` crate can render its man page from the same
/// `clap::Command` tree the running binary parses.
pub mod cli {
    use clap::Parser;

    use crate::web::server::DEFAULT_PARSE_TIMEOUT_SECS;

    /// Command-line options for the `bca-web` REST API server.
    #[derive(Parser, Debug)]
    #[clap(name = "bca-web", version, author, about = "Run a web server.")]
    pub struct Opts {
        /// Number of jobs.
        #[clap(long, short = 'j')]
        pub num_jobs: Option<usize>,
        /// Host for the web server.
        #[clap(long, default_value = "127.0.0.1")]
        pub host: String,
        /// Port for the web server.
        #[clap(long, short, default_value = "8080")]
        pub port: u16,
        /// Timeout in seconds for each parse operation (0 = no timeout).
        #[clap(long, default_value_t = DEFAULT_PARSE_TIMEOUT_SECS)]
        pub parse_timeout_secs: u64,
    }
}
