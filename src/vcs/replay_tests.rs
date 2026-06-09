use super::*;
use crate::vcs::options::Options;

/// Build a [`CommitEvent`] from terse `(path, churn)` and `(src, dst)`
/// rename literals — keeps the table-driven cases below readable.
fn event(oid: &str, time: i64, touched: &[(&str, u64)], renames: &[(&str, &str)]) -> CommitEvent {
    CommitEvent {
        oid: oid.to_owned(),
        time,
        authors: vec![format!("digest-of-{oid}")],
        bug_fix: false,
        security_fix: false,
        revert: false,
        renames: renames
            .iter()
            .map(|(s, d)| (PathBuf::from(s), PathBuf::from(d)))
            .collect(),
        touched: touched
            .iter()
            .map(|(p, c)| (PathBuf::from(p), *c))
            .collect(),
    }
}

fn windowed_options(long: i64, recent: i64) -> Options {
    Options {
        long_window_secs: long,
        recent_window_secs: recent,
        ..Options::default()
    }
}

#[test]
fn replay_counts_in_window_commits_and_prunes_old_ones() {
    let now = 1_000_000;
    let options = windowed_options(100_000, 10_000);
    let seed = HashMap::from([(PathBuf::from("a.rs"), 50)]);
    let events = vec![
        event("c2", now - 5_000, &[("a.rs", 4)], &[]), // recent + long
        event("c1", now - 50_000, &[("a.rs", 6)], &[]), // long only
        event("old", now - 200_000, &[("a.rs", 99)], &[]), // outside the long window → pruned
    ];

    let out = replay(seed, &events, &options, now);
    let stats = out.files.get(Path::new("a.rs")).expect("a.rs present");
    assert_eq!(stats.commits_long, 2);
    assert_eq!(stats.commits_recent, 1);
    assert_eq!(stats.churn_long, 10);
    assert_eq!(stats.churn_recent, 4);
}

#[test]
fn replay_seeds_inactive_files_with_zero_counts() {
    // A tracked file present at the ref but never touched in-window still
    // emits a zeroed record (distinct from an untracked file with no block).
    let now = 1_000_000;
    let options = windowed_options(100_000, 10_000);
    let seed = HashMap::from([(PathBuf::from("idle.rs"), 12)]);
    let out = replay(seed, &[], &options, now);
    let stats = out
        .files
        .get(Path::new("idle.rs"))
        .expect("idle.rs present");
    assert_eq!(stats.commits_long, 0);
    assert_eq!(stats.churn_long, 0);
}

#[test]
fn replay_stitches_renames_across_the_event_log() {
    // A newer commit renames a.rs → b.rs; an older commit edited a.rs.
    // Replaying newest-first must attribute the older edit to b.rs (the
    // name the file carries at the target ref), exactly as the live walk
    // does — and as an incremental splice relies on.
    let now = 1_000_000;
    let options = windowed_options(500_000, 10_000);
    let seed = HashMap::from([(PathBuf::from("b.rs"), 50)]);
    let events = vec![
        event("rename", now - 1_000, &[("b.rs", 2)], &[("a.rs", "b.rs")]),
        event("old", now - 100_000, &[("a.rs", 5)], &[]),
    ];

    let out = replay(seed, &events, &options, now);
    let stats = out.files.get(Path::new("b.rs")).expect("b.rs present");
    assert_eq!(
        stats.commits_long, 2,
        "the older a.rs edit must attribute to the renamed b.rs"
    );
    assert_eq!(stats.churn_long, 7);
    // a.rs is gone at the ref and not opted-in via --include-deleted.
    assert!(!out.files.contains_key(Path::new("a.rs")));
}

#[test]
fn replay_reclamps_window_to_the_current_now() {
    // The same event log windowed at two reference times yields different
    // recent counts — proving replay re-windows rather than freezing the
    // values at cache-write time.
    let options = windowed_options(1_000_000, 10_000);
    let seed = HashMap::from([(PathBuf::from("a.rs"), 50)]);
    let commit_time = 500_000;
    let events = vec![event("c", commit_time, &[("a.rs", 4)], &[])];

    let fresh = replay(seed.clone(), &events, &options, commit_time + 5_000);
    assert_eq!(
        fresh.files.get(Path::new("a.rs")).unwrap().commits_recent,
        1,
        "within the recent window at the earlier now"
    );

    let later = replay(seed, &events, &options, commit_time + 500_000);
    assert_eq!(
        later.files.get(Path::new("a.rs")).unwrap().commits_recent,
        0,
        "the same commit has aged out of the recent window at the later now"
    );
}
