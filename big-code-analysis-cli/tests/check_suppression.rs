//! Integration tests for in-source suppression markers wired through
//! `bca check` (#98).
//!
//! These tests drive the binary against tiny inline fixtures and
//! verify both the default "honor markers" behaviour and the
//! `--no-suppress` override that CI auditors use to see un-silenced
//! offender lists.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

mod common;

/// Hermetic `bca` builder: anchors the process cwd at `dir` (a
/// `tempfile::tempdir()` with no `.git` ancestor) so `bca check` cannot
/// auto-discover the repo's own `bca.toml` / `.bca-baseline.toml` and
/// filter the inline fixtures against repo state (#491).
fn cli(dir: &Path) -> Command {
    common::cli_in(dir)
}

/// Rust function with cyclomatic complexity > 1 plus an inline
/// `bca: suppress` marker silencing cyclomatic. Used to confirm the
/// honor / ignore paths.
const SUPPRESSED_RUST: &str = r#"
pub fn classify(n: i32) -> &'static str {
    // bca: suppress(cyclomatic)
    if n < 0 {
        "neg"
    } else if n == 0 {
        "zero"
    } else {
        "pos"
    }
}
"#;

/// Rust function carrying a Lizard-style marker. Confirms the compat
/// layer fires identically to the native marker. The `#` sigil is
/// part of the Lizard directive itself; `//` is the language comment
/// opener.
const LIZARD_RUST: &str = r#"
pub fn classify(n: i32) -> &'static str {
    // #lizard forgives
    if n < 0 {
        "neg"
    } else {
        "pos"
    }
}
"#;

/// Rust source with a file-level marker covering `cyclomatic`.
const FILE_SUPPRESSED_RUST: &str = r#"
// bca: suppress-file(cyclomatic)

pub fn classify(n: i32) -> &'static str {
    if n < 0 {
        "neg"
    } else {
        "pos"
    }
}
"#;

fn write_fixture(dir: &TempDir, name: &str, body: &str) -> String {
    let path = dir.path().join(name);
    fs::write(&path, body).expect("write fixture");
    path.to_str().expect("utf8 fixture path").to_string()
}

#[test]
fn suppression_marker_silences_violation_by_default() {
    // `classify` would exceed cyclomatic=1 by a wide margin, but the
    // inline `bca: suppress(cyclomatic)` marker should silence the
    // violation so the run exits 0 with empty stderr.
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", SUPPRESSED_RUST);

    cli(dir.path())
        .args(["--paths", &path, "check", "--threshold", "cyclomatic=1"])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

#[test]
fn no_suppress_flag_re_enables_violation() {
    // `--no-suppress` is the audit toggle: every marker is ignored,
    // and the same fixture that exits 0 without the flag now exits 2.
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", SUPPRESSED_RUST);

    cli(dir.path())
        .args([
            "--paths",
            &path,
            "check",
            "--threshold",
            "cyclomatic=1",
            "--no-suppress",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("classify"))
        .stderr(predicate::str::contains("cyclomatic"));
}

#[test]
fn lizard_compat_marker_silences_violation() {
    // The `#lizard forgives` marker must produce the same exit-code
    // behaviour as the native `bca: suppress` form, so codebases coming
    // from Lizard migrate cleanly without rewriting comments.
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", LIZARD_RUST);

    cli(dir.path())
        .args(["--paths", &path, "check", "--threshold", "cyclomatic=1"])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

#[test]
fn file_scoped_marker_silences_nested_function_violation() {
    // A file-scoped marker must silence violations on every nested
    // function, not just the top-level Unit space. The threshold
    // engine ORs the file-scope against each function's own scope.
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", FILE_SUPPRESSED_RUST);

    cli(dir.path())
        .args(["--paths", &path, "check", "--threshold", "cyclomatic=1"])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

/// Regression fixture for #263. The verb `allow` was the old marker
/// spelling; after the hard rename it is no longer recognized and
/// must leave the cyclomatic violation visible.
const LEGACY_ALLOW_RUST: &str = r#"
pub fn classify(n: i32) -> &'static str {
    // bca: allow(cyclomatic)
    if n < 0 {
        "neg"
    } else if n == 0 {
        "zero"
    } else {
        "pos"
    }
}
"#;

#[test]
fn legacy_allow_marker_does_not_suppress() {
    // Hard-rename regression (#263): a `// bca: allow(...)` comment in
    // shipped source must NOT silence the violation. The parser
    // surfaces `allow` / `allow-file` as `UnknownVerb`, which the
    // walk-time scanner drops with a stderr warning — the threshold
    // checker then sees no marker and the violation fires normally.
    //
    // Three things must all be true; we pin each one independently so
    // a regression in any single half (e.g., walker silently swallows
    // the error, or warning text drifts without the violation firing)
    // surfaces clearly:
    //   1. exit code 2 — the violation is reported, the marker did not
    //      suppress it;
    //   2. stderr names the offender and metric — the violation line
    //      exists and is intelligible;
    //   3. stderr names the bad verb — the user gets a diagnostic
    //      pointing them at the rename, not a silent drop.
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", LEGACY_ALLOW_RUST);

    cli(dir.path())
        .args(["--paths", &path, "check", "--threshold", "cyclomatic=1"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("classify"))
        .stderr(predicate::str::contains("cyclomatic"))
        .stderr(predicate::str::contains(
            "unknown bca directive verb 'allow'",
        ));
}

#[test]
fn unsuppressed_metric_still_violates() {
    // Per-metric scoping: `bca: suppress(cyclomatic)` leaves other
    // metrics' violations visible. Threshold on a non-listed metric
    // (cognitive) still fires.
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", SUPPRESSED_RUST);

    cli(dir.path())
        .args(["--paths", &path, "check", "--threshold", "cognitive=0"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cognitive"));
}
