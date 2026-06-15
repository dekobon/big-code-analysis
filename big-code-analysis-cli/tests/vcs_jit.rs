//! CLI integration tests for `bca vcs jit` (issue #331): the JSON shape,
//! the `--fail-above` CI gate exit code, and the outside-a-repo error.
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
    assert_eq!(json["jit_schema_version"], 3);
    assert_eq!(json["jit_score_version"], 1);
    assert_eq!(json["source"], "commit", "commit-mode discriminator (#642)");
    assert!(json["risk_score"].is_number());
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
fn fail_above_below_threshold_exits_zero() {
    let repo = one_commit_repo("initial import");
    bca(repo.path())
        .args([
            "vcs",
            "commit",
            "HEAD",
            "-O",
            "json",
            "--fail-above",
            "9999",
        ])
        .assert()
        .success();
}

#[test]
fn fail_above_at_or_above_threshold_exits_two() {
    let repo = one_commit_repo("initial import");
    // Every commit scores >= 0, so a zero threshold always trips the gate.
    bca(repo.path())
        .args(["vcs", "commit", "HEAD", "-O", "json", "--fail-above", "0"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("fail-above threshold"));
}

/// #850: a non-finite (`nan`, `inf`, `-inf`) or negative `--fail-above`
/// threshold would silently disable the gate (`score >= NaN`/`>= inf`
/// is always `false`) or trip it on every commit (negative). It must be
/// rejected at parse time (exit 1, the tool-error code) with a message
/// naming the flag, mirroring the `check` threshold parser.
#[test]
fn fail_above_rejects_non_finite_and_negative() {
    // Leading-`-` values (`-1`, `-inf`) must use the `--flag=value` form
    // so clap reads them as the flag's value rather than a new flag —
    // the standard CLI convention for negative numeric arguments.
    let cases = [
        vec!["--fail-above", "nan"],
        vec!["--fail-above", "inf"],
        vec!["--fail-above=-1"],
        vec!["--fail-above=-inf"],
    ];
    for case in cases {
        let repo = one_commit_repo("initial import");
        let mut args = vec!["vcs", "commit", "HEAD", "-O", "json"];
        args.extend(case.iter().copied());
        bca(repo.path())
            .args(&args)
            .assert()
            .code(1)
            .stderr(predicate::str::contains(
                "must be a finite non-negative number",
            ))
            .stderr(predicate::str::contains("fail-above"));
    }
}

/// #850: a valid finite non-negative threshold is still accepted — `0`
/// trips on every commit (exit 2), a large value passes (exit 0).
#[test]
fn fail_above_accepts_finite_non_negative() {
    let repo = one_commit_repo("initial import");
    bca(repo.path())
        .args(["vcs", "commit", "HEAD", "-O", "json", "--fail-above", "0"])
        .assert()
        .code(2);
    bca(repo.path())
        .args([
            "vcs",
            "commit",
            "HEAD",
            "-O",
            "json",
            "--fail-above",
            "9999",
        ])
        .assert()
        .success();
}

/// #603: the renamed subcommand (`jit` → `commit`) and flag
/// (`--fail-over` → `--fail-above`) keep their old spellings working for
/// one release cycle via hidden aliases; the gate still emits the new
/// `vcs commit: ... fail-above threshold` message regardless.
#[test]
fn jit_and_fail_over_aliases_still_work() {
    let repo = one_commit_repo("initial import");
    bca(repo.path())
        .args(["vcs", "jit", "HEAD", "-O", "json", "--fail-over", "0"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("vcs commit:"))
        .stderr(predicate::str::contains("fail-above threshold"));
}

/// #834: the `jit` -> `commit` deprecation warning must still fire when a
/// `global = true` flag (here `-w`) precedes the `jit` subcommand token —
/// the warning's whole reason to exist (#646) was being dropped whenever
/// any global flag shifted `jit` off the `vcs + 1` position.
#[test]
fn jit_warns_with_global_flag_before_subcommand() {
    let repo = one_commit_repo("initial import");
    bca(repo.path())
        .args(["vcs", "-w", "jit", "HEAD", "-O", "json"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "`jit` is deprecated; use `commit`",
        ));
}

/// #834: the value-taking either-position window flag
/// (`--long-window 6mo`) before `jit` must skip its value and still draw
/// the deprecation warning — the documented `bca vcs --long-window 6mo
/// jit` ordering.
#[test]
fn jit_warns_with_value_taking_flag_before_subcommand() {
    let repo = one_commit_repo("initial import");
    bca(repo.path())
        .args(["vcs", "--long-window", "6mo", "jit", "HEAD", "-O", "json"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "`jit` is deprecated; use `commit`",
        ));
}

/// #832: `--fail-over` (the deprecated spelling of `--fail-above`) must
/// emit the one-cycle deprecation warning, not merely be honored. Pinned
/// here because the flag lives on `vcs commit`, which needs a real repo.
#[test]
fn fail_over_alias_warns() {
    let repo = one_commit_repo("initial import");
    bca(repo.path())
        .args(["vcs", "commit", "HEAD", "-O", "json", "--fail-over", "9999"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "`--fail-over` is deprecated; use `--fail-above`",
        ));
}

/// #603: the deprecated `jit` alias is hidden from `bca vcs --help` while
/// the canonical `commit` is listed.
#[test]
fn commit_listed_in_help_jit_hidden() {
    let tmp = std::env::temp_dir();
    let out = bca(&tmp)
        .args(["vcs", "--help"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let help = String::from_utf8(out).expect("utf8");
    // Pin to the subcommand *listing* line (`commit  <description>`), not
    // the word "commit" in description prose — which appears regardless of
    // whether the canonical subcommand is listed (#913). Mirrors the
    // line-anchored structure of the negative `jit` assertion below.
    assert!(
        help.lines().any(|l| {
            let t = l.trim_start();
            t.starts_with("commit ") || t == "commit"
        }),
        "vcs --help should list `commit` as a subcommand: {help}"
    );
    // The hidden alias must not appear as its own listed subcommand line.
    assert!(
        !help
            .lines()
            .any(|l| l.trim_start().starts_with("jit ") || l.trim_start() == "jit"),
        "`jit` alias should be hidden from help: {help}"
    );
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
    assert!(json["partial_risk_score"].is_number());
    assert_eq!(json["size"]["files_touched"], 2);
    assert_eq!(json["size"]["lines_added"], 3);
    assert_eq!(json["diffusion"]["subsystems"], 2, "src + docs");
    // The unavailable groups must be absent — distinct from a real zero.
    let obj = json.as_object().expect("object");
    for absent in ["history", "experience", "purpose", "commit", "risk_score"] {
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

/// A history-tuning flag (`--long-window`) is accepted in the *subcommand*
/// position (`vcs jit --long-window …`), the natural git/cargo/rg ordering,
/// and genuinely tunes the walk — the echoed `long_window_days` reflects the
/// supplied 6mo window, not the 12mo default (issue #598).
#[test]
fn jit_accepts_long_window_in_subcommand_position() {
    let repo = one_commit_repo("initial import");
    let output = bca(repo.path())
        .args(["vcs", "jit", "--long-window", "6mo", "-O", "json"])
        .output()
        .expect("run bca vcs jit");
    assert!(
        output.status.success(),
        "subcommand-position flag should parse"
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("jit output is valid JSON");
    // 6mo resolves to 183 days; the 12mo default would echo 365 (or 366).
    assert_eq!(json["long_window_days"], 183);
}

/// The historical parent-position spelling (`vcs --long-window … jit`) keeps
/// working and resolves to the same tuned window — `global = true` makes the
/// flag valid in either position rather than moving it (issue #598).
#[test]
fn jit_accepts_long_window_in_parent_position() {
    let repo = one_commit_repo("initial import");
    let output = bca(repo.path())
        .args(["vcs", "--long-window", "6mo", "jit", "-O", "json"])
        .output()
        .expect("run bca vcs jit");
    assert!(
        output.status.success(),
        "parent-position flag should still parse"
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("jit output is valid JSON");
    assert_eq!(json["long_window_days"], 183);
}

/// `--ref` is meaningless under `jit` (the commit is positional), so it is a
/// usage error (exit 1) rather than the prior silent ignore. The message
/// names both the conflicting flag and the subcommand (issue #598). Pinned in
/// both flag positions because `--ref` is `global = true`.
#[test]
fn ref_under_jit_is_a_usage_error() {
    let repo = one_commit_repo("initial import");
    // Exercised in both flag positions and under both the canonical
    // `commit` spelling and the deprecated `jit` alias (#603); the error
    // names the canonical `vcs commit` regardless of how it was invoked.
    for argv in [
        ["vcs", "--ref", "v1.0", "commit"].as_slice(),
        ["vcs", "commit", "--ref", "v1.0"].as_slice(),
        ["vcs", "--ref", "v1.0", "jit"].as_slice(),
        ["vcs", "jit", "--ref", "v1.0"].as_slice(),
    ] {
        bca(repo.path())
            .args(argv)
            .assert()
            .failure()
            .code(1)
            .stderr(predicate::str::contains("--ref").and(predicate::str::contains("vcs commit")));
    }
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
