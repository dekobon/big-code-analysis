//! CLI integration tests for `bca vcs trend` (issue #333): the JSON time
//! series shape, the absent-file `null` markers, the point-count
//! validation error, and the outside-a-repo error.
//!
//! Each test drives the real `bca` binary against a tiny throwaway git
//! repository built through the `git` CLI with a fixed identity and
//! explicit commit dates, so the runs are deterministic.

use std::path::Path;
use std::process::Command as Git;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

mod common;

/// Reference "now" the trend's most-recent point is pinned to via
/// `--as-of`, so the sampled grid is fully reproducible.
const FIXED_NOW: i64 = 1_700_000_000;
const DAY: i64 = 86_400;

/// Build a two-file, two-commit repo: `early.rs` at `now − 300d`, then
/// `late.rs` added at `now − 100d`.
fn staged_repo() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "commit.gpgsign", "false"]);

    std::fs::write(dir.path().join("early.rs"), "fn a() {}\n").expect("write early");
    git(&dir, &["add", "-A"]);
    commit(&dir, FIXED_NOW - 300 * DAY, "init early");

    std::fs::write(dir.path().join("late.rs"), "fn b() {}\n").expect("write late");
    git(&dir, &["add", "-A"]);
    commit(&dir, FIXED_NOW - 100 * DAY, "add late");
    dir
}

fn git(dir: &TempDir, args: &[&str]) {
    let status = Git::new("git")
        .args(args)
        .current_dir(dir.path())
        .status()
        .expect("spawn git");
    assert!(status.success(), "git {args:?} failed");
}

fn commit(dir: &TempDir, secs: i64, message: &str) {
    let date = format!("@{secs} +0000");
    let status = Git::new("git")
        .args(["commit", "-q", "--no-verify", "-m", message])
        .current_dir(dir.path())
        .envs([
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_AUTHOR_DATE", date.as_str()),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_DATE", date.as_str()),
        ])
        .status()
        .expect("spawn git commit");
    assert!(status.success(), "git commit failed");
}

/// A `bca` command rooted at `dir`.
fn bca(dir: &Path) -> Command {
    common::cli_in(dir)
}

#[test]
fn trend_emits_stable_json_shape() {
    let repo = staged_repo();
    // `--as-of` (parent flag) pins the newest point; 3 points / 300d puts
    // the oldest exactly on the first commit.
    let as_of = format!("@{FIXED_NOW}");
    let output = bca(repo.path())
        .args([
            "vcs", "--as-of", &as_of, "trend", "--points", "3", "--span", "300d", "-O", "json",
        ])
        .output()
        .expect("run bca vcs trend");
    assert!(output.status.success(), "trend should exit 0 inside a repo");

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("trend output is valid JSON");
    assert_eq!(json["trend_schema_version"], 1);
    // Pin the concrete per-point block version (stats::VCS_SCHEMA_VERSION),
    // not merely "is a number" — a schema bump must update this test.
    assert_eq!(json["vcs_schema_version"], 2);
    let points = json["as_of_points"].as_array().expect("as_of_points array");
    assert_eq!(points.len(), 3);
    assert_eq!(points[2], FIXED_NOW, "newest point is the as-of anchor");

    // late.rs was added at the middle commit, so it is null at the oldest
    // point and present afterwards.
    let late = json["files"]["late.rs"].as_array().expect("late.rs series");
    assert!(late[0].is_null(), "late.rs absent at the oldest point");
    assert!(late[2].is_object(), "late.rs present at the newest point");
    assert_eq!(late[2]["as_of"], FIXED_NOW);
    assert!(
        late[2]["risk_score"].is_number(),
        "a present point carries the flattened vcs block"
    );

    // The delta summary is present and split into two lists.
    assert!(json["deltas"]["improved"].is_array());
    assert!(json["deltas"]["regressed"].is_array());
}

#[test]
fn yaml_format_emits_valid_document() {
    let repo = staged_repo();
    let output = bca(repo.path())
        .args([
            "vcs", "trend", "--points", "2", "--span", "30d", "-O", "yaml",
        ])
        .output()
        .expect("run bca vcs trend yaml");
    assert!(output.status.success());
    let doc: serde_yaml::Value =
        serde_yaml::from_slice(&output.stdout).expect("trend output is valid YAML");
    assert_eq!(doc["trend_schema_version"], 1);
    // Non-vacuous structural checks: the point grid and file map are
    // present with the requested shape.
    assert_eq!(
        doc["as_of_points"].as_sequence().map(Vec::len),
        Some(2),
        "two requested points appear in the YAML"
    );
    assert!(doc["files"].is_mapping(), "files is a path-keyed mapping");
}

/// A history-tuning flag (`--as-of`) is accepted in the *subcommand*
/// position (`vcs trend --as-of …`), the natural ordering, and genuinely
/// anchors the walk: the newest sample point lands on the supplied instant,
/// exactly as it does from the parent position (issue #598). `--ref` is also
/// valid under `trend` (it analyses ref'd state, unlike `jit`), so unlike
/// `jit` no conflict is raised here.
#[test]
fn trend_accepts_as_of_in_subcommand_position() {
    let repo = staged_repo();
    let as_of = format!("@{FIXED_NOW}");
    let output = bca(repo.path())
        .args([
            "vcs", "trend", "--as-of", &as_of, "--points", "3", "--span", "300d", "-O", "json",
        ])
        .output()
        .expect("run bca vcs trend");
    assert!(
        output.status.success(),
        "subcommand-position --as-of should parse and anchor the walk"
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("trend output is valid JSON");
    let points = json["as_of_points"].as_array().expect("as_of_points array");
    assert_eq!(
        points[2], FIXED_NOW,
        "newest point honors subcommand-position --as-of"
    );
}

#[test]
fn too_few_points_is_rejected() {
    let repo = staged_repo();
    bca(repo.path())
        .args(["vcs", "trend", "--points", "1", "--span", "30d"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("at least 2 points"));
}

#[test]
fn outside_a_repo_errors() {
    let dir = tempfile::tempdir().expect("tempdir");
    bca(dir.path())
        .args(["vcs", "trend", "--points", "3", "--span", "30d"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "not inside a supported version-control working tree",
        ));
}
