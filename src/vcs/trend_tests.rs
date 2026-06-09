// The asserted risk scores are exactly-representable literals from
// integer-valued fixtures, so `float_cmp` is a false positive here —
// matching the sibling convention in the other vcs `*_tests.rs`.
#![allow(clippy::float_cmp)]

use super::*;

/// Build a [`Stats`] carrying just the `risk_score` the delta logic reads.
fn stats(risk: f64) -> Stats {
    Stats {
        risk_score: risk,
        ..Stats::default()
    }
}

#[test]
fn timestamps_hit_both_endpoints_exactly() {
    // 12 points over 24 "units": oldest is end-span, newest is end.
    let end = 1_000_000;
    let span = 24_000;
    let stamps = timestamps(end, span, 12);
    assert_eq!(stamps.len(), 12);
    assert_eq!(*stamps.first().expect("first"), end - span);
    assert_eq!(*stamps.last().expect("last"), end);
}

#[test]
fn timestamps_are_monotonic_non_decreasing() {
    let stamps = timestamps(500, 333, 7);
    assert!(stamps.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn timestamps_two_points_are_just_the_endpoints() {
    assert_eq!(timestamps(100, 30, 2), vec![70, 100]);
}

#[test]
fn timestamps_degenerate_single_point_is_the_end() {
    // The public path validates `points >= 2`; the helper still must not
    // divide by zero on a degenerate count.
    assert_eq!(timestamps(100, 30, 1), vec![100]);
    assert_eq!(timestamps(100, 30, 0), vec![100]);
}

#[test]
fn validate_points_rejects_below_minimum() {
    assert!(matches!(
        validate_points(MIN_TREND_POINTS - 1),
        Err(Error::InvalidTrend(_))
    ));
}

#[test]
fn validate_points_rejects_above_maximum() {
    assert!(matches!(
        validate_points(MAX_TREND_POINTS + 1),
        Err(Error::InvalidTrend(_))
    ));
}

#[test]
fn validate_points_accepts_in_range() {
    assert_eq!(validate_points(MIN_TREND_POINTS).expect("min ok"), 2);
    assert_eq!(
        validate_points(MAX_TREND_POINTS).expect("max ok"),
        MAX_TREND_POINTS
    );
}

#[test]
fn from_snapshots_aligns_points_and_marks_absent_as_none() {
    // a.rs present at points 0 and 2 (absent at 1); b.rs only at point 1.
    let stamps = vec![10, 20, 30];
    let mut p0 = HashMap::new();
    p0.insert(PathBuf::from("a.rs"), stats(5.0));
    let mut p1 = HashMap::new();
    p1.insert(PathBuf::from("b.rs"), stats(9.0));
    let mut p2 = HashMap::new();
    p2.insert(PathBuf::from("a.rs"), stats(3.0));

    let trend = Trend::from_snapshots(stamps, vec![p0, p1, p2], 365, 90, false);

    assert_eq!(trend.len(), 2);
    let files: HashMap<_, _> = trend.iter().map(|(p, s)| (p.clone(), s.to_vec())).collect();
    let a = &files[&PathBuf::from("a.rs")];
    assert_eq!(a.len(), 3);
    assert_eq!(a[0].as_ref().map(|s| s.risk_score), Some(5.0));
    assert!(a[1].is_none(), "a.rs absent at point 1 must be None");
    assert_eq!(a[2].as_ref().map(|s| s.risk_score), Some(3.0));
    let b = &files[&PathBuf::from("b.rs")];
    assert!(b[0].is_none() && b[2].is_none());
    assert_eq!(b[1].as_ref().map(|s| s.risk_score), Some(9.0));
}

#[test]
fn from_snapshots_carries_window_and_shallow_metadata() {
    let trend = Trend::from_snapshots(
        vec![1, 2],
        vec![HashMap::new(), HashMap::new()],
        365,
        90,
        true,
    );
    assert_eq!(trend.long_window_days(), 365);
    assert_eq!(trend.recent_window_days(), 90);
    assert!(trend.truncated_shallow_clone());
    assert!(trend.is_empty());
}

#[test]
fn deltas_split_improved_from_regressed_and_sort_by_magnitude() {
    // down.rs: 8 -> 2 (improved, -6). up.rs: 1 -> 5 (regressed, +4).
    // big.rs: 10 -> 1 (improved, -9, ranks ahead of down.rs).
    let stamps = vec![0, 1];
    let snap = |a: f64, b: f64, c: f64| {
        let mut m = HashMap::new();
        m.insert(PathBuf::from("down.rs"), stats(a));
        m.insert(PathBuf::from("up.rs"), stats(b));
        m.insert(PathBuf::from("big.rs"), stats(c));
        m
    };
    let trend = Trend::from_snapshots(
        stamps,
        vec![snap(8.0, 1.0, 10.0), snap(2.0, 5.0, 1.0)],
        365,
        90,
        false,
    );

    let deltas = trend.deltas(0);
    let improved: Vec<_> = deltas
        .improved
        .iter()
        .map(|d| (d.path.clone(), d.delta))
        .collect();
    assert_eq!(
        improved,
        vec![
            (PathBuf::from("big.rs"), -9.0),
            (PathBuf::from("down.rs"), -6.0),
        ]
    );
    assert_eq!(deltas.regressed.len(), 1);
    assert_eq!(deltas.regressed[0].path, PathBuf::from("up.rs"));
    assert_eq!(deltas.regressed[0].delta, 4.0);
}

#[test]
fn deltas_break_equal_magnitude_ties_by_path() {
    // Two files improve by the same amount (-3); the tie must resolve on
    // path ascending so the output is deterministic.
    let stamps = vec![0, 1];
    let mut p0 = HashMap::new();
    p0.insert(PathBuf::from("z.rs"), stats(5.0));
    p0.insert(PathBuf::from("a.rs"), stats(5.0));
    let mut p1 = HashMap::new();
    p1.insert(PathBuf::from("z.rs"), stats(2.0));
    p1.insert(PathBuf::from("a.rs"), stats(2.0));
    let trend = Trend::from_snapshots(stamps, vec![p0, p1], 365, 90, false);

    let improved: Vec<_> = trend
        .deltas(0)
        .improved
        .iter()
        .map(|d| d.path.clone())
        .collect();
    assert_eq!(
        improved,
        vec![PathBuf::from("a.rs"), PathBuf::from("z.rs")],
        "equal deltas sort by path ascending"
    );
}

#[test]
fn deltas_top_truncates_each_list() {
    let stamps = vec![0, 1];
    let mut p0 = HashMap::new();
    let mut p1 = HashMap::new();
    // Increasing improvement magnitude so truncation order is clear.
    for (name, start) in [("a", 10.0), ("b", 11.0), ("c", 12.0)] {
        p0.insert(PathBuf::from(name), stats(start));
        p1.insert(PathBuf::from(name), stats(0.0));
    }
    let trend = Trend::from_snapshots(stamps, vec![p0, p1], 365, 90, false);
    assert_eq!(trend.deltas(1).improved.len(), 1);
    assert_eq!(trend.deltas(2).improved.len(), 2);
}

#[test]
fn deltas_ignore_files_present_at_fewer_than_two_points() {
    // once.rs exists only at the last point — no direction to report.
    let stamps = vec![0, 1];
    let p0: HashMap<PathBuf, Stats> = HashMap::new();
    let mut p1 = HashMap::new();
    p1.insert(PathBuf::from("once.rs"), stats(7.0));
    let trend = Trend::from_snapshots(stamps, vec![p0, p1], 365, 90, false);
    let deltas = trend.deltas(0);
    assert!(deltas.improved.is_empty() && deltas.regressed.is_empty());
}

#[test]
fn deltas_exclude_zero_movement() {
    let stamps = vec![0, 1];
    let mut p0 = HashMap::new();
    p0.insert(PathBuf::from("flat.rs"), stats(4.0));
    let mut p1 = HashMap::new();
    p1.insert(PathBuf::from("flat.rs"), stats(4.0));
    let trend = Trend::from_snapshots(stamps, vec![p0, p1], 365, 90, false);
    let deltas = trend.deltas(0);
    assert!(deltas.improved.is_empty() && deltas.regressed.is_empty());
}

#[test]
fn deltas_use_first_and_last_present_points() {
    // gap.rs: present at 0 (risk 6) and 2 (risk 1), absent at 1.
    // The delta must span points 0..2, not the adjacent present pair.
    let stamps = vec![100, 200, 300];
    let mut p0 = HashMap::new();
    p0.insert(PathBuf::from("gap.rs"), stats(6.0));
    let p1: HashMap<PathBuf, Stats> = HashMap::new();
    let mut p2 = HashMap::new();
    p2.insert(PathBuf::from("gap.rs"), stats(1.0));
    let trend = Trend::from_snapshots(stamps, vec![p0, p1, p2], 365, 90, false);
    let deltas = trend.deltas(0);
    assert_eq!(deltas.improved.len(), 1);
    let d = &deltas.improved[0];
    assert_eq!((d.first_as_of, d.last_as_of), (100, 300));
    assert_eq!((d.first_risk_score, d.last_risk_score), (6.0, 1.0));
    assert_eq!(d.delta, -5.0);
}
