//! End-to-end tests for `bca vcs` and `bca metrics --vcs` (issue #328).
//!
//! Each builds a throwaway git repo with fixed author identities and
//! commit timestamps dated relative to wall-clock now, so the per-window
//! counts are deterministic across runs.

#![allow(clippy::doc_markdown)]

use std::path::Path;
use std::process::Command as StdCommand;
use std::time::{SystemTime, UNIX_EPOCH};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

mod common;

const DAY: i64 = 86_400;

fn cli() -> Command {
    common::bca_command()
}

/// Wall-clock "now" in Unix seconds. Both `bca vcs` and `bca metrics
/// --vcs` default to wall-clock time, so fixture commits are dated
/// relative to this; the *relative* offsets (and hence the counts) stay
/// deterministic across runs.
fn now() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_secs(),
    )
    .expect("now fits i64")
}

/// Run `git <args>` in `dir` at commit time `secs`, asserting success.
fn git_at(dir: &Path, secs: i64, args: &[&str]) {
    let date = format!("@{secs} +0000");
    let status = StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Ada")
        .env("GIT_AUTHOR_EMAIL", "ada@example.com")
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_NAME", "Ada")
        .env("GIT_COMMITTER_EMAIL", "ada@example.com")
        .env("GIT_COMMITTER_DATE", &date)
        .status()
        .expect("spawn git");
    assert!(status.success(), "git {args:?} failed");
}

/// A repo with `src/work.rs` committed twice: once 200 days ago (long
/// window only) and once 5 days ago (recent window).
fn repo_two_commits() -> TempDir {
    let now = now();
    let dir = tempfile::tempdir().expect("tempdir");
    git_at(dir.path(), now, &["init", "-q", "-b", "main"]);
    git_at(dir.path(), now, &["config", "commit.gpgsign", "false"]);
    std::fs::create_dir(dir.path().join("src")).expect("mkdir");
    std::fs::write(dir.path().join("src/work.rs"), "fn a() {}\n").expect("write");
    git_at(dir.path(), now - 200 * DAY, &["add", "."]);
    git_at(
        dir.path(),
        now - 200 * DAY,
        &["commit", "-q", "-m", "add work"],
    );
    std::fs::write(dir.path().join("src/work.rs"), "fn a() {}\nfn b() {}\n").expect("write");
    git_at(
        dir.path(),
        now - 5 * DAY,
        &["commit", "-aqm", "fix bug in work"],
    );
    dir
}

#[test]
fn vcs_json_ranks_the_tracked_file() {
    let repo = repo_two_commits();
    let assert = cli()
        .current_dir(repo.path())
        .args(["vcs", "--paths", ".", "--format", "json"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json");

    let files = doc["files"].as_array().expect("files array");
    let work = files
        .iter()
        .find(|f| f["path"] == "src/work.rs")
        .expect("src/work.rs ranked");
    assert_eq!(work["commits_long"], 2);
    assert_eq!(work["commits_recent"], 1, "only the 5-day commit is recent");
    assert_eq!(
        work["bug_fix_commits"], 1,
        "the second commit message says 'fix bug'"
    );
    assert_eq!(work["authors_long"], 1);
    assert_eq!(doc["long_window_days"], 365);
}

#[test]
fn vcs_table_is_the_default_output() {
    let repo = repo_two_commits();
    cli()
        .current_dir(repo.path())
        .args(["vcs", "--paths", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("RANK").and(predicate::str::contains("src/work.rs")));
}

#[test]
fn metrics_vcs_attaches_a_block_with_hotspot() {
    let repo = repo_two_commits();
    let assert = cli()
        .current_dir(repo.path())
        .args([
            "metrics",
            "--vcs",
            "--paths",
            "src/work.rs",
            "--format",
            "json",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    let vcs = &doc["metrics"]["vcs"];
    assert!(vcs.is_object(), "metrics.vcs block present");
    assert_eq!(vcs["commits_long"], 2);
    // hotspot = cyclomatic_sum × churn_recent; both factors are positive
    // for this fixture, so the product must be strictly positive — a
    // zeroed or dropped-multiplier bug fails this, unlike `is_number`.
    let hotspot = vcs["hotspot_score"]
        .as_f64()
        .expect("hotspot_score is a number");
    assert!(hotspot > 0.0, "hotspot_score positive, got {hotspot}");
}

#[test]
fn metrics_without_vcs_flag_has_no_block() {
    let repo = repo_two_commits();
    let assert = cli()
        .current_dir(repo.path())
        .args(["metrics", "--paths", "src/work.rs", "--format", "json"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert!(
        doc["metrics"].get("vcs").is_none(),
        "no vcs block without --vcs"
    );
}

#[test]
fn vcs_outside_a_repo_errors_with_nonzero_exit() {
    let dir = tempfile::tempdir().expect("tempdir");
    cli()
        .current_dir(dir.path())
        .args(["vcs", "--paths", "."])
        .assert()
        .failure()
        .stderr(predicate::str::contains("version-control"));
}

#[test]
fn metrics_vcs_outside_repo_warns_and_still_emits() {
    // `--vcs` is an additive opt-in: outside a git repo it must NOT abort
    // the metrics run — it warns and omits the vcs block (issue #328).
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("work.rs"), "fn a() {}\n").expect("write");
    let assert = cli()
        .current_dir(dir.path())
        .args(["metrics", "--vcs", "--paths", "work.rs", "--format", "json"])
        .assert()
        .success()
        .stderr(predicate::str::contains("warning"));
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json still emitted");
    assert!(
        doc["metrics"].get("cyclomatic").is_some(),
        "AST metrics still present"
    );
    assert!(
        doc["metrics"].get("vcs").is_none(),
        "vcs block omitted outside a repo"
    );
}

#[test]
fn vcs_csv_format_emits_header() {
    let repo = repo_two_commits();
    cli()
        .current_dir(repo.path())
        .args(["vcs", "--paths", ".", "--format", "csv"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("path,risk_score,commits_long")
                .and(predicate::str::contains("src/work.rs")),
        );
}

#[test]
fn vcs_emit_author_details_controls_author_ids() {
    let repo = repo_two_commits();
    // Returns the `author_ids` field via the object accessor so an
    // *absent* key (serde-skipped) is distinguished from an explicit
    // `null` — indexing would conflate the two.
    let ids = |extra: &[&str]| -> Option<serde_json::Value> {
        let mut args = vec!["vcs", "--paths", ".", "--format", "json"];
        args.extend_from_slice(extra);
        let assert = cli().current_dir(repo.path()).args(args).assert().success();
        let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
        let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json");
        doc["files"][0].get("author_ids").cloned()
    };
    // Off by default; opt-in surfaces a (sorted) array of hashed ids.
    assert!(ids(&[]).is_none(), "author_ids key absent by default");
    let with = ids(&["--emit-author-details"]).expect("author_ids present under the flag");
    let arr = with.as_array().expect("author_ids is an array");
    assert!(!arr.is_empty());
    assert!(
        arr[0].as_str().is_some_and(|h| h.len() == 64),
        "SHA-256 hex"
    );
}

#[test]
fn vcs_include_deleted_surfaces_removed_file() {
    let now = now();
    let repo = tempfile::tempdir().expect("tempdir");
    git_at(repo.path(), now, &["init", "-q", "-b", "main"]);
    git_at(repo.path(), now, &["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.path().join("gone.rs"), "fn g() {}\n").expect("write");
    git_at(repo.path(), now - 30 * DAY, &["add", "."]);
    git_at(repo.path(), now - 30 * DAY, &["commit", "-qm", "add gone"]);
    git_at(repo.path(), now - 10 * DAY, &["rm", "-q", "gone.rs"]);
    std::fs::write(repo.path().join("kept.rs"), "fn k() {}\n").expect("write");
    git_at(repo.path(), now - 10 * DAY, &["add", "."]);
    git_at(
        repo.path(),
        now - 10 * DAY,
        &["commit", "-qm", "remove gone"],
    );

    let paths = |extra: &[&str]| -> Vec<String> {
        let mut args = vec!["vcs", "--paths", ".", "--format", "json"];
        args.extend_from_slice(extra);
        let assert = cli().current_dir(repo.path()).args(args).assert().success();
        let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
        let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json");
        doc["files"]
            .as_array()
            .expect("files")
            .iter()
            .map(|f| f["path"].as_str().unwrap_or_default().to_owned())
            .collect()
    };
    // Default ranking omits the deleted file (it is not on disk); the
    // opt-in pulls it from the index via `append_deleted_entries`.
    assert!(!paths(&[]).iter().any(|p| p == "gone.rs"));
    assert!(
        paths(&["--include-deleted"]).iter().any(|p| p == "gone.rs"),
        "deleted file surfaced with --include-deleted"
    );
}
