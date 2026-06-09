use super::*;
use crate::vcs::options::{FileTypeScope, Options};

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

#[test]
fn replay_include_deleted_respects_file_type_scope() {
    // A file deleted at the target ref (present in events, absent from the
    // seed) is re-admitted by --include-deleted only when it passes the
    // file-type scope (issue #576): a deleted `.rs` returns under the
    // default `metrics` scope, a deleted `.md` does not.
    let now = 1_000_000;
    let options = Options {
        long_window_secs: 100_000,
        recent_window_secs: 10_000,
        include_deleted: true,
        file_types: FileTypeScope::Metrics,
        ..Options::default()
    };
    // Seed is empty: both touched files were deleted before the ref.
    let seed = HashMap::new();
    let events = vec![event(
        "c1",
        now - 5_000,
        &[("gone.rs", 7), ("notes.md", 9)],
        &[],
    )];

    let out = replay(seed, &events, &options, now);
    assert!(
        out.files.contains_key(Path::new("gone.rs")),
        "a deleted source file is re-admitted under the metrics scope"
    );
    assert!(
        !out.files.contains_key(Path::new("notes.md")),
        "a deleted non-source file stays out of a metrics-scoped ranking"
    );
}

#[test]
fn replay_include_deleted_all_scope_admits_every_deleted_file() {
    // Under the `all` scope, --include-deleted re-admits a deleted file
    // regardless of extension — the pre-#576 behaviour.
    let now = 1_000_000;
    let options = Options {
        long_window_secs: 100_000,
        recent_window_secs: 10_000,
        include_deleted: true,
        file_types: FileTypeScope::All,
        ..Options::default()
    };
    let events = vec![event("c1", now - 5_000, &[("notes.md", 9)], &[])];
    let out = replay(HashMap::new(), &events, &options, now);
    assert!(
        out.files.contains_key(Path::new("notes.md")),
        "the `all` scope keeps deleted non-source files"
    );
}
