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
    // The per-file metrics nest under a `vcs` key (#684); the constant
    // stamps live once on the envelope, never per row (#635).
    let work_vcs = &work["vcs"];
    assert_eq!(work_vcs["commits_long"], 2);
    assert_eq!(
        work_vcs["commits_recent"], 1,
        "only the 5-day commit is recent"
    );
    assert_eq!(
        work_vcs["bug_fix_commits"], 1,
        "the second commit message says 'fix bug'"
    );
    assert_eq!(work_vcs["authors_long"], 1);
    assert_eq!(doc["long_window_days"], 365);
    for constant in [
        "vcs_schema_version",
        "risk_score_version",
        "long_window_days",
        "recent_window_days",
    ] {
        assert!(
            work.get(constant).is_none() && work_vcs.get(constant).is_none(),
            "constant `{constant}` must not be repeated per row (#635)"
        );
    }
}

/// #659: `bca vcs --format text` renders the same human ranked table the
/// bare `bca vcs` prints — the human format is now named and selectable
/// here, not only the unnamed default.
#[test]
fn vcs_format_text_renders_the_human_table() {
    let repo = repo_two_commits();
    let default_out = cli()
        .current_dir(repo.path())
        .args(["vcs", "--paths", "."])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text_out = cli()
        .current_dir(repo.path())
        .args(["vcs", "--paths", ".", "--format", "text"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        text_out, default_out,
        "`vcs --format text` must match the default human table"
    );
    assert!(
        String::from_utf8(text_out).unwrap().contains("RISK"),
        "human table carries the RISK column header"
    );
}

/// Count `*.json` cache entries anywhere under `root`.
fn count_cache_entries(root: &Path) -> usize {
    let mut count = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "json") {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn vcs_cache_dir_persists_and_replays_identically() {
    let repo = repo_two_commits();
    let cache = tempfile::tempdir().expect("cache dir");
    let cache_arg = cache.path().to_str().expect("utf8 cache path");

    let run = || {
        let assert = cli()
            .current_dir(repo.path())
            .args([
                "vcs",
                "--paths",
                ".",
                "--format",
                "json",
                "--as-of",
                "@1700000000",
                "--cache-dir",
                cache_arg,
            ])
            .assert()
            .success();
        String::from_utf8(assert.get_output().stdout.clone()).expect("utf8")
    };

    let first = run(); // miss: walks and writes
    assert_eq!(
        count_cache_entries(cache.path()),
        1,
        "an entry is persisted"
    );
    let second = run(); // hit: replays
    assert_eq!(
        first, second,
        "a cache hit is byte-identical to the first run"
    );
}

#[test]
fn vcs_no_cache_writes_nothing() {
    let repo = repo_two_commits();
    let cache = tempfile::tempdir().expect("cache dir");
    cli()
        .current_dir(repo.path())
        .args([
            "vcs",
            "--paths",
            ".",
            "--no-cache",
            "--cache-dir",
            cache.path().to_str().expect("utf8"),
        ])
        .assert()
        .success();
    assert_eq!(
        count_cache_entries(cache.path()),
        0,
        "--no-cache neither reads nor writes the cache"
    );
}

#[test]
fn vcs_clear_cache_wipes_persisted_entries() {
    let repo = repo_two_commits();
    let cache = tempfile::tempdir().expect("cache dir");
    let cache_arg = cache.path().to_str().expect("utf8");

    cli()
        .current_dir(repo.path())
        .args(["vcs", "--paths", ".", "--cache-dir", cache_arg])
        .assert()
        .success();
    assert!(count_cache_entries(cache.path()) >= 1);

    cli()
        .current_dir(repo.path())
        .args([
            "vcs",
            "--paths",
            ".",
            "--no-cache",
            "--clear-cache",
            "--cache-dir",
            cache_arg,
        ])
        .assert()
        .success();
    assert_eq!(
        count_cache_entries(cache.path()),
        0,
        "cleared, not re-primed"
    );
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
fn metrics_vcs_per_function_attaches_nested_blocks() {
    // `--vcs-per-function` implies `--vcs`, so the file-level block is
    // still present, and every nested function space additionally carries
    // a blame-derived block. The fixture's `src/work.rs` ends with two
    // top-level functions, so the JSON tree must show two nested spaces,
    // each with its own `vcs` object.
    let repo = repo_two_commits();
    let assert = cli()
        .current_dir(repo.path())
        .args([
            "metrics",
            "--vcs-per-function",
            "--paths",
            "src/work.rs",
            "--format",
            "json",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json");

    // File-level block still attached (implied --vcs).
    assert!(
        doc["metrics"]["vcs"].is_object(),
        "file-level vcs block present under --vcs-per-function"
    );

    let spaces = doc["spaces"].as_array().expect("nested spaces array");
    let with_vcs: Vec<&serde_json::Value> = spaces
        .iter()
        .filter(|s| s["metrics"]["vcs"].is_object())
        .collect();
    assert_eq!(
        with_vcs.len(),
        2,
        "both nested functions carry a per-function vcs block"
    );
    // Every nested block must carry a per-function hotspot score derived
    // from that function's own cyclomatic sum.
    for space in with_vcs {
        assert!(
            space["metrics"]["vcs"]["hotspot_score"].is_number(),
            "per-function hotspot_score present"
        );
    }
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
        // `author_ids` lives inside the nested per-file `vcs` block (#684).
        doc["files"][0]["vcs"].get("author_ids").cloned()
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

#[test]
fn vcs_markdown_renders_a_ranked_table_to_stdout() {
    let repo = repo_two_commits();
    cli()
        .current_dir(repo.path())
        .args(["vcs", "--paths", ".", "--format", "markdown"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("# Change-history risk")
                .and(predicate::str::contains("| Rank |"))
                .and(predicate::str::contains("src/work.rs")),
        );
}

#[test]
fn vcs_html_writes_a_single_self_contained_file() {
    let repo = repo_two_commits();
    let out = repo.path().join("vcs.html");
    cli()
        .current_dir(repo.path())
        .args([
            "vcs",
            "--paths",
            ".",
            "--format",
            "html",
            "--output",
            out.to_str().expect("utf8 path"),
        ])
        .assert()
        .success();
    // The whole-repo report is a single document, so `--output` must name
    // one file — not the per-file *directory* `metrics`/`ops` write
    // (issue #573). Before the fix this path was a directory holding
    // `vcs.html/vcs.html`.
    assert!(out.is_file(), "--output must write a single file");
    let html = std::fs::read_to_string(&out).expect("read html");
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains("<table class=\"hotspot\">"));
    assert!(html.contains("src/work.rs"));
}

#[test]
fn vcs_json_output_writes_a_single_file() {
    // Regression guard for the `--output` semantics fix: structured
    // formats also write one file now, not a `<dir>/vcs.json` tree.
    let repo = repo_two_commits();
    let out = repo.path().join("vcs.json");
    cli()
        .current_dir(repo.path())
        .args([
            "vcs",
            "--paths",
            ".",
            "--format",
            "json",
            "--output",
            out.to_str().expect("utf8 path"),
        ])
        .assert()
        .success();
    assert!(out.is_file(), "--output must write a single file");
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).expect("read json")).expect("json");
    // Non-empty + names the tracked file, so a regression that writes an
    // empty document to the single file still fails here.
    let files = doc["files"].as_array().expect("files array");
    assert!(
        files.iter().any(|f| f["path"] == "src/work.rs"),
        "single-file report ranks the tracked file"
    );
}

#[test]
fn vcs_html_is_deterministic_across_runs() {
    let repo = repo_two_commits();
    let render = || {
        let assert = cli()
            .current_dir(repo.path())
            .args(["vcs", "--paths", ".", "--format", "html"])
            .assert()
            .success();
        assert.get_output().stdout.clone()
    };
    assert_eq!(render(), render(), "HTML output must be byte-stable");
}

#[test]
fn report_markdown_vcs_appends_change_history_section() {
    let repo = repo_two_commits();
    cli()
        .current_dir(repo.path())
        .args(["report", "markdown", "--vcs", "--paths", "."])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("## Change-history risk")
                .and(predicate::str::contains("| Rank |"))
                .and(predicate::str::contains("src/work.rs")),
        );
}

#[test]
fn report_markdown_vcs_fills_the_hotspot_column() {
    // Issue #615: `report --vcs` computes AST metrics in the same run, so
    // the change-history section must join a real `hotspot_score`
    // (complexity × recent churn) onto each file — not leave the trailing
    // column blank. The fixture file has a non-zero recent churn and a
    // cyclomatic sum > 0, so a join produces a positive value.
    let repo = repo_two_commits();
    let assert = cli()
        .current_dir(repo.path())
        .args(["report", "markdown", "--vcs", "--paths", "."])
        .assert()
        .success();
    let md = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    // The column header and legend entry are present (not omitted).
    assert!(md.contains("| Hotspot |"), "Hotspot header should render");
    // Locate the data row for the tracked file and read its trailing
    // Hotspot cell — the last `|`-delimited field. It must parse to a
    // positive number, proving the join happened.
    let row = md
        .lines()
        .find(|l| l.starts_with('|') && l.contains("src/work.rs"))
        .expect("a data row for src/work.rs");
    let hotspot_cell = row
        .trim_end_matches('|')
        .rsplit('|')
        .next()
        .expect("a trailing Hotspot cell")
        .trim();
    let value: f64 = hotspot_cell
        .parse()
        .unwrap_or_else(|_| panic!("Hotspot cell {hotspot_cell:?} should be a number, not blank"));
    assert!(
        value > 0.0,
        "joined hotspot score should be positive, got {value}"
    );
}

#[test]
fn report_html_vcs_appends_sortable_change_history_section() {
    let repo = repo_two_commits();
    cli()
        .current_dir(repo.path())
        .args(["report", "html", "--vcs", "--paths", "."])
        .assert()
        .success()
        .stdout(
            predicate::str::contains(">Change-history risk</h2>")
                .and(predicate::str::contains("<table class=\"hotspot\">"))
                .and(predicate::str::contains("src/work.rs")),
        );
}

#[test]
fn report_without_vcs_flag_has_no_change_history_section() {
    let repo = repo_two_commits();
    cli()
        .current_dir(repo.path())
        .args(["report", "markdown", "--paths", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("Change-history risk").not());
}

#[test]
fn report_vcs_outside_a_repo_warns_and_still_renders() {
    // `report --vcs` is additive like `metrics --vcs`: outside a git tree
    // it must warn and still emit the AST report, just without the
    // change-history section (issue #573).
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("work.rs"), "fn a() {}\n").expect("write");
    cli()
        .current_dir(dir.path())
        .args(["report", "markdown", "--vcs", "--paths", "work.rs"])
        .assert()
        .success()
        .stderr(predicate::str::contains("warning"))
        // The AST report must still render (the "still renders" claim) —
        // only the change-history section is omitted.
        .stdout(
            predicate::str::contains("Code Quality Metrics Summary")
                .and(predicate::str::contains("Change-history risk").not()),
        );
}

#[test]
fn bad_long_window_names_the_flag_and_echoes_the_input() {
    // Regression for #607: the failure used to name neither the flag nor
    // the offending input (it quoted the split-off empty magnitude as
    // `""`). The CLI must now identify `--long-window` and echo `bogus`,
    // plus the parser's accepted-format hint.
    let repo = repo_two_commits();
    cli()
        .current_dir(repo.path())
        .args(["vcs", "--paths", ".", "--long-window", "bogus"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("--long-window")
                .and(predicate::str::contains("\"bogus\""))
                .and(predicate::str::contains("expected")),
        );
}

#[test]
fn bad_recent_window_names_its_own_flag() {
    // The sibling flag must surface its own name, so a CI invocation
    // passing both windows points at the actual offender (#607).
    let repo = repo_two_commits();
    cli()
        .current_dir(repo.path())
        .args(["vcs", "--paths", ".", "--recent-window", "12parsec"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("--recent-window")
                .and(predicate::str::contains("\"12parsec\"")),
        );
}
