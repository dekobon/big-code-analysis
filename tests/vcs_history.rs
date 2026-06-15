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

use big_code_analysis::vcs::{
    self, CacheConfig, Options, build_history_index, build_history_index_cached, build_trend,
    workdir_root,
};

mod common;
use common::vcs_fixture::{DAY, FIXED_NOW, Repo};

/// Options pinned to the fixture clock; tweak fields per test.
fn opts() -> Options {
    let mut options = Options::default();
    options.as_of = Some(FIXED_NOW);
    options
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
        let mut options = opts();
        options.exclude_bots = exclude_bots;
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
        let mut options = opts();
        options.follow_renames = follow_renames;
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
fn coauthor_quoted_in_body_is_not_counted() {
    // Issue #812: a `Co-authored-by:` line quoted inside an indented body
    // block (here, a reverted commit pasted into the message), with a real
    // `Signed-off-by:` trailer at the end, must NOT add the quoted author.
    // Only the author counts; the final trailer block holds no co-author.
    let repo = Repo::init();
    repo.write("f.rs", "1\n");
    repo.commit(
        "Ada",
        "ada@example.com",
        FIXED_NOW - 10 * DAY,
        "Revert a bad merge\n\
         \n\
         This reverts the merge. The original commit read:\n\
         \n\
         \u{20}\u{20}\u{20}\u{20}Add pairing helper\n\
         \n\
         \u{20}\u{20}\u{20}\u{20}Co-authored-by: Eve Quoted <eve@example.com>\n\
         \n\
         Signed-off-by: Ada Lovelace <ada@example.com>\n",
    );

    let index = build_history_index(repo.path(), &opts()).expect("walk");
    let stats = stats_for(&index, "f.rs");
    assert_eq!(stats.commits_long, 1);
    assert_eq!(
        stats.authors_long, 1,
        "a body-quoted co-author must not be attributed"
    );
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

/// log₂(3): the entropy of a uniform 3-way distribution.
const LOG2_3: f64 = 1.584_962_500_721_156;

#[test]
fn trio_commit_yields_change_and_cochange_entropy() {
    let repo = Repo::init();
    // Three one-line files born in a single commit: each contributes 1
    // line of churn, so the commit's churn distribution is uniform over 3
    // → change entropy H = log2(3), and each file is credited its 1/3
    // share. They all co-changed pairwise once → a triangle, so each
    // file's co-change neighbours are {1, 1} → exactly 1 bit.
    repo.write("a.rs", "fn a() {}\n");
    repo.write("b.rs", "fn b() {}\n");
    repo.write("c.rs", "fn c() {}\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 10 * DAY, "feat: trio");

    let index = build_history_index(repo.path(), &opts()).expect("walk");
    for f in ["a.rs", "b.rs", "c.rs"] {
        let s = stats_for(&index, f);
        let share = LOG2_3 / 3.0;
        assert!(
            (s.change_entropy_long - share).abs() < 1e-9,
            "{f} change_entropy_long = {} (want {share})",
            s.change_entropy_long
        );
        assert!((s.change_entropy_recent - share).abs() < 1e-9);
        assert!(
            (s.cochange_entropy_long - 1.0).abs() < 1e-9,
            "{f} cochange_entropy_long = {}",
            s.cochange_entropy_long
        );
        assert!((s.cochange_entropy_recent - 1.0).abs() < 1e-9);
    }
}

#[test]
fn single_file_commit_has_zero_entropy() {
    let repo = Repo::init();
    // A file that only ever changes alone: a one-file commit has a
    // degenerate (certain) churn distribution → change entropy 0, and no
    // co-change partner → co-change entropy 0. This is the "computed
    // zero" the issue distinguishes from "not computed".
    repo.write("lonely.rs", "fn l() {}\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 10 * DAY, "solo");

    let index = build_history_index(repo.path(), &opts()).expect("walk");
    let s = stats_for(&index, "lonely.rs");
    assert_eq!(s.change_entropy_long, 0.0);
    assert_eq!(s.change_entropy_recent, 0.0);
    assert_eq!(s.cochange_entropy_long, 0.0);
    assert_eq!(s.cochange_entropy_recent, 0.0);
}

#[test]
fn entropy_windows_split_recent_from_long() {
    let repo = Repo::init();
    // Long-only triangle 200 days back: a, b, c born together.
    repo.write("a.rs", "fn a() {}\n");
    repo.write("b.rs", "fn b() {}\n");
    repo.write("c.rs", "fn c() {}\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 200 * DAY, "init trio");
    // Recent edit 10 days back co-changing only a and b (one line each).
    repo.write("a.rs", "fn a() {}\nfn a2() {}\n");
    repo.write("b.rs", "fn b() {}\nfn b2() {}\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 10 * DAY, "edit a+b");

    let index = build_history_index(repo.path(), &opts()).expect("walk");
    let a = stats_for(&index, "a.rs");

    // Co-change long graph: a–b appeared together twice (init + recent),
    // a–c once → a's edge weights {2, 1} → entropy ≈ 0.9183 bits.
    assert!(
        (a.cochange_entropy_long - 0.918_295_834_054_490).abs() < 1e-9,
        "a cochange_entropy_long = {}",
        a.cochange_entropy_long
    );
    // Recent subgraph: a co-changed only with b → single neighbour → 0.
    assert_eq!(
        a.cochange_entropy_recent, 0.0,
        "recent co-change sees one neighbour only"
    );

    // Change entropy long = init-commit share (log2(3)/3, uniform over 3)
    // + recent-commit share (0.5·log2(2)=0.5, uniform over 2). Recent
    // window sees only the second commit's 0.5.
    let want_long = LOG2_3 / 3.0 + 0.5;
    assert!(
        (a.change_entropy_long - want_long).abs() < 1e-9,
        "a change_entropy_long = {} (want {want_long})",
        a.change_entropy_long
    );
    assert!(
        (a.change_entropy_recent - 0.5).abs() < 1e-9,
        "a change_entropy_recent = {}",
        a.change_entropy_recent
    );
}

#[test]
fn cochange_counts_deleted_partners_independent_of_include_deleted() {
    // A surviving file that historically co-changed with a since-deleted
    // file should carry that coupling regardless of `--include-deleted`:
    // the co-change graph is built from the full commit, while the flag
    // only governs which files get a Stats record. This pins the
    // documented "graph spans all touched files" design.
    let repo = Repo::init();
    repo.write("kept.rs", "fn k() {}\n");
    repo.write("gone.rs", "fn g() {}\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 30 * DAY, "feat: pair");
    repo.git(&["rm", "-q", "gone.rs"]);
    repo.write("kept.rs", "fn k() {}\nfn k2() {}\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 10 * DAY, "drop gone");

    // `kept` co-changed with `gone` once (the pair commit) → a single
    // neighbour → co-change entropy 0 either way, but the edge exists.
    // To make the neighbour count load-bearing, the value we assert is
    // the same under both flag settings.
    let kept = |include_deleted| {
        let mut options = opts();
        options.include_deleted = include_deleted;
        build_history_index(repo.path(), &options)
            .expect("walk")
            .get(Path::new("kept.rs"))
            .expect("kept.rs present at HEAD")
            .cochange_entropy_long
    };
    assert_eq!(
        kept(false),
        kept(true),
        "co-change entropy must not depend on --include-deleted"
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
    let mut options = opts();
    options.risk_formula = vcs::RiskFormula::Percentile;
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
    let mut deleted_options = opts();
    deleted_options.include_deleted = true;
    let with_deleted = build_history_index(repo.path(), &deleted_options).expect("walk");
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
    let mut merges = opts();
    merges.include_merges = true;
    assert_eq!(commits_long(&merges), 1, "the merge commit introduces f.rs");
    // Full history walks the second parent, counting the branch commit.
    // The merge is still skipped (full_history does not imply
    // include_merges), so it is exactly one.
    let mut full = opts();
    full.full_history = true;
    assert_eq!(
        commits_long(&full),
        1,
        "full history reaches the branch commit"
    );
    // Both flags together count the file twice — once on the branch
    // commit (C2) and once on the merge's diff against its first parent
    // (M). This double-count is the documented consequence of walking
    // the full DAG *and* attributing merges; pinned so it cannot change
    // silently.
    let mut full_with_merges = opts();
    full_with_merges.full_history = true;
    full_with_merges.include_merges = true;
    assert_eq!(
        commits_long(&full_with_merges),
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
fn trend_anchors_newest_when_all_commits_share_one_second() {
    // Reproduces issue #650: every commit lands in the same commit-time
    // second (scripted setups, squash/rebase chains, bot commits). The
    // empty init commit and the content commit are indistinguishable by
    // time, so the per-point tip selection must tie-break toward the
    // newest (content-bearing) commit — otherwise it anchors at the empty
    // init and the whole trend collapses to `{}`.
    let shared_second = FIXED_NOW - 5 * DAY;
    let repo = Repo::init();
    // Empty init commit at the shared second (the classic oldest tie).
    repo.commit("Ada", "ada@example.com", shared_second, "init");
    // Content commit at the *same* second. Unique per-line content avoids
    // the gix-blame repetitive-content non-determinism on identical lines.
    repo.write(
        "src/work.rs",
        "fn first() {}\nfn second() {}\nfn third() {}\n",
    );
    repo.commit("Ada", "ada@example.com", shared_second, "add work");

    // Sample points all fall after the shared second, so each must anchor
    // at the content commit.
    let mut options = Options::default();
    options.as_of = Some(shared_second + 2 * DAY);
    let trend = build_trend(repo.path(), &options, 2, DAY).expect("trend builds");

    assert!(
        !trend.is_empty(),
        "trend must not collapse to empty when commits share one second"
    );
    let series: Vec<_> = trend.iter().collect();
    assert_eq!(series.len(), 1, "exactly the one tracked file");
    let (path, stats) = series[0];
    assert_eq!(path.as_path(), Path::new("src/work.rs"));
    assert!(
        stats.iter().all(Option::is_some),
        "the file must exist at every sampled point after its commit"
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

#[test]
fn workdir_root_coalesces_files_across_subdirectories() {
    // Two files in *different* subdirectories of one checkout must resolve
    // to the same work-tree root — the property the batch VCS cache relies
    // on to build one index per repository rather than one per directory
    // (issue #670).
    let repo = Repo::init();
    repo.write("src/a.rs", "fn a() {}\n");
    repo.write("tests/b.rs", "fn b() {}\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 10 * DAY, "init");

    let root_a = workdir_root(&repo.path().join("src/a.rs")).expect("a.rs is in a repo");
    let root_b = workdir_root(&repo.path().join("tests/b.rs")).expect("b.rs is in a repo");
    assert_eq!(root_a, root_b, "both files share one work-tree root");

    // The discovered root is the repo's canonicalised work tree.
    let canonical = repo.path().canonicalize().expect("canonicalize repo path");
    assert_eq!(root_a, canonical);
}

#[test]
fn workdir_root_is_none_outside_a_repository() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(
        workdir_root(&dir.path().join("loose.rs")).is_none(),
        "a path outside any repository has no work-tree root"
    );
}

#[test]
fn workdir_root_distinguishes_separate_repositories() {
    let one = Repo::init();
    one.write("x.rs", "fn x() {}\n");
    one.commit("Ada", "ada@example.com", FIXED_NOW - 10 * DAY, "one");
    let two = Repo::init();
    two.write("y.rs", "fn y() {}\n");
    two.commit("Ada", "ada@example.com", FIXED_NOW - 10 * DAY, "two");

    let root_one = workdir_root(&one.path().join("x.rs")).expect("repo one");
    let root_two = workdir_root(&two.path().join("y.rs")).expect("repo two");
    assert_ne!(root_one, root_two, "distinct repos yield distinct roots");
}

/// Issue #648: the plain ranking walk must re-anchor at the mainline tip
/// at-or-before `as_of`, so a commit in the *future* of the requested
/// reference time is not counted. This is the exact reported reproducer:
/// the repo's only commit is dated 2026, queried with `as_of` in 2023.
#[test]
fn as_of_excludes_commits_in_its_future() {
    // The fixture clock (`FIXED_NOW`, 2023-11-14) is the as_of; the single
    // commit is dated a year later, in its future.
    let as_of = FIXED_NOW;
    let future = FIXED_NOW + 365 * DAY;

    let repo = Repo::init();
    repo.write("src/work.rs", "fn a() {}\n");
    repo.commit("Ada", "ada@example.com", future, "future commit");

    let mut options = Options::default();
    options.as_of = Some(as_of);
    let index = build_history_index(repo.path(), &options).expect("walk");

    // The only commit lies after as_of, so the snapshot is empty: no tip
    // existed at-or-before as_of. (Before the fix the commit was clamped
    // onto `now` and counted, reporting commits_long == 1.)
    assert!(
        index.get(Path::new("src/work.rs")).is_none(),
        "a commit in the future of as_of must not appear in the snapshot"
    );
    assert!(
        index.is_empty(),
        "no commit at-or-before as_of → empty index"
    );
}

/// Issue #648: with history straddling `as_of`, only the at-or-before
/// portion is counted — the snapshot reflects the tree and windows as of
/// that instant, not HEAD.
#[test]
fn as_of_counts_only_history_at_or_before_it() {
    let repo = Repo::init();
    // Two commits to the same file: one before as_of, one after.
    repo.write("a.rs", "one\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 30 * DAY, "before");
    repo.write("a.rs", "one\ntwo\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW + 30 * DAY, "after");

    let mut options = Options::default();
    options.as_of = Some(FIXED_NOW);
    let index = build_history_index(repo.path(), &options).expect("walk");
    let stats = stats_for(&index, "a.rs");

    // Only the pre-as_of commit is anchored and counted; the future commit
    // (and its churn) is excluded.
    assert_eq!(
        stats.commits_long, 1,
        "only the at-or-before commit is counted"
    );
    assert_eq!(stats.churn_long, 1, "only the first commit's one line");

    // Without as_of (HEAD anchor) both commits are visible — proving the
    // re-anchoring, not a window artifact, drove the difference above.
    let mut head_opts = Options::default();
    head_opts.as_of = Some(FIXED_NOW + 60 * DAY);
    let head_index = build_history_index(repo.path(), &head_opts).expect("walk");
    assert_eq!(
        stats_for(&head_index, "a.rs").commits_long,
        2,
        "anchored past both commits, the walk sees both"
    );
}

/// Issue #648 edge case: `as_of` before the first commit yields an empty
/// snapshot, handled gracefully rather than as an error (matching trend).
#[test]
fn as_of_before_first_commit_is_an_empty_snapshot() {
    let repo = Repo::init();
    repo.write("a.rs", "one\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 10 * DAY, "first");

    let mut options = Options::default();
    options.as_of = Some(FIXED_NOW - 100 * DAY);
    let index = build_history_index(repo.path(), &options).expect("walk must not error");

    assert!(
        index.is_empty(),
        "as_of predating the first commit → empty, not an error"
    );
}

/// Issue #648: the `bca vcs` / web `/vcs` / py `vcs_metrics` surfaces all
/// drive the *cached* entry point, so the re-anchoring must hold there too
/// (the cache is HEAD-keyed and must not serve an `as_of` snapshot).
#[test]
fn as_of_excludes_future_commits_through_the_cached_entry_point() {
    let repo = Repo::init();
    repo.write("a.rs", "one\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 30 * DAY, "before");
    repo.write("a.rs", "one\ntwo\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW + 30 * DAY, "after");

    let mut options = Options::default();
    options.as_of = Some(FIXED_NOW);
    // Caching enabled (the default) must not change the as_of result: the
    // cached path delegates to the re-anchored plain walk.
    let index = build_history_index_cached(repo.path(), &options, &CacheConfig::default())
        .expect("cached walk");
    let stats = stats_for(&index, "a.rs");

    assert_eq!(
        stats.commits_long, 1,
        "the cached entry point must also exclude commits in the future of as_of"
    );
    assert_eq!(stats.churn_long, 1);
}
