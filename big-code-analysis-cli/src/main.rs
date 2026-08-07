//! `bca` binary entry point. All logic lives in the
//! [`big_code_analysis_cli`] library so the workspace `xtask` crate can
//! reuse the same `clap` definition to render man pages.

// Production-only `unwrap()` ban. See `[workspace.lints.clippy]` in the
// root `Cargo.toml` for why this is a per-root attribute and not a
// Cargo lint (#1227).
#![cfg_attr(not(test), warn(clippy::unwrap_used))]

fn main() {
    big_code_analysis_cli::run();
}
