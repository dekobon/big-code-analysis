//! Persistent change-history cache tests (issue #334).
//!
//! The cache is a pure optimization: every path through
//! [`build_history_index_cached`] — a fresh miss, a pure hit, an
//! incremental splice, or a force-push fallback — must produce the same
//! index an uncached [`build_history_index`] would at the same reference
//! time. These tests pin that invariant against real, deterministic git
//! repositories, plus the cache-file side effects (entry creation,
//! supersession, clearing) the issue's acceptance criteria call for.
#![cfg(feature = "vcs-git")]
// Exact-equality on the per-file `Stats` (which embed f64 signals) is the
// point: a cache hit must be *bit-identical* to a fresh walk.
#![allow(clippy::float_cmp)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use big_code_analysis::vcs::{
    self, CacheConfig, Options, build_history_index, build_history_index_cached, cache,
};

mod common;
use common::vcs_fixture::{DAY, FIXED_NOW, Repo};

/// Options pinned to the fixture's reference time so windowing is stable
/// across the back-to-back runs a cache hit requires.
fn opts() -> Options {
    Options {
        as_of: Some(FIXED_NOW),
        compute_bus_factor: true,
        ..Options::default()
    }
}

/// A cache config rooted at an explicit temp directory.
fn config(dir: &Path, enabled: bool, clear: bool) -> CacheConfig {
    CacheConfig {
        enabled,
        clear,
        dir: Some(dir.to_path_buf()),
    }
}

/// Comparable view of an index: every file's stats keyed by path. `Stats`
/// derives `PartialEq`, so this compares the full per-file signal set
/// (including the f64 entropy / risk fields) exactly.
fn snapshot(index: &vcs::HistoryIndex) -> BTreeMap<String, vcs::Stats> {
    index
        .iter()
        .map(|(path, stats)| (path.to_string_lossy().into_owned(), stats.clone()))
        .collect()
}

/// Count `*.json` cache entries anywhere under `root` (the cache nests one
/// directory per repository).
fn count_entries(root: &Path) -> usize {
    fn walk(dir: &Path, count: &mut usize) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, count);
            } else if path.extension().is_some_and(|ext| ext == "json") {
                *count += 1;
            }
        }
    }
    let mut count = 0;
    walk(root, &mut count);
    count
}

/// Overwrite every persisted `*.json` cache entry under `root` with
/// garbage, so the next run must recover by recomputing.
fn corrupt_entries(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            corrupt_entries(&path);
        } else if path.extension().is_some_and(|ext| ext == "json") {
            std::fs::write(&path, b"{ not valid").expect("corrupt");
        }
    }
}

/// A small repo with three in-window commits touching two files.
fn build_repo() -> Repo {
    let repo = Repo::init();
    repo.write("a.rs", "fn a() {}\n");
    repo.commit(
        "Ada",
        "ada@example.com",
        FIXED_NOW - 30 * DAY,
        "feat: add a",
    );
    repo.write("b.rs", "fn b() {}\n");
    repo.commit(
        "Grace",
        "grace@example.com",
        FIXED_NOW - 10 * DAY,
        "fix: b crash",
    );
    repo.write("a.rs", "fn a() { work(); }\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 2 * DAY, "fix: a bug");
    repo
}

#[test]
fn cache_hit_is_bit_identical_to_a_fresh_walk() {
    let repo = build_repo();
    let cache_dir = tempfile::tempdir().expect("tempdir");
    let cfg = config(cache_dir.path(), true, false);

    let uncached = build_history_index(repo.path(), &opts()).expect("uncached");
    let miss = build_history_index_cached(repo.path(), &opts(), &cfg).expect("miss writes");
    let hit = build_history_index_cached(repo.path(), &opts(), &cfg).expect("hit replays");

    assert_eq!(snapshot(&uncached), snapshot(&miss), "miss == uncached");
    assert_eq!(snapshot(&uncached), snapshot(&hit), "hit == uncached");
    assert_eq!(count_entries(cache_dir.path()), 1, "one entry persisted");
}

#[test]
fn incremental_splice_stitches_a_rename_in_a_new_commit() {
    // The trickiest correctness path: a *new* commit renames a file that a
    // *cached* commit had edited. The splice must re-home the cached edits
    // onto the new name — exactly as a full walk would — proving the raw
    // event log (location paths + rename edges, alias-resolved at replay)
    // survives the incremental boundary.
    let repo = Repo::init();
    repo.write("old.rs", "fn a() {}\n");
    repo.commit(
        "Ada",
        "ada@example.com",
        FIXED_NOW - 40 * DAY,
        "feat: add old",
    );
    repo.write("old.rs", "fn a() { work(); }\n");
    repo.commit(
        "Ada",
        "ada@example.com",
        FIXED_NOW - 20 * DAY,
        "fix: old bug",
    );

    let cache_dir = tempfile::tempdir().expect("tempdir");
    let cfg = config(cache_dir.path(), true, false);
    build_history_index_cached(repo.path(), &opts(), &cfg).expect("prime under old name");

    // New commit is a *pure* rename old.rs → new.rs (no content edit, so
    // gix detects a 100%-similarity rewrite the alias chain can follow).
    repo.git(&["mv", "old.rs", "new.rs"]);
    repo.commit(
        "Ada",
        "ada@example.com",
        FIXED_NOW - DAY,
        "refactor: rename",
    );

    let cached = build_history_index_cached(repo.path(), &opts(), &cfg).expect("incremental");
    let uncached = build_history_index(repo.path(), &opts()).expect("uncached");
    assert_eq!(
        snapshot(&uncached),
        snapshot(&cached),
        "cached edits re-home onto the renamed file across the splice"
    );
    // The renamed file carries the full history (all three commits).
    let new_stats = cached
        .iter()
        .find(|(path, _)| path.to_string_lossy() == "new.rs")
        .map(|(_, stats)| stats)
        .expect("new.rs ranked");
    assert_eq!(new_stats.commits_long, 3, "old edits follow the rename");
}

#[test]
fn incremental_walk_matches_a_full_walk_and_supersedes_the_tail() {
    let repo = build_repo();
    let cache_dir = tempfile::tempdir().expect("tempdir");
    let cfg = config(cache_dir.path(), true, false);

    // Prime the cache at the first HEAD.
    build_history_index_cached(repo.path(), &opts(), &cfg).expect("prime");
    assert_eq!(count_entries(cache_dir.path()), 1);

    // Advance HEAD by one commit; the cached run must splice it on.
    repo.write("c.rs", "fn c() {}\n");
    repo.commit("Ada", "ada@example.com", FIXED_NOW - DAY, "feat: add c");

    let cached = build_history_index_cached(repo.path(), &opts(), &cfg).expect("incremental");
    let uncached = build_history_index(repo.path(), &opts()).expect("uncached");
    assert_eq!(
        snapshot(&uncached),
        snapshot(&cached),
        "incremental splice == full walk"
    );
    // The spliced ancestor entry is removed, so only the new HEAD remains.
    assert_eq!(count_entries(cache_dir.path()), 1, "tail superseded");
}

#[test]
fn force_pushed_history_invalidates_rather_than_reusing_stale_events() {
    let repo = build_repo();
    let cache_dir = tempfile::tempdir().expect("tempdir");
    let cfg = config(cache_dir.path(), true, false);

    // Prime at the original HEAD (last commit churns a.rs by one line).
    build_history_index_cached(repo.path(), &opts(), &cfg).expect("prime");

    // Rewrite HEAD with materially different content (more churn) and a
    // new object id — a force-push: the cached head is no longer an
    // ancestor, so its events must not be reused.
    repo.write(
        "a.rs",
        "fn a() {\n    work();\n    more();\n    extra();\n}\n",
    );
    repo.git(&["add", "-A"]);
    repo.git_at(
        "Ada",
        "ada@example.com",
        FIXED_NOW - 2 * DAY,
        &[
            "commit",
            "--amend",
            "--no-verify",
            "-q",
            "-m",
            "fix: a bug harder",
        ],
    );

    let cached = build_history_index_cached(repo.path(), &opts(), &cfg).expect("post force-push");
    let uncached = build_history_index(repo.path(), &opts()).expect("uncached");
    assert_eq!(
        snapshot(&uncached),
        snapshot(&cached),
        "force-push recomputes; stale events are not reused"
    );
}

#[test]
fn cached_and_uncached_agree_under_emit_author_details() {
    // The cache stores only hashed author digests; replaying them must
    // reproduce the same emitted identities as a fresh walk (the digest is
    // injective, and a reconstructed identity hashes to itself).
    let repo = build_repo();
    let cache_dir = tempfile::tempdir().expect("tempdir");
    let options = Options {
        emit_author_details: true,
        ..opts()
    };

    let fresh = build_history_index(repo.path(), &options).expect("fresh");
    let cfg = config(cache_dir.path(), true, false);
    let miss = build_history_index_cached(repo.path(), &options, &cfg).expect("miss");
    let hit = build_history_index_cached(repo.path(), &options, &cfg).expect("hit");

    assert_eq!(snapshot(&fresh), snapshot(&miss));
    assert_eq!(snapshot(&fresh), snapshot(&hit));
    // Sanity: author ids were actually emitted (so the parity above is
    // exercising the hashed-identity path, not comparing two `None`s).
    let stats = miss
        .iter()
        .find_map(|(_, stats)| stats.author_ids.as_ref())
        .expect("some file emitted author ids");
    assert!(!stats.is_empty());
    // The emitted ids are full SHA-256 hex digests (64 chars), confirming
    // the cache stored digests and replay reproduced them verbatim — not
    // some truncated or double-hashed form.
    assert!(
        stats.iter().all(|digest| digest.len() == 64),
        "author ids are SHA-256 hex digests"
    );
}

#[test]
fn full_history_mode_caches_and_replays() {
    // `--full-history` opts out of the incremental splice (a full-DAG splice
    // is unsound), but a *pure hit* on an unchanged tree must still replay
    // correctly — the exact-entry path is traversal-mode-independent.
    let repo = build_repo();
    let cache_dir = tempfile::tempdir().expect("tempdir");
    let cfg = config(cache_dir.path(), true, false);
    let full = Options {
        full_history: true,
        ..opts()
    };

    let uncached = build_history_index(repo.path(), &full).expect("uncached full-history");
    let miss = build_history_index_cached(repo.path(), &full, &cfg).expect("miss writes");
    let hit = build_history_index_cached(repo.path(), &full, &cfg).expect("hit replays");
    assert_eq!(snapshot(&uncached), snapshot(&miss));
    assert_eq!(snapshot(&uncached), snapshot(&hit));
    assert_eq!(count_entries(cache_dir.path()), 1);
}

#[test]
#[cfg(unix)]
fn an_unwritable_cache_directory_degrades_gracefully() {
    use std::os::unix::fs::PermissionsExt;

    // A cache-write failure is an optimization miss, never a hard error:
    // the build must still return the correct index.
    let repo = build_repo();
    let cache_root = tempfile::tempdir().expect("tempdir");
    // A read-only root that does not yet contain the repo's sub-directory,
    // so `create_dir_all` (and therefore the write) fails.
    std::fs::set_permissions(cache_root.path(), std::fs::Permissions::from_mode(0o555))
        .expect("chmod read-only");
    let cfg = config(cache_root.path(), true, false);

    let result = build_history_index_cached(repo.path(), &opts(), &cfg);

    // Restore permissions before any assertion can unwind the temp-dir drop.
    std::fs::set_permissions(cache_root.path(), std::fs::Permissions::from_mode(0o755))
        .expect("restore perms");

    let cached = result.expect("build succeeds despite an unwritable cache");
    let uncached = build_history_index(repo.path(), &opts()).expect("uncached");
    assert_eq!(
        snapshot(&uncached),
        snapshot(&cached),
        "an unwritable cache does not change the result"
    );
}

#[test]
fn a_changed_window_is_not_served_from_a_stale_fingerprint() {
    let repo = build_repo();
    let cache_dir = tempfile::tempdir().expect("tempdir");
    let cfg = config(cache_dir.path(), true, false);

    // Prime with the default 12-month long window.
    build_history_index_cached(repo.path(), &opts(), &cfg).expect("prime");

    // A shorter long window changes the fingerprint, so the entry is
    // ignored and the result matches a fresh short-window walk (here, the
    // 30-day-old commit drops out of a 7-day long window).
    let short = Options {
        long_window_secs: 7 * DAY,
        recent_window_secs: 2 * DAY,
        ..opts()
    };
    let cached = build_history_index_cached(repo.path(), &short, &cfg).expect("short cached");
    let uncached = build_history_index(repo.path(), &short).expect("short uncached");
    assert_eq!(snapshot(&uncached), snapshot(&cached));
}

#[test]
fn clear_cache_removes_persisted_entries() {
    let repo = build_repo();
    let cache_dir = tempfile::tempdir().expect("tempdir");

    build_history_index_cached(repo.path(), &opts(), &config(cache_dir.path(), true, false))
        .expect("prime");
    assert!(count_entries(cache_dir.path()) >= 1);

    // `clear` with caching otherwise disabled wipes without re-priming.
    build_history_index_cached(repo.path(), &opts(), &config(cache_dir.path(), false, true))
        .expect("clear");
    assert_eq!(count_entries(cache_dir.path()), 0, "cleared, not re-primed");
}

#[test]
fn no_cache_neither_reads_nor_writes() {
    let repo = build_repo();
    let cache_dir = tempfile::tempdir().expect("tempdir");
    let cfg = config(cache_dir.path(), false, false);

    let disabled = build_history_index_cached(repo.path(), &opts(), &cfg).expect("disabled");
    let uncached = build_history_index(repo.path(), &opts()).expect("uncached");
    assert_eq!(snapshot(&uncached), snapshot(&disabled));
    assert_eq!(count_entries(cache_dir.path()), 0, "nothing written");
}

#[test]
fn a_corrupt_entry_is_recomputed_not_fatal() {
    let repo = build_repo();
    let cache_dir = tempfile::tempdir().expect("tempdir");
    let cfg = config(cache_dir.path(), true, false);

    build_history_index_cached(repo.path(), &opts(), &cfg).expect("prime");

    // Overwrite every persisted entry with garbage; the next run must
    // recover by recomputing rather than erroring.
    corrupt_entries(cache_dir.path());

    let recovered = build_history_index_cached(repo.path(), &opts(), &cfg).expect("recover");
    let uncached = build_history_index(repo.path(), &opts()).expect("uncached");
    assert_eq!(snapshot(&uncached), snapshot(&recovered));
}

/// Performance acceptance check (issue #334): an incremental update of a
/// small delta on a large history must be far cheaper than a full walk.
///
/// `#[ignore]`d because it builds a few hundred commits through the `git`
/// CLI (seconds), which is too slow for the per-PR suite. Run with
/// `cargo test --features vcs --test vcs_cache -- --ignored`. The
/// supersession assertion in the non-ignored incremental test already
/// proves the *splice path* is taken; this adds the wall-clock evidence.
#[test]
#[ignore = "builds a large history; run explicitly with --ignored"]
fn incremental_update_is_much_cheaper_than_a_full_walk() {
    use std::time::Instant;

    let repo = Repo::init();
    let base = FIXED_NOW - 200 * DAY;
    let bulk = 400;
    for i in 0..bulk {
        repo.write("a.rs", &format!("fn a() {{ /* rev {i} */ }}\n"));
        repo.commit(
            "Ada",
            "ada@example.com",
            base + i64::from(i) * 600,
            &format!("edit {i}"),
        );
    }

    let cache_dir = tempfile::tempdir().expect("tempdir");
    let cfg = config(cache_dir.path(), true, false);

    // Prime the cache over the whole history.
    build_history_index_cached(repo.path(), &opts(), &cfg).expect("prime");

    // Append a 100-commit delta.
    for i in bulk..bulk + 100 {
        repo.write("a.rs", &format!("fn a() {{ /* rev {i} */ }}\n"));
        repo.commit(
            "Ada",
            "ada@example.com",
            base + i64::from(i) * 600,
            &format!("edit {i}"),
        );
    }

    let full_start = Instant::now();
    let full = build_history_index(repo.path(), &opts()).expect("full");
    let full_elapsed = full_start.elapsed();

    let inc_start = Instant::now();
    let incremental = build_history_index_cached(repo.path(), &opts(), &cfg).expect("incremental");
    let inc_elapsed = inc_start.elapsed();

    assert_eq!(snapshot(&full), snapshot(&incremental), "still correct");
    assert!(
        inc_elapsed * 2 < full_elapsed,
        "incremental ({inc_elapsed:?}) should be far cheaper than a full walk ({full_elapsed:?})"
    );
}

/// The cache's public schema surface is wired up — a smoke check that the
/// re-exported version constant and config type are reachable from a
/// downstream crate (the published API the front ends depend on).
#[test]
fn public_cache_surface_is_reachable() {
    let _: PathBuf = config(Path::new("/tmp/x"), true, false)
        .dir
        .expect("dir set");
    assert_eq!(cache::CACHE_SCHEMA_VERSION, vcs::CACHE_SCHEMA_VERSION);
}
