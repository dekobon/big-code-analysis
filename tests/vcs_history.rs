//! End-to-end change-history (VCS) tests against real, deterministic
//! git repositories built through the `git` CLI (issue #328).
//!
//! Every commit carries a fixed identity and UNIX timestamp, and every
//! walk pins `as_of` to [`vcs_fixture::FIXED_NOW`], so per-signal counts
//! are exact and reproducible. The whole file is gated behind the
//! `vcs-git` backend feature.
#![cfg(feature = "vcs-git")]
// Exact-equality on f64 is intentional: the compared values are
// exactly-representable literals (1.0) from exact integer ratios.
#![allow(clippy::float_cmp)]

use std::path::Path;

use big_code_analysis::vcs::{self, Options, build_history_index};

mod common;
use common::vcs_fixture::{DAY, FIXED_NOW, Repo};

/// Options pinned to the fixture clock; tweak fields per test.
fn opts() -> Options {
    Options {
        as_of: Some(FIXED_NOW),
        ..Options::default()
    }
}

fn stats_for<'a>(index: &'a vcs::HistoryIndex, rel: &str) -> &'a vcs::Stats {
    index
        .get(Path::new(rel))
        .unwrap_or_else(|| panic!("expected stats for {rel}; index has {} files", index.len()))
}

#[test]
fn single_commit_one_file() {
    let repo = Repo::init();
    repo.write("src/work.rs", "fn a() {}\nfn b() {}\nfn c() {}\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 10 * DAY, "initial");

    let index = build_history_index(repo.path(), &opts()).expect("walk");
    let stats = stats_for(&index, "src/work.rs");

    assert_eq!(stats.commits_long, 1);
    assert_eq!(stats.commits_recent, 1);
    assert_eq!(stats.authors_long, 1);
    assert_eq!(stats.authors_recent, 1);
    assert_eq!(stats.churn_long, 3, "three lines added");
    assert_eq!(stats.churn_recent, 3);
    assert_eq!(stats.ownership_top_share, 1.0);
    assert_eq!(stats.age_days, 10);
    assert_eq!(stats.last_modified_days, 10);
    assert_eq!(stats.bug_fix_commits, 0);
}

#[test]
fn two_authors_counted_distinctly() {
    let repo = Repo::init();
    repo.write("f.rs", "one\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 20 * DAY, "a");
    repo.write("f.rs", "one\ntwo\n");
    repo.commit("Grace", "grace@example.com", FIXED_NOW - 10 * DAY, "b");

    let index = build_history_index(repo.path(), &opts()).expect("walk");
    let stats = stats_for(&index, "f.rs");
    assert_eq!(stats.commits_long, 2);
    assert_eq!(stats.authors_long, 2);
    assert!((stats.ownership_top_share - 0.5).abs() < 1e-9);
}

#[test]
fn bots_excluded_by_default_then_included() {
    let build = |exclude_bots: bool| {
        let repo = Repo::init();
        repo.write("f.rs", "x\n");
        repo.commit("Ada", "ada@example.com", FIXED_NOW - 20 * DAY, "human");
        repo.write("f.rs", "x\ny\n");
        repo.commit(
            "dependabot[bot]",
            "49699333+dependabot[bot]@users.noreply.github.com",
            FIXED_NOW - 10 * DAY,
            "bump dep",
        );
        let options = Options {
            exclude_bots,
            ..opts()
        };
        let index = build_history_index(repo.path(), &options).expect("walk");
        let stats = stats_for(&index, "f.rs");
        (stats.commits_long, stats.authors_long)
    };

    // Default: the bot-only commit is dropped entirely.
    assert_eq!(build(true), (1, 1));
    // Opt-in: both commits and both authors count.
    assert_eq!(build(false), (2, 2));
}

#[test]
fn renames_followed_by_default() {
    let build = |follow_renames: bool| {
        let repo = Repo::init();
        repo.write("a.rs", "line\n");
        repo.commit("Ada", "ada@example.com", FIXED_NOW - 20 * DAY, "create a");
        repo.git(&["mv", "a.rs", "b.rs"]);
        repo.commit(
            "Ada",
            "ada@example.com",
            FIXED_NOW - 10 * DAY,
            "rename to b",
        );
        let options = Options {
            follow_renames,
            ..opts()
        };
        let index = build_history_index(repo.path(), &options).expect("walk");
        stats_for(&index, "b.rs").commits_long
    };

    // Following renames sees both the create (under a.rs) and the move.
    assert_eq!(build(true), 2);
    // Not following stops at the move commit.
    assert_eq!(build(false), 1);
}

#[test]
fn recent_window_excludes_old_commits() {
    let repo = Repo::init();
    repo.write("f.rs", "1\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 200 * DAY, "old");
    repo.write("f.rs", "1\n2\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 5 * DAY, "fresh");

    let index = build_history_index(repo.path(), &opts()).expect("walk");
    let stats = stats_for(&index, "f.rs");
    assert_eq!(stats.commits_long, 2);
    assert_eq!(
        stats.commits_recent, 1,
        "only the 5-day-old commit is recent"
    );
    assert_eq!(stats.last_modified_days, 5);
    assert_eq!(stats.age_days, 200);
}

#[test]
fn long_window_excludes_commits_beyond_the_boundary() {
    let repo = Repo::init();
    repo.write("f.rs", "1\n");
    // Well outside the 365-day long window.
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 800 * DAY, "ancient");
    repo.write("f.rs", "1\n2\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 30 * DAY, "recent");

    let index = build_history_index(repo.path(), &opts()).expect("walk");
    let stats = stats_for(&index, "f.rs");
    assert_eq!(
        stats.commits_long, 1,
        "the 800-day-old commit is outside the window"
    );
}

#[test]
fn bug_and_security_fixes_classified() {
    let repo = Repo::init();
    repo.write("f.rs", "1\n");
    repo.commit(
        "Ada",
        "ada@example.com",
        FIXED_NOW - 30 * DAY,
        "fix crash on resize",
    );
    repo.write("f.rs", "1\n2\n");
    repo.commit(
        "Ada",
        "ada@example.com",
        FIXED_NOW - 20 * DAY,
        "patch CVE-2021-44228 in parser",
    );
    repo.write("f.rs", "1\n2\n3\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 10 * DAY, "add docs");

    let index = build_history_index(repo.path(), &opts()).expect("walk");
    let stats = stats_for(&index, "f.rs");
    assert_eq!(stats.bug_fix_commits, 1);
    assert_eq!(stats.security_fix_commits, 1);
}

#[test]
fn mailmap_canonicalizes_one_author() {
    let repo = Repo::init();
    repo.mailmap("Ada Lovelace <ada@example.com> <ada@old.example.com>\n");
    repo.write("f.rs", "1\n");
    repo.commit("Ada", "ada@old.example.com", FIXED_NOW - 20 * DAY, "a");
    repo.write("f.rs", "1\n2\n");
    repo.commit("Ada Lovelace", "ada@example.com", FIXED_NOW - 10 * DAY, "b");

    let index = build_history_index(repo.path(), &opts()).expect("walk");
    let stats = stats_for(&index, "f.rs");
    assert_eq!(stats.commits_long, 2);
    assert_eq!(
        stats.authors_long, 1,
        "two emails canonicalize to one identity"
    );
}

#[test]
fn coauthored_by_trailer_counts_extra_author() {
    let repo = Repo::init();
    repo.write("f.rs", "1\n");
    repo.commit(
        "Ada",
        "ada@example.com",
        FIXED_NOW - 10 * DAY,
        "pair work\n\nCo-authored-by: Grace Hopper <grace@example.com>\n",
    );

    let index = build_history_index(repo.path(), &opts()).expect("walk");
    let stats = stats_for(&index, "f.rs");
    assert_eq!(stats.commits_long, 1);
    assert_eq!(stats.authors_long, 2, "author + co-author");
}

#[test]
fn untracked_file_has_no_stats() {
    let repo = Repo::init();
    repo.write("tracked.rs", "1\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 10 * DAY, "a");
    // An untracked working-tree file is absent from the index entirely,
    // distinct from a tracked file with zero counts.
    std::fs::write(repo.path().join("untracked.rs"), "2\n").expect("write");

    let index = build_history_index(repo.path(), &opts()).expect("walk");
    assert!(index.get(Path::new("tracked.rs")).is_some());
    assert!(index.get(Path::new("untracked.rs")).is_none());
}

#[test]
fn revert_commit_classified_end_to_end() {
    let repo = Repo::init();
    repo.write("f.rs", "1\n");
    repo.commit(
        "Ada",
        "ada@example.com",
        FIXED_NOW - 20 * DAY,
        "add feature",
    );
    repo.write("f.rs", "1\n2\n");
    // git's auto-generated revert subject.
    repo.commit(
        "Ada",
        "ada@example.com",
        FIXED_NOW - 10 * DAY,
        "Revert \"add feature\"",
    );
    let index = build_history_index(repo.path(), &opts()).expect("walk");
    let stats = stats_for(&index, "f.rs");
    assert_eq!(
        stats.revert_commits, 1,
        "revert classified through the real walk"
    );
    assert_eq!(stats.bug_fix_commits, 0);
}

#[test]
fn risk_score_ranks_busy_above_quiet() {
    let repo = Repo::init();
    // A quiet file: one tiny commit, long ago.
    repo.write("quiet.rs", "fn q() {}\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 300 * DAY, "add quiet");
    // A busy file: large recent churn across several commits and authors.
    for step in 0..6_i64 {
        let who = if step % 2 == 0 {
            ("Ada", "ada@example.com")
        } else {
            ("Grace", "grace@example.com")
        };
        let line_count = usize::try_from(step * 10 + 1).expect("fits usize");
        repo.write("busy.rs", &"fn f() {}\n".repeat(line_count));
        repo.commit(who.0, who.1, FIXED_NOW - (30 - step) * DAY, "edit busy");
    }
    let index = build_history_index(repo.path(), &opts()).expect("walk");
    // End-to-end: the score wiring (finalize → ScoreInput → weighted)
    // must rank the high-churn, multi-author, recently-edited file above
    // the quiet one. A miscabled score field would flip or flatten this.
    assert!(
        stats_for(&index, "busy.rs").risk_score > stats_for(&index, "quiet.rs").risk_score,
        "busy file outranks quiet file"
    );
}

#[test]
fn empty_repo_errors_on_unborn_head() {
    // A freshly-initialised repo (no commits) has an unborn HEAD; the
    // ref cannot resolve, so the walk reports ResolveRef rather than
    // panicking or hanging.
    let repo = Repo::init();
    let err = build_history_index(repo.path(), &opts()).expect_err("unborn HEAD must error");
    assert!(matches!(err, vcs::Error::ResolveRef { .. }), "got {err:?}");
}

#[test]
fn parse_timestamp_accepts_formats_and_rejects_garbage() {
    assert!(vcs::parse_timestamp("2023-11-14T00:00:00Z").is_ok());
    assert_eq!(
        vcs::parse_timestamp("@1700000000").expect("epoch form"),
        1_700_000_000
    );
    assert!(matches!(
        vcs::parse_timestamp("not-a-date"),
        Err(vcs::Error::InvalidTimestamp(_))
    ));
}

#[test]
fn percentile_formula_reranks_end_to_end() {
    let repo = Repo::init();
    repo.write("quiet.rs", "fn q() {}\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 300 * DAY, "quiet");
    for step in 0..5_i64 {
        let lines = usize::try_from(step * 8 + 1).expect("fits usize");
        repo.write("busy.rs", &"fn f() {}\n".repeat(lines));
        repo.commit(
            "Ada",
            "ada@example.com",
            FIXED_NOW - (20 - step) * DAY,
            "busy",
        );
    }
    let options = Options {
        risk_formula: vcs::RiskFormula::Percentile,
        ..opts()
    };
    let index = build_history_index(repo.path(), &options).expect("walk");
    let busy = stats_for(&index, "busy.rs").risk_score;
    let quiet = stats_for(&index, "quiet.rs").risk_score;
    assert!(
        busy > quiet,
        "percentile ranks busy above quiet: {busy} vs {quiet}"
    );
    assert!((0.0..=100.0).contains(&busy), "percentile score in [0,100]");
}

#[test]
fn include_deleted_surfaces_a_removed_file() {
    let repo = Repo::init();
    repo.write("gone.rs", "fn g() {}\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 30 * DAY, "add gone");
    repo.git(&["rm", "-q", "gone.rs"]);
    repo.write("kept.rs", "fn k() {}\n");
    repo.commit(
        "Ada",
        "ada@example.com",
        FIXED_NOW - 10 * DAY,
        "remove gone",
    );

    // Default: a file deleted at the target ref is absent from the index.
    let default = build_history_index(repo.path(), &opts()).expect("walk");
    assert!(default.get(Path::new("gone.rs")).is_none());
    // Opt-in: the deleted file is surfaced with its in-window history.
    let with_deleted = build_history_index(
        repo.path(),
        &Options {
            include_deleted: true,
            ..opts()
        },
    )
    .expect("walk");
    let gone = with_deleted
        .get(Path::new("gone.rs"))
        .expect("deleted file surfaced under include_deleted");
    // Both the "add gone" commit and the "remove gone" deletion commit
    // touch the file (a deletion is a churn-bearing change), so exactly
    // two commits attribute — a regression dropping the deletion-commit
    // attribution would fail this where `>= 1` would not.
    assert_eq!(gone.commits_long, 2);
}

#[test]
fn first_parent_merges_and_full_history_count_branch_work() {
    let repo = Repo::init();
    repo.write("base.rs", "fn b() {}\n");
    repo.commit(
        "Ada",
        "ada@example.com",
        FIXED_NOW - 40 * DAY,
        "base on main",
    );
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("f.rs", "fn f() {}\n");
    repo.commit(
        "Grace",
        "grace@example.com",
        FIXED_NOW - 30 * DAY,
        "feature work",
    );
    repo.git(&["checkout", "-q", "main"]);
    repo.git_at(
        "Ada",
        "ada@example.com",
        FIXED_NOW - 10 * DAY,
        &["merge", "--no-ff", "-m", "merge feature", "feature"],
    );

    let commits_long = |options: &Options| {
        build_history_index(repo.path(), options)
            .expect("walk")
            .get(Path::new("f.rs"))
            .expect("f.rs is present at the merged HEAD")
            .commits_long
    };

    // Default (first-parent, skip merges): the branch-side commit is on
    // the second parent and the merge is skipped, so f.rs is unattributed.
    assert_eq!(commits_long(&opts()), 0, "first-parent skips branch work");
    // Including merges: the merge commit's diff against its first parent
    // introduces f.rs, so it is counted once.
    assert_eq!(
        commits_long(&Options {
            include_merges: true,
            ..opts()
        }),
        1,
        "the merge commit introduces f.rs"
    );
    // Full history walks the second parent, counting the branch commit.
    // The merge is still skipped (full_history does not imply
    // include_merges), so it is exactly one.
    assert_eq!(
        commits_long(&Options {
            full_history: true,
            ..opts()
        }),
        1,
        "full history reaches the branch commit"
    );
    // Both flags together count the file twice — once on the branch
    // commit (C2) and once on the merge's diff against its first parent
    // (M). This double-count is the documented consequence of walking
    // the full DAG *and* attributing merges; pinned so it cannot change
    // silently.
    assert_eq!(
        commits_long(&Options {
            full_history: true,
            include_merges: true,
            ..opts()
        }),
        2,
        "full DAG + merges attributes both the branch and merge commits"
    );
}

#[test]
fn shallow_clone_degrades_gracefully() {
    use std::process::Command;
    // Source repo with two commits so the shallow tip has a parent that
    // the depth-1 clone will not fetch (a grafted, absent boundary).
    let src = Repo::init();
    src.write("f.rs", "fn a() {}\n");
    src.commit("Ada", "ada@example.com", FIXED_NOW - 30 * DAY, "first");
    src.write("f.rs", "fn a() {}\nfn b() {}\n");
    src.commit("Ada", "ada@example.com", FIXED_NOW - 10 * DAY, "second");

    // `file://` forces the non-local transport so `--depth` actually
    // produces a shallow clone (a bare path clone ignores `--depth`).
    let dest = tempfile::tempdir().expect("tempdir");
    let url = format!("file://{}", src.path().display());
    let clone_ok = Command::new("git")
        .args(["clone", "--quiet", "--depth", "1", &url])
        .arg(dest.path())
        .status()
        .expect("spawn git clone")
        .success();
    assert!(clone_ok, "shallow clone failed");

    // Must NOT abort on the missing grafted parent: it is treated as a
    // boundary (diff against the empty tree) and the result is flagged
    // truncated rather than erroring out — the graceful path that was
    // previously dead because the walk aborted before finalizing.
    let index = build_history_index(dest.path(), &opts()).expect("shallow walk completes");
    assert!(index.truncated_shallow_clone(), "shallow clone is flagged");
    let stats = stats_for(&index, "f.rs");
    assert_eq!(stats.commits_long, 1, "only the shallow tip is present");
    assert!(
        stats.churn_long >= 1,
        "the tip's content counts as additions"
    );
}

#[test]
fn not_a_repository_errors_clearly() {
    let dir = tempfile::tempdir().expect("tempdir");
    let err = build_history_index(dir.path(), &opts()).expect_err("must error outside a repo");
    assert!(
        matches!(err, vcs::Error::NotARepository(_)),
        "expected NotARepository, got {err:?}"
    );
}
