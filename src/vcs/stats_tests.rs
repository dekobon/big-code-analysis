// Exact-equality on f64 is intentional here: every compared value is
// an exactly-representable literal (0.0, 0.5, 0.75, 1.0) produced by
// exact integer ratios, so `float_cmp` is a false positive in tests.
#![allow(clippy::float_cmp)]

use super::*;
use crate::vcs::options::Options;

const DAY: i64 = 86_400;
const NOW: i64 = 1_700_000_000;

fn author(email: &str) -> AuthorId {
    AuthorId::new(b"Someone", email.as_bytes())
}

fn change(
    churn: u64,
    days_ago: i64,
    authors: &[AuthorId],
    class: Classification,
) -> ChangeRecord<'_> {
    ChangeRecord {
        churn,
        commit_time: NOW - days_ago * DAY,
        in_recent: days_ago <= 90,
        class,
        authors,
        change_entropy: 0.0,
    }
}

#[test]
fn empty_accumulator_finalizes_to_zeros() {
    let stats = Accumulator::new(120).finalize(NOW, &Options::default(), 0.0, 0.0);
    assert_eq!(stats.commits_long, 0);
    assert_eq!(stats.churn_long, 0);
    assert_eq!(stats.authors_long, 0);
    assert_eq!(stats.ownership_top_share, 0.0);
    assert_eq!(stats.burst, 0.0);
    // A tracked-but-untouched file caps age/last-modified at the window.
    assert_eq!(stats.age_days, 365);
    assert_eq!(stats.last_modified_days, 365);
    // No commits → no scattered change and no co-change neighbours.
    assert_eq!(stats.change_entropy_long, 0.0);
    assert_eq!(stats.change_entropy_recent, 0.0);
    assert_eq!(stats.cochange_entropy_long, 0.0);
    assert_eq!(stats.cochange_entropy_recent, 0.0);
    assert_eq!(stats.vcs_schema_version, VCS_SCHEMA_VERSION);
    assert_eq!(stats.risk_score_version, RISK_SCORE_VERSION);
    assert!(stats.risk_score.is_finite());
}

#[test]
fn single_commit_counts_once_with_full_ownership() {
    let mut acc = Accumulator::new(100);
    let ada = [author("ada@example.com")];
    acc.record(&change(40, 10, &ada, Classification::default()));
    let stats = acc.finalize(NOW, &Options::default(), 0.0, 0.0);

    assert_eq!(stats.commits_long, 1);
    assert_eq!(stats.commits_recent, 1);
    assert_eq!(stats.churn_long, 40);
    assert_eq!(stats.churn_recent, 40);
    assert_eq!(stats.authors_long, 1);
    assert_eq!(stats.authors_recent, 1);
    assert_eq!(stats.ownership_top_share, 1.0);
    assert_eq!(stats.burst, 1.0);
    assert_eq!(stats.age_days, 10);
    assert_eq!(stats.last_modified_days, 10);
}

#[test]
fn classification_flags_accumulate() {
    let mut acc = Accumulator::new(100);
    let ada = [author("ada@example.com")];
    acc.record(&change(
        10,
        5,
        &ada,
        Classification {
            bug_fix: true,
            security_fix: false,
            revert: false,
        },
    ));
    acc.record(&change(
        10,
        6,
        &ada,
        Classification {
            bug_fix: true,
            security_fix: true,
            revert: false,
        },
    ));
    acc.record(&change(
        10,
        7,
        &ada,
        Classification {
            bug_fix: false,
            security_fix: false,
            revert: true,
        },
    ));
    let stats = acc.finalize(NOW, &Options::default(), 0.0, 0.0);
    assert_eq!(stats.bug_fix_commits, 2);
    assert_eq!(stats.security_fix_commits, 1);
    assert_eq!(stats.revert_commits, 1);
}

#[test]
fn ownership_reflects_edit_split() {
    let mut acc = Accumulator::new(100);
    let ada = [author("ada@example.com")];
    let grace = [author("grace@example.com")];
    // Ada edits three times, Grace once → top share 3/4.
    for days in [10, 20, 30] {
        acc.record(&change(10, days, &ada, Classification::default()));
    }
    acc.record(&change(10, 40, &grace, Classification::default()));
    let stats = acc.finalize(NOW, &Options::default(), 0.0, 0.0);
    assert_eq!(stats.authors_long, 2);
    assert!((stats.ownership_top_share - 0.75).abs() < 1e-9);
}

#[test]
fn coauthors_count_toward_distinct_authors() {
    let mut acc = Accumulator::new(100);
    let pair = [author("ada@example.com"), author("grace@example.com")];
    acc.record(&change(10, 10, &pair, Classification::default()));
    let stats = acc.finalize(NOW, &Options::default(), 0.0, 0.0);
    // One commit, two participants.
    assert_eq!(stats.commits_long, 1);
    assert_eq!(stats.authors_long, 2);
    // Each credited one edit → 1/2 top share.
    assert!((stats.ownership_top_share - 0.5).abs() < 1e-9);
}

#[test]
fn recent_window_excludes_old_commits() {
    let mut acc = Accumulator::new(100);
    let ada = [author("ada@example.com")];
    acc.record(&change(10, 30, &ada, Classification::default())); // recent
    acc.record(&change(10, 200, &ada, Classification::default())); // long only
    let stats = acc.finalize(NOW, &Options::default(), 0.0, 0.0);
    assert_eq!(stats.commits_long, 2);
    assert_eq!(stats.commits_recent, 1);
    assert_eq!(stats.churn_long, 20);
    assert_eq!(stats.churn_recent, 10);
    assert_eq!(stats.age_days, 200);
    assert_eq!(stats.last_modified_days, 30);
    assert!((stats.burst - 0.5).abs() < 1e-9);
}

#[test]
fn emit_author_details_adds_sorted_hashes() {
    let mut acc = Accumulator::new(100);
    let pair = [author("grace@example.com"), author("ada@example.com")];
    acc.record(&change(10, 10, &pair, Classification::default()));

    let options = Options {
        emit_author_details: true,
        ..Options::default()
    };
    let stats = acc.finalize(NOW, &options, 0.0, 0.0);

    let ids = stats.author_ids.expect("author_ids present");
    assert_eq!(ids.len(), 2);
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "hashed ids are emitted sorted");

    // Off by default.
    let without = acc.finalize(NOW, &Options::default(), 0.0, 0.0);
    assert!(without.author_ids.is_none());
}

#[test]
fn change_entropy_accumulates_per_window() {
    let mut acc = Accumulator::new(100);
    let ada = [author("ada@example.com")];
    // A recent commit contributing 0.5 bits and an old (long-only) commit
    // contributing 0.2 bits. The recent sum sees only the first; the long
    // sum sees both.
    acc.record(&ChangeRecord {
        change_entropy: 0.5,
        ..change(10, 30, &ada, Classification::default())
    });
    acc.record(&ChangeRecord {
        change_entropy: 0.2,
        ..change(10, 200, &ada, Classification::default())
    });
    let stats = acc.finalize(NOW, &Options::default(), 0.0, 0.0);
    assert!((stats.change_entropy_long - 0.7).abs() < 1e-9);
    assert!((stats.change_entropy_recent - 0.5).abs() < 1e-9);
}

#[test]
fn cochange_entropy_is_passed_through_to_stats() {
    // The accumulator never sees the co-change graph; finalize takes the
    // long/recent values from the backend and stores them verbatim.
    let stats = Accumulator::new(100).finalize(NOW, &Options::default(), 1.5, 0.75);
    assert_eq!(stats.cochange_entropy_long, 1.5);
    assert_eq!(stats.cochange_entropy_recent, 0.75);
    // A single-file-only accumulator still reports zero change entropy.
    assert_eq!(stats.change_entropy_long, 0.0);
    assert_eq!(stats.change_entropy_recent, 0.0);
}

#[test]
fn future_dated_commit_clamps_age_to_zero() {
    let mut acc = Accumulator::new(100);
    let ada = [author("ada@example.com")];
    // A commit 5 days in the "future" (clock skew); days_ago negative.
    acc.record(&change(10, -5, &ada, Classification::default()));
    let stats = acc.finalize(NOW, &Options::default(), 0.0, 0.0);
    assert_eq!(stats.age_days, 0);
    assert_eq!(stats.last_modified_days, 0);
}
