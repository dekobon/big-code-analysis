use super::*;
use crate::vcs::options::{Options, RiskFormula};
use std::path::PathBuf;

fn sample_event(oid: &str, time: i64) -> CommitEvent {
    CommitEvent {
        oid: oid.to_owned(),
        time,
        authors: vec!["abc123".to_owned()],
        bug_fix: false,
        security_fix: false,
        revert: false,
        renames: Vec::new(),
        touched: vec![(PathBuf::from("a.rs"), 3)],
    }
}

fn sample_cache(fingerprint: u64) -> HistoryCache {
    HistoryCache {
        cache_schema_version: CACHE_SCHEMA_VERSION,
        vcs_schema_version: VCS_SCHEMA_VERSION,
        risk_score_version: RISK_SCORE_VERSION,
        options_fingerprint: fingerprint,
        head_sha: "deadbeef".to_owned(),
        walk_long_boundary: 0,
        truncated_shallow_clone: false,
        events: vec![sample_event("deadbeef", 10)],
    }
}

#[test]
fn fingerprint_ignores_finalization_only_knobs() {
    // Formula, author-detail emission, deleted-file inclusion, and the
    // bus-factor options are all applied at *replay*, so changing one must
    // reuse the same cached event log (same fingerprint).
    let base = Options::default();
    let cases = [
        Options {
            risk_formula: RiskFormula::Percentile,
            ..base.clone()
        },
        Options {
            emit_author_details: true,
            ..base.clone()
        },
        Options {
            include_deleted: true,
            ..base.clone()
        },
        Options {
            compute_bus_factor: true,
            ..base.clone()
        },
        // The revision name does not change the event log — the resolved
        // head SHA keys the entry — so it is excluded too.
        Options {
            reference: "main".to_owned(),
            ..base.clone()
        },
    ];
    for case in cases {
        assert_eq!(fingerprint(&base), fingerprint(&case));
    }
}

#[test]
fn fingerprint_changes_with_walk_affecting_knobs() {
    let base = Options::default();
    let cases = [
        Options {
            long_window_secs: base.long_window_secs + 1,
            ..base.clone()
        },
        Options {
            recent_window_secs: base.recent_window_secs + 1,
            ..base.clone()
        },
        Options {
            full_history: true,
            ..base.clone()
        },
        Options {
            include_merges: true,
            ..base.clone()
        },
        Options {
            follow_renames: false,
            ..base.clone()
        },
        Options {
            exclude_bots: false,
            ..base.clone()
        },
        Options {
            bot_pattern: "custom".to_owned(),
            ..base.clone()
        },
        Options {
            as_of: Some(42),
            ..base.clone()
        },
    ];
    for case in cases {
        assert_ne!(
            fingerprint(&base),
            fingerprint(&case),
            "a walk-affecting option must change the fingerprint"
        );
    }
}

#[test]
fn is_compatible_requires_matching_versions_and_fingerprint() {
    // `sample_cache` is a non-shallow entry, so the current shallow state
    // must be `false` for it to be reusable.
    let cache = sample_cache(7);
    assert!(cache.is_compatible(7, false));
    assert!(
        !cache.is_compatible(8, false),
        "fingerprint mismatch invalidates"
    );

    let stale_format = HistoryCache {
        cache_schema_version: CACHE_SCHEMA_VERSION + 1,
        ..sample_cache(7)
    };
    assert!(
        !stale_format.is_compatible(7, false),
        "format bump invalidates"
    );

    // `vcs_schema_version` bumps when the emitted output shape changes; an
    // entry walked under an old shape must not be replayed (#930).
    let stale_schema = HistoryCache {
        vcs_schema_version: VCS_SCHEMA_VERSION + 1,
        ..sample_cache(7)
    };
    assert!(
        !stale_schema.is_compatible(7, false),
        "vcs schema bump invalidates"
    );

    let stale_score = HistoryCache {
        risk_score_version: RISK_SCORE_VERSION + 1,
        ..sample_cache(7)
    };
    assert!(
        !stale_score.is_compatible(7, false),
        "score bump invalidates"
    );
}

#[test]
fn is_compatible_requires_matching_shallow_state() {
    // A cache entry written from a shallow clone (truncated_shallow_clone =
    // true) must NOT be reused once the repo is deepened (current shallow =
    // false): the truncated event counts would otherwise be replayed while
    // the index reports `shallow = false`. The reverse — a full-walk entry
    // reused under a now-shallow clone — would report a false truncation
    // flag over complete counts. A plain equality guard handles both
    // directions (issue #810).
    let shallow_entry = HistoryCache {
        truncated_shallow_clone: true,
        ..sample_cache(7)
    };
    assert!(
        shallow_entry.is_compatible(7, true),
        "shallow entry reusable while the repo is still shallow"
    );
    assert!(
        !shallow_entry.is_compatible(7, false),
        "shallow-walked entry must not be reused after unshallow"
    );

    let full_entry = sample_cache(7);
    assert!(
        full_entry.is_compatible(7, false),
        "full entry reusable while the repo is full"
    );
    assert!(
        !full_entry.is_compatible(7, true),
        "full-walk entry must not be reused under a now-shallow clone"
    );
}

#[test]
fn commit_event_roundtrips_and_omits_false_flags() {
    let event = sample_event("abc", 10);
    let json = serde_json::to_string(&event).expect("serialize");
    // Default-false class flags and empty renames are omitted to keep the
    // on-disk log compact.
    assert!(!json.contains("bug_fix"), "{json}");
    assert!(!json.contains("renames"), "{json}");
    let back: CommitEvent = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.oid, "abc");
    assert_eq!(back.author_ids().len(), 1);
    assert!(!back.classification().bug_fix);
}

#[test]
fn write_load_roundtrip_then_clear() {
    let dir = tempfile::tempdir().expect("temp dir");
    let repo = repo_dir(dir.path(), Path::new("/some/repo"));
    let path = entry_path(&repo, "deadbeef");
    let cache = sample_cache(99);

    write_atomic(&path, &cache).expect("write");
    let loaded = load(&path).expect("load");
    assert_eq!(loaded.head_sha, cache.head_sha);
    assert_eq!(loaded.events.len(), 1);

    // A compatible entry is discoverable for an incremental splice (the
    // sample entry is non-shallow, so the current run must be non-shallow).
    let found = load_compatible(&repo, 99, false);
    assert_eq!(found.len(), 1);
    // An incompatible fingerprint yields no candidates.
    assert!(load_compatible(&repo, 100, false).is_empty());
    // A shallow-state mismatch also yields no candidates: the non-shallow
    // entry is not an incremental-splice base for a now-shallow walk (#810).
    assert!(load_compatible(&repo, 99, true).is_empty());

    // Clearing removes the directory; a second clear on the now-missing
    // directory is a no-op, not an error.
    clear_repo(&repo).expect("clear");
    clear_repo(&repo).expect("clear again is idempotent");
    assert!(load(&path).is_none());
}

#[test]
fn distinct_repo_paths_get_distinct_directories() {
    let root = Path::new("/cache");
    assert_ne!(
        repo_dir(root, Path::new("/a/repo")),
        repo_dir(root, Path::new("/b/repo"))
    );
    // The same path is stable across calls.
    assert_eq!(
        repo_dir(root, Path::new("/a/repo")),
        repo_dir(root, Path::new("/a/repo"))
    );
}

#[test]
fn load_ignores_missing_and_corrupt_files() {
    let dir = tempfile::tempdir().expect("temp dir");
    assert!(load(&dir.path().join("absent.json")).is_none());
    let corrupt = dir.path().join("corrupt.json");
    std::fs::write(&corrupt, b"not json").expect("write corrupt");
    assert!(
        load(&corrupt).is_none(),
        "a corrupt entry is ignored, not fatal"
    );
}
