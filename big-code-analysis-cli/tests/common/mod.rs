#![allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]

//! Format-validity helpers for the CLI integration suite.
//!
//! Submodule `validators` carries the same three helpers as
//! `tests/common/validators.rs` in the lib crate (validate_sarif,
//! assert_checkstyle_well_formed_and_structural, assert_html_well_formed).
//! Cargo `[dev-dependencies]` and shared modules do not propagate
//! across workspace members, so the duplication is unavoidable
//! without a separate test-helpers crate. Three small helpers don't
//! merit that indirection today.

use assert_cmd::Command;

#[allow(dead_code)]
pub mod validators;

/// Scrub CI-side env vars that `bca check` auto-detects from a
/// freshly-built `Command`. On a GitHub Actions runner the parent
/// process exports `GITHUB_STEP_SUMMARY` pointing to the runner's
/// real step-summary file, and `GITHUB_ACTIONS=true` enables
/// `::error` annotations. `assert_cmd::Command` inherits the
/// parent environment by default, so without this scrub every
/// test-driven `bca check` invocation appends a TempDir-fixture
/// digest to the runner's UI panel — and because the digest is
/// bounded by fixed sentinels, the last test wins, replacing every
/// earlier block (see #388).
///
/// Call sites name their builder `cli()` (or `bin()` in
/// `big-code-analysis-web`); each delegates to this helper so a
/// future new env-leak only needs to be patched once.
#[allow(dead_code)]
pub fn scrub_ci_env(cmd: &mut Command) -> &mut Command {
    cmd.env_remove("GITHUB_STEP_SUMMARY")
        .env_remove("GITHUB_ACTIONS")
}

/// Build a `bca` `Command` with CI-side env vars scrubbed. The
/// per-test `cli()` helpers delegate here so the env-isolation
/// policy lives in one place.
#[allow(dead_code)]
pub fn bca_command() -> Command {
    let mut cmd = Command::cargo_bin("bca").expect("bca binary builds");
    scrub_ci_env(&mut cmd);
    cmd
}
