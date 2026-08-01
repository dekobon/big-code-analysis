//! Regression: `bca check --changed-only --report-suppressed` must scope
//! the *surfaced suppressed debt* to the diff range exactly as it scopes the
//! active offenders. Before the fix, `--changed-only` was applied only to the
//! active set (after the active/suppressed partition), so suppressed offenders
//! from files outside the diff range leaked into the SARIF document. The fix
//! applies the diff-scope filter to all offenders *before* the split.
//!
//! The test contrasts two runs over the same fixture: without `--changed-only`
//! the out-of-range suppressed file IS surfaced (proving the fixture is a real
//! suppressed offender); with `--changed-only` it must disappear while the
//! in-range active offender stays.

use std::fs;
use std::path::Path;
use std::process::Command as StdCommand;

use assert_cmd::Command;

use crate::common;

fn cli() -> Command {
    common::bca_command()
}

/// Run `git <args>` in `dir`, asserting success.
fn git(dir: &Path, args: &[&str]) {
    let status = StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .status()
        .expect("spawn git");
    assert!(status.success(), "git {args:?} failed");
}

/// cyclomatic 1 (no branching) — below a `cyclomatic=1` threshold.
const FLAT: &str = "fn g(x: i32) -> i32 {\n    x + 1\n}\n";
/// cyclomatic 2 (one `if`) — above the threshold.
const BRANCHY: &str = "fn g(x: i32) -> i32 {\n    if x > 0 { 1 } else { 2 }\n}\n";
/// A whole-file-suppressed cyclomatic offender (cyclomatic 2).
const SUPPRESSED: &str =
    "// bca: suppress-file(cyclomatic)\nfn f(x: i32) -> i32 {\n    if x > 0 { 1 } else { 2 }\n}\n";

/// uris of every SARIF result in `stdout`.
fn result_uris(stdout: &[u8]) -> Vec<String> {
    let doc: serde_json::Value = serde_json::from_slice(stdout).expect("json");
    doc["runs"][0]["results"]
        .as_array()
        .expect("results array")
        .iter()
        .map(|r| {
            r["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
                .as_str()
                .expect("uri")
                .to_string()
        })
        .collect()
}

/// Build a repo whose first commit holds an out-of-range, whole-file-suppressed
/// offender (`src/suppressed.rs`) plus a flat `src/changed.rs`; the second
/// commit grows `src/changed.rs` into an active offender. So `HEAD~1...HEAD`
/// touches only `changed.rs` — `suppressed.rs` is outside the diff range.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    git(dir.path(), &["init", "-q"]);
    fs::create_dir(dir.path().join("src")).expect("mkdir src");
    fs::write(dir.path().join("src/suppressed.rs"), SUPPRESSED).expect("write suppressed");
    fs::write(dir.path().join("src/changed.rs"), FLAT).expect("write flat");
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "-m", "base"]);
    fs::write(dir.path().join("src/changed.rs"), BRANCHY).expect("grow changed");
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "-m", "grow"]);
    dir
}

#[test]
fn report_suppressed_surfaces_out_of_range_debt_without_changed_only() {
    // Baseline of the contrast: without `--changed-only`, the whole tree is
    // analysed, so the suppressed file IS surfaced (as an inSource
    // suppression). This proves `suppressed.rs` is a genuine suppressed
    // offender — the next test's absence is meaningful only against this.
    let repo = repo();
    let out = cli()
        .current_dir(repo.path())
        .args([
            "check",
            "--paths",
            ".",
            "--since",
            "HEAD~1",
            "--report-suppressed",
            "--output-format",
            "sarif",
            "--threshold",
            "cyclomatic=1",
            "--no-fail",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let uris = result_uris(&out);
    assert!(
        uris.iter().any(|u| u.contains("suppressed.rs")),
        "without --changed-only the suppressed offender must be surfaced; got {uris:?}"
    );
}

#[test]
fn changed_only_drops_out_of_range_suppressed_debt() {
    // The regression guard: with `--changed-only`, `HEAD~1...HEAD` touches
    // only `changed.rs`, so the out-of-range `suppressed.rs` debt must be
    // dropped from the document — while the in-range active offender stays.
    let repo = repo();
    let out = cli()
        .current_dir(repo.path())
        .args([
            "check",
            "--paths",
            ".",
            "--since",
            "HEAD~1",
            "--changed-only",
            "--report-suppressed",
            "--output-format",
            "sarif",
            "--threshold",
            "cyclomatic=1",
            "--no-fail",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let uris = result_uris(&out);
    assert!(
        uris.iter().any(|u| u.contains("changed.rs")),
        "in-range active offender must stay; got {uris:?}"
    );
    assert!(
        !uris.iter().any(|u| u.contains("suppressed.rs")),
        "out-of-range suppressed debt leaked past --changed-only; got {uris:?}"
    );
}
