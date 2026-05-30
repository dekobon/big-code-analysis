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
        /// Number of jobs (worker threads); must be at least 1.
        // A value of 0 would create a zero-permit semaphore (blocking every
        // parse forever) and trip actix-server's `assert_ne!(num, 0)` in
        // `ServerBuilder::workers`, panicking at startup. Reject it at parse
        // time with a clap range validator instead.
        #[clap(long, short = 'j', value_parser = clap::value_parser!(u32).range(1..))]
        pub num_jobs: Option<u32>,
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

    #[cfg(test)]
    mod tests {
        use super::*;
        use clap::error::ErrorKind;

        #[test]
        fn opts_parses_defaults() {
            let opts = Opts::try_parse_from(["bca-web"]).expect("default parse must succeed");
            assert_eq!(opts.host, "127.0.0.1");
            assert_eq!(opts.port, 8080);
            assert_eq!(opts.num_jobs, None);
            assert_eq!(opts.parse_timeout_secs, DEFAULT_PARSE_TIMEOUT_SECS);
        }

        #[test]
        fn opts_overrides_host() {
            let opts = Opts::try_parse_from(["bca-web", "--host", "0.0.0.0"])
                .expect("host override must parse");
            assert_eq!(opts.host, "0.0.0.0");
            assert_eq!(opts.port, 8080);
        }

        #[test]
        fn opts_overrides_port_long() {
            let opts = Opts::try_parse_from(["bca-web", "--port", "9000"])
                .expect("port override must parse");
            assert_eq!(opts.port, 9000);
            assert_eq!(opts.host, "127.0.0.1");
        }

        #[test]
        fn opts_overrides_port_short() {
            let opts =
                Opts::try_parse_from(["bca-web", "-p", "7777"]).expect("-p short flag must parse");
            assert_eq!(opts.port, 7777);
        }

        #[test]
        fn opts_rejects_non_numeric_port() {
            let err = Opts::try_parse_from(["bca-web", "--port", "not-a-number"])
                .expect_err("non-numeric port must be rejected");
            assert_eq!(err.kind(), ErrorKind::ValueValidation);
        }

        #[test]
        fn opts_rejects_out_of_range_port() {
            let err = Opts::try_parse_from(["bca-web", "--port", "70000"])
                .expect_err("port above u16::MAX must be rejected");
            assert_eq!(err.kind(), ErrorKind::ValueValidation);
        }

        #[test]
        fn opts_overrides_num_jobs_long() {
            let opts = Opts::try_parse_from(["bca-web", "--num-jobs", "8"])
                .expect("--num-jobs must parse");
            assert_eq!(opts.num_jobs, Some(8));
        }

        #[test]
        fn opts_overrides_num_jobs_short() {
            let opts =
                Opts::try_parse_from(["bca-web", "-j", "4"]).expect("-j short flag must parse");
            assert_eq!(opts.num_jobs, Some(4));
        }

        #[test]
        fn opts_rejects_non_numeric_num_jobs() {
            let err = Opts::try_parse_from(["bca-web", "--num-jobs", "many"])
                .expect_err("non-numeric num_jobs must be rejected");
            assert_eq!(err.kind(), ErrorKind::ValueValidation);
        }

        #[test]
        fn opts_rejects_zero_num_jobs() {
            // 0 would create a zero-permit semaphore and panic actix-server's
            // worker assertion; clap must reject it at parse time (issue #427).
            let err = Opts::try_parse_from(["bca-web", "--num-jobs", "0"])
                .expect_err("zero num_jobs must be rejected");
            assert_eq!(err.kind(), ErrorKind::ValueValidation);
        }

        #[test]
        fn opts_accepts_one_num_job() {
            let opts =
                Opts::try_parse_from(["bca-web", "--num-jobs", "1"]).expect("one job must parse");
            assert_eq!(opts.num_jobs, Some(1));
        }

        #[test]
        fn opts_overrides_parse_timeout() {
            let opts = Opts::try_parse_from(["bca-web", "--parse-timeout-secs", "60"])
                .expect("--parse-timeout-secs must parse");
            assert_eq!(opts.parse_timeout_secs, 60);
        }

        #[test]
        fn opts_parse_timeout_zero_means_no_timeout() {
            let opts = Opts::try_parse_from(["bca-web", "--parse-timeout-secs", "0"])
                .expect("zero timeout must parse");
            assert_eq!(opts.parse_timeout_secs, 0);
        }

        #[test]
        fn opts_rejects_non_numeric_parse_timeout() {
            let err = Opts::try_parse_from(["bca-web", "--parse-timeout-secs", "soon"])
                .expect_err("non-numeric parse-timeout-secs must be rejected");
            assert_eq!(err.kind(), ErrorKind::ValueValidation);
        }

        #[test]
        fn opts_rejects_unknown_flag() {
            let err = Opts::try_parse_from(["bca-web", "--definitely-not-a-real-flag"])
                .expect_err("unknown flag must be rejected");
            assert_eq!(err.kind(), ErrorKind::UnknownArgument);
        }

        #[test]
        fn opts_combines_overrides() {
            let opts = Opts::try_parse_from([
                "bca-web",
                "--host",
                "10.0.0.1",
                "--port",
                "9090",
                "--num-jobs",
                "16",
                "--parse-timeout-secs",
                "120",
            ])
            .expect("all-overrides parse must succeed");
            assert_eq!(opts.host, "10.0.0.1");
            assert_eq!(opts.port, 9090);
            assert_eq!(opts.num_jobs, Some(16));
            assert_eq!(opts.parse_timeout_secs, 120);
        }
    }
}
