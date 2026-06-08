//! Unit tests for the pure line-span arithmetic that buckets blame
//! entries into function spans. The git-backed end-to-end behaviour is
//! exercised by the integration fixture in `tests/vcs_history.rs`.

use super::LineSpan;

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
