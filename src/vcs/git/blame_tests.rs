//! Unit tests for the pure line-span arithmetic that buckets blame
//! entries into function spans. The git-backed end-to-end behaviour is
//! exercised by the integration fixture in `tests/vcs/vcs_history.rs`.

use std::cell::Cell;

use super::{
    LineRun, LineSpan, MAX_BLAME_ATTEMPTS, ObjectId, is_transient_object_miss, retry_transient,
};

/// Drive `retry_transient` with a synthetic closure whose i-th call
/// returns `outcomes[i]` (and counts calls), treating every `Err` as
/// retryable unless overridden. Returns `(result, calls)`.
fn run_retry(
    attempts: u32,
    outcomes: &[Result<&'static str, &'static str>],
) -> (Result<&'static str, &'static str>, u32) {
    let calls = Cell::new(0u32);
    let result = retry_transient(
        attempts,
        || {
            let i = calls.get();
            calls.set(i + 1);
            outcomes[i as usize]
        },
        |_| true,
    );
    (result, calls.get())
}

#[test]
fn retry_returns_first_ok_without_extra_attempts() {
    // The happy path must cost exactly one call — no overhead when blame
    // succeeds (the overwhelmingly common case).
    let (result, calls) = run_retry(MAX_BLAME_ATTEMPTS, &[Ok("blamed")]);
    assert_eq!(result, Ok("blamed"));
    assert_eq!(calls, 1, "a first-attempt success must not retry");
}

#[test]
fn retry_recovers_after_transient_errors() {
    // Two transient misses then success → returns Ok on the third call,
    // mirroring the gix-odb pack-refresh race clearing once observed.
    let (result, calls) = run_retry(
        MAX_BLAME_ATTEMPTS,
        &[Err("miss"), Err("miss"), Ok("blamed")],
    );
    assert_eq!(result, Ok("blamed"));
    assert_eq!(calls, 3, "should retry twice before the third call wins");
}

#[test]
fn retry_exhausts_budget_and_returns_last_error() {
    // Persistent failure → the front end's graceful degradation must see
    // the final Err after exactly MAX_BLAME_ATTEMPTS calls (no more).
    let (result, calls) = run_retry(
        MAX_BLAME_ATTEMPTS,
        &[Err("miss"), Err("miss"), Err("still")],
    );
    assert_eq!(result, Err("still"));
    assert_eq!(
        calls, MAX_BLAME_ATTEMPTS,
        "budget is one initial attempt plus MAX-1 retries"
    );
}

#[test]
fn retry_stops_on_non_retryable_error() {
    // An error the predicate rejects must fail fast on the first call.
    // Which errors those are is `is_transient_object_miss`'s business,
    // pinned by the tests further down.
    let calls = Cell::new(0u32);
    let result = retry_transient(
        MAX_BLAME_ATTEMPTS,
        || {
            calls.set(calls.get() + 1);
            Err::<&str, &str>("fatal")
        },
        |_| false,
    );
    assert_eq!(result, Err("fatal"));
    assert_eq!(calls.get(), 1, "a non-retryable error must not retry");
}

#[test]
fn retry_with_single_attempt_calls_once() {
    // attempts == 1 means no retry budget at all, even for a retryable
    // error; attempts == 0 must likewise call once, never zero. The guard
    // is the empty `1..attempts` range in `retry_transient`: for attempts
    // 0 or 1 the loop body never runs, so the closure fires exactly once
    // with no counter to underflow.
    let (result, calls) = run_retry(1, &[Err("miss")]);
    assert_eq!(result, Err("miss"));
    assert_eq!(calls, 1);

    let (result, calls) = run_retry(0, &[Err("miss")]);
    assert_eq!(result, Err("miss"));
    assert_eq!(calls, 1, "attempts == 0 still runs exactly one attempt");
}

#[test]
fn line_count_is_inclusive() {
    // A single-line function spans exactly one line; lines 10..=12 span
    // three. Off-by-one here would mis-size every function's `sloc`.
    assert_eq!(LineSpan::new(7, 7).line_count(), 1);
    assert_eq!(LineSpan::new(10, 12).line_count(), 3);
}

#[test]
fn inverted_range_normalises_to_empty_single_line() {
    // A degenerate AST node (end < start) must not blame a negative
    // width; it collapses to a one-line span at `start`.
    let span = LineSpan::new(20, 5);
    assert_eq!(span.line_count(), 1);
    assert_eq!(span.overlap(1, 100), 1);
}

#[test]
fn overlap_counts_shared_lines_only() {
    let span = LineSpan::new(10, 20); // 11 lines
    // Disjoint runs share nothing.
    assert_eq!(span.overlap(1, 9), 0);
    assert_eq!(span.overlap(21, 30), 0);
    // A run fully inside the span contributes its own length.
    assert_eq!(span.overlap(12, 14), 3);
    // A run straddling the lower edge contributes only the inside part.
    assert_eq!(span.overlap(5, 12), 3); // lines 10, 11, 12
    // A run straddling the upper edge likewise.
    assert_eq!(span.overlap(18, 25), 3); // lines 18, 19, 20
    // A run covering the whole span contributes the full span width.
    assert_eq!(span.overlap(1, 100), 11);
}

#[test]
fn overlap_is_inclusive_at_both_boundaries() {
    let span = LineSpan::new(10, 20);
    // A single line exactly on each boundary counts as one shared line.
    assert_eq!(span.overlap(10, 10), 1);
    assert_eq!(span.overlap(20, 20), 1);
    // One line past each boundary shares nothing.
    assert_eq!(span.overlap(9, 9), 0);
    assert_eq!(span.overlap(21, 21), 0);
}

#[test]
fn line_run_from_blame_hunk_converts_zero_based_to_one_based_inclusive() {
    // Normal values: 0-based start 0, length 3 → 1-based inclusive 1..=3.
    let run = LineRun::from_blame_hunk(0, 3, ObjectId::null(gix::hash::Kind::Sha1));
    assert_eq!(run.lo, 1);
    assert_eq!(run.hi, 3);

    // A mid-file hunk: start 9 (line 10), length 1 → 10..=10.
    let single = LineRun::from_blame_hunk(9, 1, ObjectId::null(gix::hash::Kind::Sha1));
    assert_eq!(single.lo, 10);
    assert_eq!(single.hi, 10);
}

#[test]
fn line_run_from_blame_hunk_saturates_at_u32_ceiling() {
    // #809: bare `start + 1` / `lo + len - 1` overflow at the u32 ceiling,
    // panicking in a debug build (overflow checks on). The saturating
    // conversion must clamp instead of panicking.
    let oid = ObjectId::null(gix::hash::Kind::Sha1);

    // start at u32::MAX: `lo = start + 1` saturates to u32::MAX (would panic
    // on the bare `+ 1`). `hi = lo + len - 1` saturates the add to u32::MAX,
    // then the `- 1` yields u32::MAX - 1 — a degenerate inverted run at the
    // impossible ceiling, but defined and panic-free rather than wrapping.
    let run = LineRun::from_blame_hunk(u32::MAX, 5, oid);
    assert_eq!(run.lo, u32::MAX);
    assert_eq!(run.hi, u32::MAX - 1);

    // A huge length from a near-ceiling start saturates the `lo + len` add
    // (would wrap on bare `+`); the `- 1` then leaves hi at u32::MAX - 1.
    let wide = LineRun::from_blame_hunk(u32::MAX - 1, u32::MAX, oid);
    assert_eq!(wide.lo, u32::MAX);
    assert_eq!(wide.hi, u32::MAX - 1);
}

/// Round-tripping a repository through [`gix::ThreadSafeRepository`]
/// drops the ODB object cache, and the rebuilt thread-local handle does
/// **not** get one back — so the cache has to be set *after*
/// `to_thread_local`, which is what `PerFunctionBlame::new_state` does
/// (issue #1117).
///
/// Pinning gix's half of that here rather than trusting a read of its
/// sources: `into_sync` keeps only `objects.into_inner().store()`,
/// `to_thread_local` rebuilds through `gix_odb::Cache::from` (which sets
/// `object_cache: None`), and the `setup_objects` that runs on rebuild
/// re-applies a pack cache unconditionally but an object cache only when
/// `gitoxide.objects.cacheLimit` is non-zero — and it defaults to zero.
/// A future gix that starts propagating the cache fails the first
/// assertion, at which point `new_state`'s call becomes redundant rather
/// than load-bearing, and this is the place that says so.
///
/// No commit is needed: the question is about handle construction, not
/// about any object in the database.
#[test]
fn thread_local_handle_carries_no_object_cache_until_one_is_set() {
    let dir = tempfile::tempdir().expect("temp dir");
    let repo = gix::init(dir.path()).expect("init repository");
    let shared = repo.into_sync();

    let plain = shared.to_thread_local();
    assert!(
        !plain.objects.has_object_cache(),
        "gix handed back an object cache on a fresh thread-local handle; \
         if that is now the default, new_state's object_cache_size_if_unset \
         is no longer load-bearing"
    );
    // The *pack* cache is re-applied on rebuild, so an assertion that
    // merely checked "some cache exists" would pass against the bug.
    assert!(
        plain.objects.has_pack_cache(),
        "setup_objects re-applies the pack cache unconditionally"
    );

    let mut cached = shared.to_thread_local();
    cached.object_cache_size_if_unset(super::super::OBJECT_CACHE_BYTES);
    assert!(
        cached.objects.has_object_cache(),
        "object_cache_size_if_unset must take on the thread-local handle"
    );
}

/// The state a session blames through must carry the object cache
/// (issue #1117) — the production half of the contract the test above
/// pins on gix's side.
///
/// Worth its own test because deleting `new_state`'s
/// `object_cache_size_if_unset` is invisible everywhere else: an object
/// cache changes only how fast a blame runs, never what it returns, so
/// no value assertion anywhere in the suite can see it.
///
/// The fixture only needs a resolvable `HEAD`, not a blameable file, so
/// the commit is written in-process against the empty tree rather than
/// through the `git` CLI.
#[test]
fn a_session_handle_carries_the_object_cache() {
    let dir = tempfile::tempdir().expect("temp dir");
    let repo = gix::init(dir.path()).expect("init repository");
    let who = gix::actor::SignatureRef {
        name: "Ada".into(),
        email: "ada@example.com".into(),
        time: "1700000000 +0000",
    };
    repo.commit_as(
        who,
        who,
        "HEAD",
        "seed",
        repo.empty_tree().id,
        gix::commit::NO_PARENT_IDS,
    )
    .expect("write seed commit");

    let engine = super::PerFunctionBlame::open(dir.path(), crate::vcs::options::Options::default())
        .expect("open blame engine");
    assert!(
        engine.new_state().repo.objects.has_object_cache(),
        "a blame session must set an object cache on its thread-local handle"
    );
}

/// `is_transient_object_miss` decides whether a blame or post-blame
/// commit lookup is retried. Getting it wrong is silent in both
/// directions: too narrow and the #579 race resurfaces as a hard failure,
/// too wide and a wrong-kind object or a broken handle burns the whole
/// retry budget before reporting what was actually wrong.
///
/// Since `gix` 0.88 the predicate reads an error *class*, so the errors
/// here come from real `find_commit` calls rather than hand-built values:
/// what is under test is as much gix's classification of each failure as
/// the predicate itself. A test that only supplied the retryable value
/// would pass against `|_| true`, so each rejected shape is produced too.
#[test]
fn only_a_not_found_commit_lookup_is_retried() {
    let dir = tempfile::tempdir().expect("temp dir");
    let repo = gix::init(dir.path()).expect("init repository");

    let absent =
        ObjectId::from_hex(b"1111111111111111111111111111111111111111").expect("valid hex");
    let missing = repo
        .find_commit(absent)
        .expect_err("an id with no object behind it cannot resolve");
    // Genuinely absent rather than the race, but the two share
    // `Class::NotFound` and nothing else, so this is retried as well.
    assert!(
        is_transient_object_miss(&missing),
        "a NotFound miss carries the #579 race's class and must be retried: {missing}"
    );

    let not_a_commit = repo
        .find_commit(repo.empty_tree().id)
        .expect_err("the empty tree is not a commit");
    assert!(
        !is_transient_object_miss(&not_a_commit),
        "an object that is not a commit will never become one; retrying cannot help: {not_a_commit}"
    );

    let broken = gix::Error::from_error(std::io::Error::other("odb handle is closed"));
    assert!(
        !is_transient_object_miss(&broken),
        "an unclassified lookup failure is deterministic — retrying it only delays the error"
    );
}

/// The #579 race surfaces inside `blame_file`, where gix-blame wraps the
/// object lookup's `NotFound` in its own unclassified message ("Could not
/// find existing blob or commit"). The retry fires only if the class is
/// still visible through that wrapper, which the `find_commit` errors
/// above cannot show.
///
/// The race itself cannot be provoked on demand, so the fixture builds the
/// same error shape deterministically: a committed tree whose blob entry
/// names an object that was never written.
#[test]
fn a_blame_whose_blob_lookup_misses_is_retried() {
    let dir = tempfile::tempdir().expect("temp dir");
    let repo = gix::init(dir.path()).expect("init repository");
    let dangling =
        ObjectId::from_hex(b"2222222222222222222222222222222222222222").expect("valid hex");
    let tree = gix::objs::Tree {
        entries: vec![gix::objs::tree::Entry {
            mode: gix::objs::tree::EntryKind::Blob.into(),
            filename: "f.txt".into(),
            oid: dangling,
        }],
    };
    let tree_id = repo.write_object(&tree).expect("write tree").detach();
    let who = gix::actor::SignatureRef {
        name: "Ada".into(),
        email: "ada@example.com".into(),
        time: "1700000000 +0000",
    };
    let head = repo
        .commit_as(
            who,
            who,
            "HEAD",
            "seed",
            tree_id,
            gix::commit::NO_PARENT_IDS,
        )
        .expect("write seed commit")
        .detach();

    let engine = super::PerFunctionBlame::open(dir.path(), crate::vcs::options::Options::default())
        .expect("open blame engine");
    let err = engine
        .new_state()
        .repo
        .blame_file("f.txt".into(), head, engine.blame_options())
        .expect_err("the blob behind f.txt was never written");
    assert!(
        is_transient_object_miss(&err),
        "a blob miss under gix-blame's wrapper must still read as NotFound: {err}"
    );
}
