//! `bca` binary entry point. All logic lives in the
//! [`big_code_analysis_cli`] library so the workspace `xtask` crate can
//! reuse the same `clap` definition to render man pages.

fn main() {
    big_code_analysis_cli::run();
}
