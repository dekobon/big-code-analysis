use super::*;

/// These hotspot products are all exactly representable in f64, but
/// clippy's `float_cmp` forbids `==` on floats project-wide; assert
/// closeness instead.
fn approx(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < f64::EPSILON,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn hotspot_is_complexity_times_recent_churn() {
    approx(hotspot_score(10.0, 5), 50.0);
    approx(hotspot_score(2.5, 4), 10.0);
}

#[test]
fn zero_churn_or_zero_complexity_is_zero() {
    approx(hotspot_score(0.0, 100), 0.0);
    approx(hotspot_score(12.0, 0), 0.0);
}

#[test]
fn negative_complexity_clamps_to_zero() {
    // Defends the `.max(0.0)`: a caller passing a nonsensical negative
    // complexity index must not yield a negative product, which would
    // sort the file *below* genuinely cold files.
    approx(hotspot_score(-3.0, 5), 0.0);
}
