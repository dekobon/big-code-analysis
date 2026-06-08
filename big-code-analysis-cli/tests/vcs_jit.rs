//! CLI integration tests for `bca vcs jit` (issue #331): the JSON shape,
//! the `--fail-over` CI gate exit code, and the outside-a-repo error.
//!
//! Each test drives the real `bca` binary against a tiny throwaway git
//! repository built through the `git` CLI with a fixed identity, so the
//! runs are deterministic and depend on no submodule.

use std::path::Path;
use std::process::Command as Git;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

mod common;

/// Build a one-commit git repo under a fresh tempdir and return it.
fn one_commit_repo(message: &str) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "commit.gpgsign", "false"]);
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\nfn b() {}\n").expect("write");
    git(&dir, &["add", "-A"]);
    commit(&dir, message);
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

fn commit(dir: &TempDir, message: &str) {
    let status = Git::new("git")
        .args(["commit", "-q", "--no-verify", "-m", message])
        .current_dir(dir.path())
        .envs([
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_AUTHOR_DATE", "@1700000000 +0000"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_DATE", "@1700000000 +0000"),
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
fn jit_emits_stable_json_shape() {
    let repo = one_commit_repo("initial import");
    let output = bca(repo.path())
        .args(["vcs", "jit", "HEAD", "-O", "json"])
        .output()
        .expect("run bca vcs jit");
    assert!(output.status.success(), "jit should exit 0 inside a repo");

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("jit output is valid JSON");
    assert_eq!(json["jit_schema_version"], 1);
    assert_eq!(json["jit_score_version"], 1);
    assert!(json["score"].is_number());
    // Feature groups and contributions are present and structured.
    for group in ["size", "diffusion", "history", "experience"] {
        assert!(
            json["features"][group].is_object(),
            "features.{group} should be an object"
        );
        assert!(
            json["contributions"][group].is_number(),
            "contributions.{group} should be a number"
        );
    }
    assert_eq!(json["features"]["size"]["lines_added"], 2);
    assert_eq!(json["commit"]["parent_count"], 0, "root commit");
}

#[test]
fn fail_over_below_threshold_exits_zero() {
    let repo = one_commit_repo("initial import");
    bca(repo.path())
        .args(["vcs", "jit", "HEAD", "-O", "json", "--fail-over", "9999"])
        .assert()
        .success();
}

#[test]
fn fail_over_at_or_above_threshold_exits_two() {
    let repo = one_commit_repo("initial import");
    // Every commit scores >= 0, so a zero threshold always trips the gate.
    bca(repo.path())
        .args(["vcs", "jit", "HEAD", "-O", "json", "--fail-over", "0"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("fail-over threshold"));
}

#[test]
fn outside_a_repo_errors() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Exit 1 is the generic tool-error code, so also pin the message to the
    // not-a-repository path — otherwise an unrelated failure (bad arg, panic)
    // exiting 1 would pass this test.
    bca(dir.path())
        .args(["vcs", "jit", "HEAD"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "not inside a supported version-control working tree",
        ));
}
