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

/// A small unified diff touching two subsystems, written to a temp file.
const SAMPLE_DIFF: &str = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,1 +1,3 @@
 keep
+added1
+added2
diff --git a/docs/b.md b/docs/b.md
--- a/docs/b.md
+++ b/docs/b.md
@@ -1,1 +1,2 @@
 title
+body
";

#[test]
fn diff_mode_emits_partial_report_with_unavailable_groups() {
    // `bca vcs jit --diff <file>` (issue #580): a bare diff scores only the
    // size + diffusion groups, and the JSON must mark the other groups
    // UNAVAILABLE (absent), not present as zero. No repository is needed.
    let dir = tempfile::tempdir().expect("tempdir");
    let diff_path = dir.path().join("change.diff");
    std::fs::write(&diff_path, SAMPLE_DIFF).expect("write diff");

    let output = bca(dir.path())
        .args(["vcs", "jit", "--diff"])
        .arg(&diff_path)
        .args(["-O", "json"])
        .output()
        .expect("run bca vcs jit --diff");
    assert!(output.status.success(), "diff scoring should exit 0");

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("diff output is valid JSON");
    assert_eq!(
        json["source"], "diff",
        "report self-identifies as diff mode"
    );
    assert!(json["partial_score"].is_number());
    assert_eq!(json["size"]["files_touched"], 2);
    assert_eq!(json["size"]["lines_added"], 3);
    assert_eq!(json["diffusion"]["subsystems"], 2, "src + docs");
    // The unavailable groups must be absent — distinct from a real zero.
    let obj = json.as_object().expect("object");
    for absent in ["history", "experience", "purpose", "commit", "score"] {
        assert!(
            !obj.contains_key(absent),
            "diff report must omit `{absent}` so it is not misread as low risk"
        );
    }
}

#[test]
fn diff_mode_reads_stdin_with_dash() {
    // `--diff -` reads the diff from stdin, the conventional CLI marker.
    let dir = tempfile::tempdir().expect("tempdir");
    bca(dir.path())
        .args(["vcs", "jit", "--diff", "-", "-O", "json"])
        .write_stdin(SAMPLE_DIFF)
        .assert()
        .success()
        .stdout(predicate::str::contains("\"source\":\"diff\""));
}

#[test]
fn diff_mode_malformed_input_errors() {
    let dir = tempfile::tempdir().expect("tempdir");
    let diff_path = dir.path().join("bad.diff");
    std::fs::write(
        &diff_path,
        "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ garbage @@\n",
    )
    .expect("write diff");
    bca(dir.path())
        .args(["vcs", "jit", "--diff"])
        .arg(&diff_path)
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("invalid unified diff"));
}

#[test]
fn diff_and_commit_spec_are_mutually_exclusive() {
    let dir = tempfile::tempdir().expect("tempdir");
    let diff_path = dir.path().join("change.diff");
    std::fs::write(&diff_path, SAMPLE_DIFF).expect("write diff");
    // Supplying both a positional commit and --diff is a clap usage
    // error (exit 1 — usage errors map to the tool-error code, not the
    // `--fail-over` gate band; #594).
    bca(dir.path())
        .args(["vcs", "jit", "HEAD", "--diff"])
        .arg(&diff_path)
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("cannot be used with"));
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
