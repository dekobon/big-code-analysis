// Exact-equality on f64 is intentional here: every compared value is an
// exactly-representable product of small integers (50.0, 10.0, 0.0), so
// `float_cmp` is a false positive — matching the sibling convention in
// score_tests.rs / stats_tests.rs.
#![allow(clippy::float_cmp)]

use super::*;

#[test]
fn hotspot_is_complexity_times_recent_churn() {
    assert_eq!(hotspot_score(10.0, 5), 50.0);
    assert_eq!(hotspot_score(2.5, 4), 10.0);
}

#[test]
fn zero_churn_or_zero_complexity_is_zero() {
    assert_eq!(hotspot_score(0.0, 100), 0.0);
    assert_eq!(hotspot_score(12.0, 0), 0.0);
}

#[test]
fn negative_complexity_clamps_to_zero() {
    // Defends the `.max(0.0)`: a caller passing a nonsensical negative
    // complexity index must not yield a negative product, which would
    // sort the file *below* genuinely cold files.
    assert_eq!(hotspot_score(-3.0, 5), 0.0);
}
