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

use std::path::Path;

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

/// Build a `bca` `Command` whose working directory is `dir`, scrubbing
/// CI-side env vars first.
///
/// `bca check` discovers its `bca.toml` (and the `baseline` it names)
/// by climbing parents until it finds the directory containing `.git`.
/// The integration suite runs from inside this repo, so a `Command`
/// left at the inherited cwd auto-discovers the repo's own
/// `bca.toml` + `.bca-baseline.toml` and silently filters/scales each
/// fixture run against repo state. Anchoring the cwd at a
/// `tempfile::tempdir()` — which has no `.git` ancestor — makes
/// discovery find nothing, so the run is hermetic. This is the default
/// builder for every test that does not *itself* exercise manifest
/// auto-discovery (see #491).
#[allow(dead_code)]
pub fn cli_in(dir: &Path) -> Command {
    let mut cmd = bca_command();
    cmd.current_dir(dir);
    cmd
}

/// Build a hermetic `bca` `Command` rooted at a fresh, empty
/// `tempfile::tempdir()`, returning the guard alongside it.
///
/// Use this for tests that have no fixture tempdir of their own (e.g.
/// they analyse a repo-relative fixture via an absolute `--paths`) but
/// still must not inherit the repo's discovered `bca.toml` / baseline.
/// The returned [`tempfile::TempDir`] must be kept alive until the
/// command has been spawned — drop it too early and the cwd vanishes
/// before `bca` reads it. See [`cli_in`] for the discovery rationale
/// (#491).
#[allow(dead_code)]
pub fn cli_hermetic() -> (tempfile::TempDir, Command) {
    let dir = tempfile::tempdir().expect("create tempdir for hermetic cwd");
    let cmd = cli_in(dir.path());
    (dir, cmd)
}
