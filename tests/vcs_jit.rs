//! End-to-end just-in-time (commit-level) risk-scoring tests against
//! real, deterministic git repositories (issue #331).
//!
//! Every commit carries a fixed identity and UNIX timestamp, and every
//! score pins `as_of` to [`vcs_fixture::FIXED_NOW`], so the per-commit
//! features are exact and reproducible. Gated behind the `vcs-git`
//! backend feature.
#![cfg(feature = "vcs-git")]
// Exact-equality on f64 is intentional: the compared values are exact
// literals (0.0) from the formula's zero terms.
#![allow(clippy::float_cmp)]

use big_code_analysis::vcs::{self, JitSource, Options, score_commit, score_diff};

mod common;
use common::vcs_fixture::{DAY, FIXED_NOW, Repo};

/// Options pinned to the fixture clock; tweak fields per test.
fn opts() -> Options {
    let mut options = Options::default();
    options.as_of = Some(FIXED_NOW);
    options
}

fn score(repo: &Repo, spec: &str) -> vcs::JitReport {
    score_commit(repo.path(), spec, &opts()).expect("score commit")
}

#[test]
fn root_commit_has_no_parent_and_all_new_files() {
    let repo = Repo::init();
    repo.write("src/work.rs", "fn a() {}\nfn b() {}\nfn c() {}\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 10 * DAY, "initial");

    let report = score(&repo, "HEAD");

    assert_eq!(report.commit.parent_count, 0, "root commit has no parent");
    assert!(!report.commit.is_merge);
    // Three lines added, one file, no prior history → all new.
    assert_eq!(report.features.size.lines_added, 3);
    assert_eq!(report.features.size.lines_deleted, 0);
    assert_eq!(report.features.size.files_touched, 1);
    assert_eq!(report.features.history.new_files, 1);
    assert_eq!(report.features.history.prior_changes, 0);
    assert_eq!(report.features.experience.author_prior_commits, 0);
    // Size alone drives the score; it is positive and finite.
    assert!(report.risk_score > 0.0 && report.risk_score.is_finite());
    assert!(report.contributions.size > 0.0);
    assert_eq!(report.contributions.experience, 0.0);
}

#[test]
fn append_only_change_counts_added_not_deleted() {
    let repo = Repo::init();
    repo.write("a.rs", "one\ntwo\nthree\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 30 * DAY, "init");
    repo.write("a.rs", "one\ntwo\nthree\nfour\nfive\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 5 * DAY, "extend");

    let report = score(&repo, "HEAD");

    // expected: two lines appended to an existing file, nothing removed,
    // contiguous so a single hunk.
    assert_eq!(report.features.size.lines_added, 2);
    assert_eq!(report.features.size.lines_deleted, 0);
    assert_eq!(report.features.size.files_touched, 1);
    assert_eq!(report.features.size.hunks, 1);
}

#[test]
fn diffusion_counts_distinct_subsystems_and_directories() {
    let repo = Repo::init();
    // A single root commit touching two top-level subsystems.
    repo.write("src/a.rs", "alpha\nbeta\n");
    repo.write("docs/b.md", "# title\nbody\n");
    repo.write("root.txt", "x\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 7 * DAY, "spread");

    let report = score(&repo, "HEAD");

    // subsystems: `src`, `docs`, and the repo root (root.txt) → 3.
    assert_eq!(report.features.size.files_touched, 3);
    assert_eq!(report.features.diffusion.subsystems, 3);
    assert_eq!(report.features.diffusion.directories, 3);
    // Three files with non-zero churn → positive within-commit entropy.
    assert!(report.features.diffusion.entropy > 0.0);
    assert!(report.contributions.diffusion > 0.0);
}

#[test]
fn file_priors_accumulate_from_history_before_the_commit() {
    let repo = Repo::init();
    repo.write("src/work.rs", "a\nb\nc\n");
    repo.commit(
        "Ada",
        "ada@example.com",
        FIXED_NOW - 100 * DAY,
        "fix initial bug",
    );
    repo.write("src/work.rs", "a\nB\nc\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 50 * DAY, "fix crash");
    repo.write("src/work.rs", "a\nB\nc\nd\n");
    repo.commit(
        "Ada",
        "ada@example.com",
        FIXED_NOW - 10 * DAY,
        "add feature",
    );

    let report = score(&repo, "HEAD");

    // The HEAD commit's parent index covers the two earlier commits.
    assert_eq!(report.features.history.new_files, 0, "file is not new");
    assert_eq!(
        report.features.history.prior_changes, 2,
        "two prior in-window commits touched the file"
    );
    assert_eq!(
        report.features.history.prior_bug_fix_commits, 2,
        "both prior commits matched a bug-fix keyword"
    );
    assert!(report.features.history.file_risk_max > 0.0);
    assert!(report.features.history.file_risk_mean > 0.0);
    assert!(report.contributions.history > 0.0);
}

#[test]
fn author_prior_commits_are_counted_and_lower_the_score() {
    // The newcomer-vs-veteran comparison is covered at the formula level in
    // the `experience_lowers_the_score` unit test; here we verify the
    // backend counts the author's prior commits from real history and feeds
    // them into the (negative) experience contribution.
    let repo = Repo::init();
    // The author builds three commits of history, then makes the scored one.
    repo.write("f.rs", "1\n");
    repo.commit("Vet", "vet@example.com", FIXED_NOW - 120 * DAY, "c1");
    repo.write("f.rs", "1\n2\n");
    repo.commit("Vet", "vet@example.com", FIXED_NOW - 80 * DAY, "c2");
    repo.write("f.rs", "1\n2\n3\n");
    repo.commit("Vet", "vet@example.com", FIXED_NOW - 40 * DAY, "c3");
    repo.write("f.rs", "1\n2\n3\n4\n");
    repo.commit("Vet", "vet@example.com", FIXED_NOW - 5 * DAY, "c4");

    let report = score(&repo, "HEAD");

    assert_eq!(
        report.features.experience.author_prior_commits, 3,
        "three commits precede the scored one"
    );
    // c2 (-80d) and c3 (-40d) fall inside the 90d recent window; c1
    // (-120d) does not.
    assert_eq!(report.features.experience.author_recent_commits, 2);
    assert!(
        report.contributions.experience < 0.0,
        "experience must subtract from the score"
    );
}

#[test]
fn experience_anchors_on_commit_time_not_wall_clock() {
    // Regression: the experience windows must anchor on the scored
    // commit's own timestamp, not the wall clock / `--as-of` reference.
    // Here `as_of` is 400 days *after* the commits, so a wall-clock-
    // anchored long window (now - 365d) would start in the future
    // relative to every commit and count zero prior commits. Anchoring on
    // the commit's time keeps the author's three priors in range.
    let repo = Repo::init();
    repo.write("f.rs", "1\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 60 * DAY, "c1");
    repo.write("f.rs", "1\n2\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 40 * DAY, "c2");
    repo.write("f.rs", "1\n2\n3\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 20 * DAY, "c3");
    repo.write("f.rs", "1\n2\n3\n4\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW, "scored");

    let mut options = Options::default();
    options.as_of = Some(FIXED_NOW + 400 * DAY);
    let report = score_commit(repo.path(), "HEAD", &options).expect("score commit");

    assert_eq!(
        report.features.experience.author_prior_commits, 3,
        "all three prior commits fall in the commit-time-anchored long window"
    );
    assert!(report.contributions.experience < 0.0);
}

#[test]
fn fix_and_security_messages_are_classified() {
    let repo = Repo::init();
    repo.write("f.rs", "x\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 30 * DAY, "base");
    repo.write("f.rs", "x\ny\n");
    repo.commit(
        "Ada",
        "ada@example.com",
        FIXED_NOW - 5 * DAY,
        "fix CVE-2021-1234 buffer overflow",
    );

    let report = score(&repo, "HEAD");

    assert!(report.commit.purpose.is_fix, "matches a bug-fix keyword");
    assert!(
        report.commit.purpose.is_security_fix,
        "matches CVE + overflow"
    );
    assert!(!report.commit.purpose.is_revert);
}

#[test]
fn merge_commit_is_classified_by_parent_count() {
    let repo = Repo::init();
    repo.write("main.rs", "base\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 60 * DAY, "base");
    // Branch, commit, then merge back with --no-ff so a real merge commit
    // (two parents) is created.
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("feature.rs", "feature\n");
    repo.commit("Bo", "bo@example.com", FIXED_NOW - 40 * DAY, "feature work");
    repo.git(&["checkout", "-q", "main"]);
    repo.git_at(
        "Ada",
        "ada@example.com",
        FIXED_NOW - 20 * DAY,
        &["merge", "--no-ff", "-m", "merge feature", "feature"],
    );

    let report = score(&repo, "HEAD");

    assert_eq!(report.commit.parent_count, 2, "merge has two parents");
    assert!(report.commit.is_merge);
}

#[test]
fn scoring_a_specific_revision_works() {
    let repo = Repo::init();
    repo.write("f.rs", "1\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 50 * DAY, "first");
    repo.write("f.rs", "1\n2\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 10 * DAY, "second");

    // `HEAD~1` is the first commit — a root with no parent.
    let first = score(&repo, "HEAD~1");
    assert_eq!(first.commit.parent_count, 0);
    assert_eq!(first.features.size.lines_added, 1);

    // `HEAD` is the second, with the first as its prior.
    let second = score(&repo, "HEAD");
    assert_eq!(second.commit.parent_count, 1);
    assert_eq!(second.features.history.new_files, 0);
}

#[test]
fn report_carries_stable_version_stamps() {
    let repo = Repo::init();
    repo.write("f.rs", "x\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 10 * DAY, "init");

    let report = score(&repo, "HEAD");

    assert_eq!(report.jit_schema_version, vcs::JIT_SCHEMA_VERSION);
    assert_eq!(report.jit_score_version, vcs::JIT_SCORE_VERSION);
    assert_eq!(report.long_window_days, 365);
    assert_eq!(report.recent_window_days, 90);
    assert_eq!(report.commit.id.len(), 40, "full hex sha1");
}

// --- Arbitrary-diff scoring (issue #580) ---------------------------------

#[test]
fn commit_mode_serialized_shape_carries_source_discriminator() {
    // A commit report's serialized top-level key set is exactly the
    // commit-mode field set, including the `source` discriminator added in
    // #642 so commit reports self-identify like diff reports already do.
    let repo = Repo::init();
    repo.write("a.rs", "x\ny\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 10 * DAY, "init");

    let report = score(&repo, "HEAD");
    assert_eq!(report.source, JitSource::Commit);
    let json = serde_json::to_value(&report).expect("serialize commit report");
    assert_eq!(json["source"], "commit", "commit-mode source discriminator");
    let keys: std::collections::BTreeSet<&str> = json
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    let expected: std::collections::BTreeSet<&str> = [
        "jit_schema_version",
        "jit_score_version",
        "source",
        "long_window_days",
        "recent_window_days",
        "risk_score",
        "commit",
        "features",
        "contributions",
    ]
    .into_iter()
    .collect();
    assert_eq!(keys, expected, "commit-mode key set");
    // A commit report must NOT carry the diff-only partial score.
    assert!(!keys.contains("partial_risk_score"));
    // Issue #591: exactly one domain-prefixed `<domain>_schema_version` stamp.
    let stamps: Vec<&str> = keys
        .iter()
        .filter(|k| k.ends_with("schema_version"))
        .copied()
        .collect();
    assert_eq!(stamps, ["jit_schema_version"], "one prefixed schema stamp");
}

#[test]
fn diff_mode_marks_unavailable_groups_distinct_from_zero() {
    // A two-subsystem diff: size + diffusion are computable; history /
    // experience / purpose have no input. They must be ABSENT from the
    // report (not present as a real zero), so a consumer cannot read an
    // unavailable group as "low risk".
    let diff = "\
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
    let report = score_diff(diff).expect("score diff");
    assert_eq!(report.source, JitSource::Diff);
    assert_eq!(report.size.files_touched, 2);
    assert_eq!(report.size.lines_added, 3);
    assert_eq!(report.diffusion.subsystems, 2, "src + docs");
    assert!(report.partial_risk_score > 0.0);

    let json = serde_json::to_value(&report).expect("serialize diff report");
    let obj = json.as_object().expect("object");
    assert_eq!(obj["source"], "diff");
    // The whole point: the unavailable groups have no key at all. The
    // pre-#591 `partial_score` name is listed too, so the rename to
    // `partial_risk_score` is pinned (a serializer emitting the old key
    // alongside the new one would fail here, not silently pass).
    for absent in [
        "history",
        "experience",
        "purpose",
        "commit",
        "risk_score",
        "partial_score",
    ] {
        assert!(
            !obj.contains_key(absent),
            "diff report must omit `{absent}` (would be misread as a real value)"
        );
    }
    assert!(obj.contains_key("partial_risk_score"));
}

#[test]
fn diff_mode_partial_score_is_lower_than_an_equivalent_commit() {
    // The not-comparable caveat made concrete: a commit whose diff is
    // identical scores HIGHER than the bare diff, because the commit also
    // folds in history / experience / purpose. So the two scores live on
    // different scales and must not be compared.
    let repo = Repo::init();
    repo.write("src/a.rs", "keep\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 20 * DAY, "fix base");
    repo.write("src/a.rs", "keep\nadded1\nadded2\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 5 * DAY, "fix more");
    let commit = score(&repo, "HEAD");

    // The same churn as a bare diff (one file, two added lines, one hunk).
    let diff = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,1 +1,3 @@
 keep
+added1
+added2
";
    let report = score_diff(diff).expect("score diff");
    // Pin that the diff and commit measured the SAME churn — otherwise an
    // undercounting parser (the Finding-1 `--- `/`+++ ` class of bug) could
    // shrink the diff's size term and pass `commit > diff` for the wrong
    // reason. With identical size groups, the only legitimate cause of
    // `commit > diff` is the extra history / experience / purpose terms.
    assert_eq!(
        report.size, commit.features.size,
        "same churn → identical size group; only the absent groups should differ"
    );
    // The diff's size term matches the commit's size term (same churn), but
    // the commit's total adds the history + purpose terms on top.
    assert!(
        commit.risk_score > report.partial_risk_score,
        "commit score {} must exceed the partial diff score {}",
        commit.risk_score,
        report.partial_risk_score
    );
}

#[test]
fn malformed_diff_is_an_error_not_a_panic() {
    let err = score_diff("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ garbage @@\n")
        .expect_err("malformed hunk header must error");
    assert!(
        matches!(err, vcs::Error::InvalidDiff(_)),
        "expected InvalidDiff, got {err:?}"
    );
}
