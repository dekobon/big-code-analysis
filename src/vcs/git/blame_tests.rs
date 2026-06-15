//! Unit tests for the pure line-span arithmetic that buckets blame
//! entries into function spans. The git-backed end-to-end behaviour is
//! exercised by the integration fixture in `tests/vcs_history.rs`.

use std::cell::Cell;

use super::{LineRun, LineSpan, MAX_BLAME_ATTEMPTS, ObjectId, retry_transient};

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
    // A deterministic failure (predicate returns false) must fail fast on
    // the first call — never burn retries on an object that truly is
    // absent at the ref.
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
    // error; attempts == 0 must likewise call once, never zero (the
    // saturating_sub guard against an unbounded loop).
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
