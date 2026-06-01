//! Integration tests for `bca diff --since <ref>` (issue #492).
//!
//! These build a throwaway git repo with two revisions of a source
//! file whose metrics differ, then drive the `bca` binary end-to-end:
//! the before side is materialized from a git ref, the after side is
//! the working tree (or an explicit directory). They also cover the
//! hard-error contract (missing ref, non-git checkout, ambiguous
//! positionals) and assert no temp trees survive a run.

// The metric deltas asserted here are exact small integers (cyclomatic
// counts), so float equality is intentional and reliable.
#![allow(clippy::float_cmp)]

use std::fs;
use std::path::Path;
use std::process::Command as StdCommand;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

mod common;

fn cli() -> Command {
    common::bca_command()
}

/// A function with no branching: cyclomatic complexity 1.
const FLAT_SOURCE: &str = "fn work(x: i32) -> i32 {\n    x + 1\n}\n";

/// The same function grown three extra decision points (one `if`, one
/// `match` with two non-default arms). Cyclomatic complexity rises by a
/// fixed, hand-countable amount so the delta assertion is exact.
const BRANCHY_SOURCE: &str = "fn work(x: i32) -> i32 {\n    if x > 0 {\n        return x;\n    }\n    match x {\n        -1 => 1,\n        -2 => 2,\n        other => other,\n    }\n}\n";

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

/// Build a git repo in a fresh tempdir holding `src/work.rs` with
/// `FLAT_SOURCE`, committed once. Returns the tempdir guard.
fn repo_with_flat_commit() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    git(dir.path(), &["init", "-q"]);
    fs::create_dir(dir.path().join("src")).expect("mkdir src");
    fs::write(dir.path().join("src/work.rs"), FLAT_SOURCE).expect("write flat");
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "-m", "flat"]);
    dir
}

#[test]
fn since_diffs_working_tree_against_first_commit() {
    let repo = repo_with_flat_commit();
    // Grow the working tree (uncommitted) so the after side picks it up.
    fs::write(repo.path().join("src/work.rs"), BRANCHY_SOURCE).expect("write branchy");

    // before = HEAD (flat), after = working tree (branchy). The file's
    // cyclomatic.sum is the sum over its spaces: the implicit file/unit
    // space contributes 1, plus the `work` function. Flat: 1 (file) +
    // 1 (work) = 2. Branchy: 1 (file) + (1 base + 1 `if` + 2 non-default
    // match arms = 5) = 6. So cyclomatic.sum moves 2 -> 6.
    let assert = cli()
        .current_dir(repo.path())
        .args([
            "diff", "--since", "HEAD", "--paths", "src", "--format", "json",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json");

    // Locate the cyclomatic.sum field delta in the JSON output.
    let (old, new) = cyclomatic_sum_delta(&doc).expect("cyclomatic.sum delta present");
    // expected: file-level cyclomatic.sum 2 (flat) -> 6 (branchy), per
    // the derivation above.
    assert_eq!(old, 2.0, "before-side cyclomatic.sum");
    assert_eq!(new, 6.0, "after-side cyclomatic.sum");
}

#[test]
fn since_with_explicit_after_directory() {
    let repo = repo_with_flat_commit();
    // Replace the working tree with the branchy form and point the
    // after side at the repo root explicitly (positional `.`), so both
    // sides are rooted at the same layout (the before side is the whole
    // ref tree). `--paths src` scopes both walks to the same subtree.
    fs::write(repo.path().join("src/work.rs"), BRANCHY_SOURCE).expect("write branchy");

    let assert = cli()
        .current_dir(repo.path())
        .args([
            "diff", "--since", "HEAD", ".", "--paths", "src", "--format", "json",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    let (old, new) = cyclomatic_sum_delta(&doc).expect("cyclomatic.sum delta present");
    assert_eq!(old, 2.0);
    assert_eq!(new, 6.0);
}

#[test]
fn since_missing_ref_exits_one_with_message() {
    let repo = repo_with_flat_commit();
    cli()
        .current_dir(repo.path())
        .args(["diff", "--since", "no-such-ref-xyz", "--paths", "src"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("does not resolve to a tree"));
}

#[test]
fn since_outside_git_checkout_exits_one_with_message() {
    // A fresh tempdir with no `.git` ancestor.
    let dir = tempfile::tempdir().expect("tempdir");
    fs::create_dir(dir.path().join("src")).expect("mkdir");
    fs::write(dir.path().join("src/work.rs"), FLAT_SOURCE).expect("write");
    cli()
        .current_dir(dir.path())
        .args(["diff", "--since", "HEAD", "--paths", "src"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("git checkout"));
}

#[test]
fn since_rejects_two_positionals() {
    let repo = repo_with_flat_commit();
    cli()
        .current_dir(repo.path())
        .args(["diff", "--since", "HEAD", "src", "other"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("at most one positional"));
}

#[test]
fn since_leaves_no_temp_trees_behind() {
    let repo = repo_with_flat_commit();
    fs::write(repo.path().join("src/work.rs"), BRANCHY_SOURCE).expect("write branchy");

    // Anchor a private TMPDIR so this run's temp trees are isolated and
    // observable. TempDir auto-removes on drop, so after a clean exit
    // the dir must hold no leftover extraction trees.
    let tmp = tempfile::tempdir().expect("tmp scratch");
    cli()
        .current_dir(repo.path())
        .env("TMPDIR", tmp.path())
        .args(["diff", "--since", "HEAD", "--paths", "src"])
        .assert()
        .success();

    let leftovers: Vec<_> = fs::read_dir(tmp.path())
        .expect("read scratch")
        .filter_map(Result::ok)
        .collect();
    assert!(
        leftovers.is_empty(),
        "expected no leftover temp trees, found: {:?}",
        leftovers
            .iter()
            .map(std::fs::DirEntry::path)
            .collect::<Vec<_>>()
    );
}

#[test]
fn since_from_subdir_pairs_against_repo_root() {
    // Regression: `bca diff --since` must pair files correctly when run
    // from a subdirectory. The before side is a `git archive` of the
    // whole ref tree (rooted at the repo top), so the working-tree after
    // side must also anchor at the repo root — not the process CWD —
    // else the keys (`src/work.rs` vs a CWD-relative form) never match
    // and every file reports as a meaningless add+remove instead of a
    // delta. Run from `src/`; the delta must still resolve to 2 -> 6.
    let repo = repo_with_flat_commit();
    fs::write(repo.path().join("src/work.rs"), BRANCHY_SOURCE).expect("write branchy");

    let assert = cli()
        .current_dir(repo.path().join("src"))
        .args([
            "diff", "--since", "HEAD", "--paths", "src", "--format", "json",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    // expected: pairs as a delta (2 -> 6), NOT add+remove. Pre-fix the
    // after side anchored at the `src/` CWD, so no `cyclomatic.changed`
    // row existed and this `expect` panicked.
    let (old, new) = cyclomatic_sum_delta(&doc).expect("cyclomatic.sum delta present");
    assert_eq!(old, 2.0, "before-side cyclomatic.sum");
    assert_eq!(new, 6.0, "after-side cyclomatic.sum");
}

#[test]
fn since_rejects_absolute_paths() {
    // An absolute `--paths` cannot address the extracted `<ref>` tree
    // (it points at the live filesystem), which would silently walk the
    // current tree for both sides. Reject it with a clear message.
    let repo = repo_with_flat_commit();
    let abs = repo.path().join("src");
    cli()
        .current_dir(repo.path())
        .args(["diff", "--since", "HEAD", "--paths"])
        .arg(&abs)
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("--paths must be relative"));
}

#[test]
fn since_rejects_dash_leading_ref() {
    // A dash-leading ref would reach git's option parser; the explicit
    // `--since=-x` form binds the value, so the dash guard (not clap)
    // produces the diagnostic.
    let repo = repo_with_flat_commit();
    cli()
        .current_dir(repo.path())
        .args(["diff", "--since=-x", "--paths", "src"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("starts with `-`"));
}

/// Pull `(old, new)` for the `cyclomatic.sum` field out of the
/// `--format json` diff document, searching the `cyclomatic` bucket's
/// changed entries.
fn cyclomatic_sum_delta(doc: &serde_json::Value) -> Option<(f64, f64)> {
    let buckets = doc.get("buckets")?;
    // The JSON shape keys buckets by metric name; each carries a
    // `changed` array of `{file, field, old, new}` rows.
    let cyclo = buckets.get("cyclomatic")?;
    let changed = cyclo.get("changed")?.as_array()?;
    for row in changed {
        if row.get("field").and_then(serde_json::Value::as_str) == Some("sum") {
            let old = row.get("old")?.as_f64()?;
            let new = row.get("new")?.as_f64()?;
            return Some((old, new));
        }
    }
    None
}
