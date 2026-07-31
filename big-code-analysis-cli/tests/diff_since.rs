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

/// #692: `bca diff --exit-code` over a changed tree exits with the
/// metric-gate code (2); the default (no flag) exits 0.
#[test]
fn exit_code_flag_returns_two_on_changed_diff() {
    let repo = repo_with_flat_commit();
    fs::write(repo.path().join("src/work.rs"), BRANCHY_SOURCE).expect("write branchy");
    cli()
        .current_dir(repo.path())
        .args(["diff", "--since", "HEAD", "--paths", "src", "--exit-code"])
        .assert()
        .code(2);
    // Without the flag, the same changed diff exits 0 (informational).
    cli()
        .current_dir(repo.path())
        .args(["diff", "--since", "HEAD", "--paths", "src"])
        .assert()
        .success();
}

/// #692: `bca diff --exit-code` over an identical tree (working tree ==
/// HEAD) exits 0 — the filtered diff is empty.
#[test]
fn exit_code_flag_returns_zero_on_identical_diff() {
    let repo = repo_with_flat_commit();
    // No working-tree edit: before == after, so the diff is empty.
    cli()
        .current_dir(repo.path())
        .args(["diff", "--since", "HEAD", "--paths", "src", "--exit-code"])
        .assert()
        .success();
}

/// #692: a tool error (unresolvable ref) under `--exit-code` still exits
/// 1, keeping the tool-error signal distinct from the metric gate (2).
#[test]
fn exit_code_flag_tool_error_still_exits_one() {
    let repo = repo_with_flat_commit();
    cli()
        .current_dir(repo.path())
        .args(["diff", "--since", "no-such-ref", "--exit-code"])
        .assert()
        .code(1);
}

#[test]
fn output_flag_writes_diff_to_file_and_stdout_stays_empty() {
    let repo = repo_with_flat_commit();
    fs::write(repo.path().join("src/work.rs"), BRANCHY_SOURCE).expect("write branchy");
    cli()
        .current_dir(repo.path())
        .args(["diff", "--since", "HEAD", "--output", "diff.txt"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
    let written = fs::read_to_string(repo.path().join("diff.txt")).expect("output file written");
    assert!(
        written.contains("## cyclomatic"),
        "file content missing diff body: {written}"
    );
    // The report echoes the path as the upstream metrics output recorded
    // it, which uses the platform separator (`src\work.rs` on Windows),
    // so normalize before matching the Unix-style relative path.
    assert!(
        written.replace('\\', "/").contains("src/work.rs"),
        "file content: {written}"
    );
}

#[test]
fn short_output_flag_is_accepted() {
    let repo = repo_with_flat_commit();
    fs::write(repo.path().join("src/work.rs"), BRANCHY_SOURCE).expect("write branchy");
    cli()
        .current_dir(repo.path())
        .args(["diff", "--since", "HEAD", "--paths", "src", "-o", "d.txt"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
    assert!(repo.path().join("d.txt").exists());
}

#[test]
fn strip_prefix_trims_displayed_paths() {
    let repo = repo_with_flat_commit();
    fs::write(repo.path().join("src/work.rs"), BRANCHY_SOURCE).expect("write branchy");
    // Diff the whole tree (no `--paths` scope) so file keys carry the
    // `./src/` prefix; `--strip-prefix ./src/` must then trim it to
    // `work.rs` in the per-file change rows (e.g. `work.rs.sum` under
    // `cyclomatic`).
    cli()
        .current_dir(repo.path())
        .args(["diff", "--since", "HEAD", "--strip-prefix", "./src/"])
        .assert()
        .success()
        .stdout(predicate::str::contains("work.rs.sum"))
        .stdout(predicate::str::contains("src/work.rs").not());
}

#[test]
fn since_positional_scopes_subtree() {
    // #497: the `--since` positional is a relative *scope* (like
    // `--paths`), applied to both sides — NOT an alternate after-root.
    // A subtree positional (`src`) must pair the same files on each
    // side. Before the fix it re-rooted only the after walk, so the
    // before keys (`src/work.rs`) and after keys (`work.rs`) never
    // matched and every file reported as both added and removed with no
    // delta — the assertion below would find no `cyclomatic.sum` change.
    let repo = repo_with_flat_commit();
    fs::write(repo.path().join("src/work.rs"), BRANCHY_SOURCE).expect("write branchy");

    let assert = cli()
        .current_dir(repo.path())
        .args(["diff", "--since", "HEAD", "src", "--format", "json"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    // A real paired delta proves the keys lined up across both sides.
    let (old, new) = cyclomatic_sum_delta(&doc).expect("cyclomatic.sum delta present");
    assert_eq!(old, 2.0);
    assert_eq!(new, 6.0);
    // The scope confines the diff to `src/`; nothing outside it leaks in
    // as a spurious add/remove.
    let added = doc["added_files"].as_array().expect("added_files array");
    let removed = doc["removed_files"]
        .as_array()
        .expect("removed_files array");
    assert!(added.is_empty(), "no spurious added files: {added:?}");
    assert!(removed.is_empty(), "no spurious removed files: {removed:?}");
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
        .stderr(predicate::str::contains("paths must be relative"));
}

#[test]
fn since_rejects_dotdot_escaping_paths() {
    // A relative `--paths` that escapes its walk root via `..` addresses
    // different trees on the before/after sides (the `/tmp/…` extraction
    // vs the repo root), silently mis-pairing the diff. Reject it with a
    // clear message rather than emit a bogus all-zero / partial diff
    // (#704).
    let repo = repo_with_flat_commit();
    cli()
        .current_dir(repo.path())
        .args(["diff", "--since", "HEAD", "--paths", "../escape"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("must stay within the tree"));
}

#[test]
fn since_rejects_dotdot_escaping_positional() {
    // The same `..`-escape guard applies to the optional positional
    // scope, not just `--paths` (#704).
    let repo = repo_with_flat_commit();
    cli()
        .current_dir(repo.path())
        .args(["diff", "--since", "HEAD", ".."])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("must stay within the tree"));
}

#[test]
fn since_allows_interior_dotdot_that_stays_in_tree() {
    // A `..` that nets back inside the tree (`src/../src`) does NOT
    // escape, so it must be accepted — the guard rejects only net escapes
    // (#704). The diff succeeds (no offenders, exit 0 without --exit-code).
    let repo = repo_with_flat_commit();
    cli()
        .current_dir(repo.path())
        .args(["diff", "--since", "HEAD", "--paths", "src/../src"])
        .assert()
        .success();
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

#[test]
fn since_rejects_paths_from() {
    // `--paths-from` names a file list that cannot resolve consistently
    // against both the extracted <ref> tree and the working tree, so it
    // is rejected rather than silently ignored (which would walk the
    // whole tree on both sides).
    let repo = repo_with_flat_commit();
    fs::write(repo.path().join("files.txt"), "src/work.rs\n").expect("write list");
    cli()
        .current_dir(repo.path())
        .args(["diff", "--since", "HEAD", "--paths-from", "files.txt"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("--paths-from is not supported"));
}

/// #662: a typo'd `--metric` errors at parse time (exit 1) with the
/// known-names list and a did-you-mean, instead of silently filtering
/// the diff to nothing and exiting 0.
#[test]
fn unknown_metric_errors_with_did_you_mean() {
    let repo = repo_with_flat_commit();
    cli()
        .current_dir(repo.path())
        .args(["diff", "--since", "HEAD", "--metric", "cylomatic"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("unknown metric"))
        .stderr(predicate::str::contains("did you mean"))
        .stderr(predicate::str::contains("cyclomatic"));
}

/// #662: valid `--metric` spellings — a canonical bucket name, the #514
/// bare `loc` sub-metric alias, and a dotted `check --threshold` id — all
/// pass validation, run, AND actually populate their selected bucket.
///
/// Asserting only exit 0 (#902) cannot tell a real metric-filtered diff
/// from the silent-empty regression #662 was filed against: a valid
/// spelling that matches no bucket would filter the diff to nothing and
/// still exit 0. Each spelling here drives a known `FLAT_SOURCE` ->
/// `BRANCHY_SOURCE` delta, so its selected bucket must carry a non-empty
/// `changed` array. The dotted `cyclomatic.modified` id selects the
/// `cyclomatic` bucket (see the `--format json` shape).
#[test]
fn valid_metric_spellings_are_accepted() {
    let repo = repo_with_flat_commit();
    fs::write(repo.path().join("src/work.rs"), BRANCHY_SOURCE).expect("write branchy");
    for (name, bucket) in [
        ("cyclomatic", "cyclomatic"),
        ("sloc", "sloc"),
        ("cyclomatic.modified", "cyclomatic"),
    ] {
        let output = cli()
            .current_dir(repo.path())
            .args([
                "diff", "--since", "HEAD", "src", "--metric", name, "--format", "json",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let doc: serde_json::Value = serde_json::from_slice(&output)
            .unwrap_or_else(|e| panic!("diff JSON for `{name}` is invalid: {e}"));
        let changed = doc
            .get("buckets")
            .and_then(|b| b.get(bucket))
            .and_then(|b| b.get("changed"))
            .and_then(serde_json::Value::as_array)
            .unwrap_or_else(|| {
                panic!(
                    "spelling `{name}` must populate the `{bucket}` bucket's `changed` array: {doc}"
                )
            });
        assert!(
            !changed.is_empty(),
            "spelling `{name}` filtered the `{bucket}` diff to empty (silent-empty regression, #662/#902): {doc}"
        );
    }
}

/// #838: an `export-ignore`'d source file present unchanged on both
/// sides must NOT be reported as added. The former before-side
/// materialization shelled out to `git archive`, which silently honours
/// the `export-ignore` gitattribute and drops the file from the before
/// tree — so it paired as a full positive "added" delta on every run,
/// even though it had not changed. The `ls-tree` + `cat-file` route
/// never consults gitattributes, so the file is present on both sides
/// and the diff is empty.
#[test]
fn since_keeps_export_ignored_files_on_before_side() {
    let repo = repo_with_flat_commit();
    // Mark the committed source file `export-ignore` and commit the
    // attribute so it lives in the tree at HEAD (where `git archive`
    // would read it). The working-tree (after) copy is left unchanged.
    fs::write(
        repo.path().join(".gitattributes"),
        "src/work.rs export-ignore\n",
    )
    .expect("write .gitattributes");
    git(repo.path(), &["add", ".gitattributes"]);
    git(repo.path(), &["commit", "-q", "-m", "mark export-ignore"]);

    let assert = cli()
        .current_dir(repo.path())
        .args(["diff", "--since", "HEAD", "src", "--format", "json"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json");

    let added = doc["added_files"].as_array().expect("added_files array");
    assert!(
        added.is_empty(),
        "export-ignore'd file must not appear as added: {added:?}"
    );
    // And it must be present on the before side: with the working tree
    // unchanged, there is no delta at all.
    assert!(
        cyclomatic_sum_delta(&doc).is_none(),
        "unchanged export-ignore'd file should yield no metric delta"
    );
}

/// #1098: a file the after-side walk cannot read drops out of that
/// side's `MetricSet` entirely, which pairs as a *removed* file against
/// the before side — a wrong diff, not a missing one. The walk must
/// surface the read failure as a tool error instead, naming the side so
/// the user knows which tree to look at.
///
/// Staged on the after side because that is the reachable one: the
/// before side is a fresh extraction of `<ref>` whose files this process
/// just wrote, and git records no permission bits beyond the executable
/// one, so a mode-000 committed file extracts readable.
#[cfg(unix)]
#[test]
fn since_errors_when_the_after_side_has_an_unreadable_file() {
    use std::os::unix::fs::PermissionsExt;

    let repo = repo_with_flat_commit();
    // A second file, committed so it exists on the before side, then
    // locked in the working tree. Without the guard the after side
    // simply omits it and the diff reports a removed file.
    let extra = repo.path().join("src/extra.rs");
    fs::write(&extra, FLAT_SOURCE).expect("write extra");
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "extra"]);
    fs::set_permissions(&extra, fs::Permissions::from_mode(0o000)).expect("chmod 000");
    // Probe the real capability rather than the uid: root ignores mode
    // bits, and then the scenario cannot be staged at all.
    if fs::read(&extra).is_ok() {
        eprintln!("skipping: this process can read a mode-000 file");
        return;
    }

    cli()
        .current_dir(repo.path())
        .args(["diff", "--since", "HEAD", "src", "--format", "json"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Permission denied"))
        .stderr(predicate::str::contains(
            "diff --since after tree: 1 input file could not be read",
        ))
        // No diff document at all, so the wrong comparison this
        // replaces — `extra.rs` reported as a removed file — cannot be
        // consumed. Before the fix this stream carried a full JSON diff
        // with `extra.rs` in `removed_files`.
        .stdout(predicate::str::is_empty());
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
