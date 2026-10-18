// Sibling-file unit tests for `Threshold`, wired in via
// `#[path = "threshold_tests.rs"] mod tests;`. The `./**/*_tests.rs`
// rule in `.bcaignore` keeps this file out of the self-scan walker.

#![allow(clippy::float_cmp)] // Several contracts here are bit-exact.

use super::*;
use crate::metric_catalog::{MetricScope, scale};

const MI_IDS: [&str; 3] = ["mi.original", "mi.sei", "mi.visual_studio"];

fn th(id: &str, limit: f64) -> Threshold {
    let checked = Threshold::check(id, limit).expect("test limit is valid");
    checked.threshold
}

fn approx_eq(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

/// The soft-tier limit `t` resolves to at `ratio`.
fn soft_at(t: Threshold, ratio: f64) -> f64 {
    t.soft_limit(ratio).expect("ratio ok").limit()
}

/// Rows the catalog does not have today, so the direction/scale
/// combinations the arithmetic must stay total over are exercised.
static RATIO_FLOOR: MetricInfo = MetricInfo {
    id: "synthetic.ratio_floor",
    family: "synthetic",
    long_description: "",
    direction: Direction::LowerIsWorse,
    skip_at_unit: false,
    scope: MetricScope::Function,
    scale: Scale::Ratio,
};
static INTERVAL_CEILING: MetricInfo = MetricInfo {
    id: "synthetic.interval_ceiling",
    family: "synthetic",
    long_description: "",
    direction: Direction::HigherIsWorse,
    skip_at_unit: false,
    scope: MetricScope::Function,
    scale: Scale::Interval {
        min: 0.0,
        max: 100.0,
        unit: 10.0,
    },
};

fn synthetic(info: &'static MetricInfo, limit: f64) -> Threshold {
    Threshold { info, limit }
}

fn ratio_ids() -> impl Iterator<Item = &'static str> {
    METRICS
        .iter()
        .filter(|m| matches!(m.scale, Scale::Ratio))
        .map(|m| m.id)
}

#[test]
fn ratio_severity_is_bitwise_value_over_limit() {
    let grid = [
        (0.3, 0.1),
        (16.0, 15.0),
        (30.0, 15.0),
        (1.0, 3.0),
        (50_001.0, 50_000.0),
        (7.7, 0.7),
        (1e-3, 3e-4),
    ];
    let mut n = 0;
    for id in ratio_ids() {
        for (v, limit) in grid {
            let got = th(id, limit).severity(v);
            assert_eq!(
                got.to_bits(),
                (v / limit).to_bits(),
                "{id}: severity({v}) at limit {limit} = {got}, want {}",
                v / limit
            );
            n += 1;
        }
    }
    assert_eq!(n, 22 * grid.len(), "every Ratio row is covered");
}

#[test]
fn ratio_zero_limit_saturates() {
    assert_eq!(th("cyclomatic", 0.0).severity(5.0), f64::INFINITY);
    assert_eq!(th("cyclomatic", 0.0).severity(0.5), f64::INFINITY);
}

#[test]
fn interval_severity_decreases_monotonically_through_zero() {
    let grid = [19.0, 10.0, 1.0, 0.0, -1.0, -14.7, -77.0, -729.0];
    let mut cases: Vec<(&str, f64, &[f64])> =
        MI_IDS.iter().map(|&id| (id, 20.0, &grid[..])).collect();
    let deep = [-51.0, -60.0, -100.5, -400.0];
    cases.push(("mi.original", -50.0, &deep));
    // `mi.visual_studio` never goes below 0, so a breach of limit 0 is
    // impossible; the grid still has to stay finite and ordered over
    // the domain it can be called on.
    let vs = [100.0, 50.0, 10.0, 0.5, 0.0];
    cases.push(("mi.visual_studio", 0.0, &vs));
    for (id, limit, values) in cases {
        let t = th(id, limit);
        let sev: Vec<f64> = values.iter().map(|&v| t.severity(v)).collect();
        for (v, s) in values.iter().zip(&sev) {
            assert!(s.is_finite(), "{id}@{limit}: severity({v}) = {s}");
        }
        for ([s0, s1], [v0, v1]) in sev.array_windows().zip(values.array_windows()) {
            assert!(
                s1 > s0,
                "{id}@{limit}: severity({v1}) = {s1} must exceed severity({v0}) = {s0}"
            );
        }
    }
}

#[test]
fn interval_severity_is_one_plus_excess_over_unit() {
    // `mi.original` unit 17.1: -14.7 against 20 is 34.7 points past.
    let s = th("mi.original", 20.0).severity(-14.7);
    assert!(approx_eq(s, 1.0 + 34.7 / 17.1), "got {s}");
    // `mi.visual_studio` unit 10: 5 against 20 is one and a half bands.
    let s = th("mi.visual_studio", 20.0).severity(5.0);
    assert!(approx_eq(s, 2.5), "got {s}");
}

#[test]
fn every_breach_away_from_the_boundary_has_severity_above_one() {
    let cases: [(&str, f64, &[f64]); 6] = [
        ("cyclomatic", 15.0, &[16.0, 20.0, 1e6]),
        ("cognitive", 1.0, &[2.0, 3.0]),
        ("cyclomatic", 0.0, &[1.0, 2.0]),
        ("mi.original", 20.0, &[19.0, 0.0, -100.0]),
        ("mi.original", -50.0, &[-51.0, -500.0]),
        ("mi.visual_studio", 20.0, &[19.0, 10.0, 0.0]),
    ];
    for (id, limit, values) in cases {
        let t = th(id, limit);
        for &v in values {
            assert!(t.breaches(v), "{id}@{limit}: {v} must breach");
            assert!(
                t.severity(v) > 1.0,
                "{id}@{limit}: severity({v}) = {}",
                t.severity(v)
            );
        }
    }
}

#[test]
fn breaches_is_the_strict_direct_comparison() {
    let ceiling = th("cyclomatic", 15.0);
    assert!(ceiling.breaches(15.5));
    assert!(!ceiling.breaches(15.0), "equal does not breach");
    assert!(!ceiling.breaches(14.0));
    assert!(!ceiling.breaches(f64::NAN));

    let floor = th("mi.original", -50.0);
    assert!(floor.breaches(-50.5));
    assert!(!floor.breaches(-50.0), "equal does not breach");
    assert!(!floor.breaches(-49.0));
    assert!(!floor.breaches(f64::NAN));
}

#[test]
fn excess_is_signed_in_the_bad_direction() {
    assert_eq!(th("cyclomatic", 15.0).excess(20.0), 5.0);
    assert_eq!(th("cyclomatic", 15.0).excess(10.0), -5.0);
    assert_eq!(th("mi.original", 20.0).excess(-5.0), 25.0);
    assert_eq!(th("mi.original", 20.0).excess(30.0), -10.0);
}

#[test]
fn cmp_severity_ranks_ratio_then_excess() {
    // Equal ratio 2.0; the larger excess (15 vs 10) is the worse breach.
    let a = th("cyclomatic", 15.0);
    let b = th("cyclomatic", 10.0);
    assert_eq!(a.severity(30.0), b.severity(20.0));
    assert_eq!(a.cmp_breach(30.0, &b, 20.0), Ordering::Greater);
    assert_eq!(b.cmp_breach(20.0, &a, 30.0), Ordering::Less);
    // Severity decides before excess: 3× over by 2 beats 2× over by 10.
    let small = th("cyclomatic", 1.0);
    assert_eq!(small.cmp_breach(3.0, &b, 20.0), Ordering::Greater);

    // One threshold, saturated severity: excess decides.
    let zero = th("cyclomatic", 0.0);
    assert_eq!(zero.cmp_severity(5.0, 3.0), Ordering::Greater);
    assert_eq!(zero.cmp_severity(3.0, 5.0), Ordering::Less);
    assert_eq!(zero.cmp_severity(3.0, 3.0), Ordering::Equal);

    // One finite threshold: severity decides.
    let mi = th("mi.original", 20.0);
    assert_eq!(mi.cmp_severity(-30.0, 5.0), Ordering::Greater);
    assert_eq!(mi.cmp_severity(5.0, -30.0), Ordering::Less);
}

/// The cross-metric ranking #1621 chose: a cognitive score 13.3× its
/// limit outranks an MI three bands below its floor.
#[test]
fn cognitive_13x_outranks_mi_three_bands_down() {
    let cog = th("cognitive", 15.0).severity(200.0);
    let mi = th("mi.original", 20.0).severity(-14.7);
    assert!(approx_eq(cog, 200.0 / 15.0), "cognitive {cog}");
    assert!(approx_eq(mi, 1.0 + 34.7 / 17.1), "mi {mi}");
    assert!(cog > mi, "cognitive {cog} must outrank mi {mi}");
    let (cog_t, mi_t) = (th("cognitive", 15.0), th("mi.original", 20.0));
    assert_eq!(cog_t.cmp_breach(200.0, &mi_t, -14.7), Ordering::Greater);
    assert_eq!(mi_t.cmp_breach(-14.7, &cog_t, 200.0), Ordering::Less);
}

#[test]
fn soft_limit_interval_moves_the_floor_by_a_fraction_of_a_unit() {
    let s = th("mi.original", 20.0).soft_limit(0.95).expect("ratio ok");
    assert!(approx_eq(s.limit(), 20.855), "got {}", s.limit());
    assert_eq!(s.id(), "mi.original");

    let s = soft_at(th("mi.visual_studio", 20.0), 0.95);
    assert!(approx_eq(s, 20.5), "got {s}");

    let hard = th("mi.original", -50.0);
    let soft = hard.soft_limit(0.95).expect("ratio ok");
    assert!(approx_eq(soft.limit(), -49.145), "got {}", soft.limit());
    assert!(soft.breaches(-49.5), "the soft floor is stricter");
    assert!(!hard.breaches(-49.5));

    // The step is formed as `unit - ratio * unit`, one rounding, so a
    // floor at zero lands within 4 ulps of the decimal a reader expects;
    // `(1 - ratio) * unit` rounds twice and sat 8 ulps above it, past
    // the CLI's sig-fig snap, which then printed 0.855001 (#1621 code
    // review). Measured in true ulps of 0.855 — a bit distance, since
    // `f64::EPSILON` is the spacing at 1.0 and would read 8 as 4. The
    // 10-unit cases are exact.
    let s = soft_at(th("mi.original", 0.0), 0.95);
    let ulps = s.to_bits().abs_diff(0.855_f64.to_bits());
    assert!(ulps <= 4, "0.855 + {ulps} ulps: {s}");
    assert_eq!(soft_at(th("mi.visual_studio", 0.0), 0.95), 0.5);
    assert_eq!(soft_at(th("mi.visual_studio", -1.0), 0.9), 0.0);

    assert_eq!(
        soft_at(th("mi.visual_studio", 100.0), 0.5),
        100.0,
        "clamped at the domain maximum"
    );
    assert_eq!(
        soft_at(th("mi.visual_studio", 99.0), 0.5),
        100.0,
        "99 + 5 clamps to 100"
    );
}

#[test]
fn soft_limit_ratio_scales_a_ceiling() {
    let s = soft_at(th("cyclomatic", 15.0), 0.95);
    assert_eq!(s, 15.0 * 0.95);
    assert!(approx_eq(s, 14.25));
}

#[test]
fn soft_limit_is_never_looser_than_hard() {
    // Out-of-domain floor: clamping to `max` alone would pull it *down*
    // to 100, looser than the hard 101.
    assert_eq!(soft_at(th("mi.visual_studio", 101.0), 0.9), 101.0);
    assert_eq!(soft_at(synthetic(&INTERVAL_CEILING, -5.0), 0.9), -5.0);
    assert!(approx_eq(
        soft_at(synthetic(&INTERVAL_CEILING, 50.0), 0.9),
        49.0
    ));
    assert_eq!(
        soft_at(synthetic(&INTERVAL_CEILING, 3.0), 0.5),
        0.0,
        "clamped at the domain minimum"
    );
    assert!(approx_eq(soft_at(synthetic(&RATIO_FLOOR, 20.0), 0.8), 25.0));
}

/// #1621's soft-tier property over a grid: for every catalog row and
/// every `r < 1`, the soft limit is at least as strict as the hard one,
/// finite, and inside the domain whenever the hard limit is.
#[test]
fn soft_limit_is_stricter_and_finite_over_a_grid() {
    let limits = [-729.0, -50.0, -0.5, 0.0, 0.5, 1.0, 15.0, 99.5, 100.0, 1e6];
    let ratios = [f64::MIN_POSITIVE, 1e-9, 0.1, 0.5, 0.95, 1.0 - f64::EPSILON];
    let mut n = 0;
    for m in METRICS {
        for limit in limits {
            let Ok(Checked {
                threshold: hard, ..
            }) = Threshold::check(m.id, limit)
            else {
                continue;
            };
            for r in ratios {
                let soft = soft_at(hard, r);
                let id = m.id;
                assert!(soft.is_finite(), "{id}={limit} r={r}: {soft}");
                let stricter = match m.direction {
                    Direction::HigherIsWorse => soft <= limit,
                    Direction::LowerIsWorse => soft >= limit,
                };
                assert!(stricter, "{id}={limit} r={r}: {soft}");
                if let Scale::Interval { min, max, .. } = m.scale
                    && (min..=max).contains(&limit)
                {
                    assert!((min..=max).contains(&soft), "{id}={limit} r={r}: {soft}");
                }
                n += 1;
            }
        }
    }
    assert!(n > 0);
    // A ratio-scale floor would overflow `limit / r` without the clamp.
    assert_eq!(soft_at(synthetic(&RATIO_FLOOR, 1e300), 1e-10), f64::MAX);
}

#[test]
fn soft_limit_ratio_one_is_bitwise_identity() {
    for (id, limit) in [
        ("cyclomatic", 8.3),
        ("cyclomatic", 0.0),
        ("mi.original", -50.0),
        ("mi.original", 20.1),
        ("mi.visual_studio", 100.0),
    ] {
        let hard = th(id, limit);
        let soft = hard.soft_limit(1.0).expect("ratio ok");
        assert_eq!(soft.limit().to_bits(), limit.to_bits(), "{id}={limit}");
        assert_eq!(soft, hard);
    }
}

#[test]
fn soft_limit_rejects_bad_ratios() {
    let t = th("cyclomatic", 15.0);
    for ratio in [0.0, -0.5, 1.5, f64::NAN, f64::INFINITY] {
        match t.soft_limit(ratio) {
            Err(LimitError::BadRatio { ratio: got }) => {
                assert_eq!(got.to_bits(), ratio.to_bits());
            }
            other => panic!("ratio {ratio}: expected BadRatio, got {other:?}"),
        }
    }
    assert_eq!(
        LimitError::BadRatio { ratio: 1.5 }.to_string(),
        "soft ratio must be in (0, 1]; got 1.5"
    );
}

#[test]
fn new_rejects_unknown_non_finite_and_negative_ratio_limits() {
    assert_eq!(
        Threshold::check("not.a.metric", 1.0),
        Err(LimitError::UnknownMetric {
            id: "not.a.metric".to_owned()
        })
    );
    for limit in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for id in ["cyclomatic", "mi.original"] {
            assert!(
                matches!(
                    Threshold::check(id, limit),
                    Err(LimitError::NotFinite { id: got, .. }) if got == id
                ),
                "{id}={limit}"
            );
        }
    }
    let err = Threshold::check("cyclomatic", -1.0).expect_err("negative count limit");
    assert_eq!(
        err,
        LimitError::Negative {
            id: "cyclomatic",
            limit: -1.0
        }
    );
    assert_eq!(
        err.to_string(),
        "limit for \"cyclomatic\" must be a finite non-negative number; got -1"
    );
}

#[test]
fn limit_error_messages() {
    let msg = |id: &str, limit: f64| {
        Threshold::check(id, limit)
            .expect_err("rejected")
            .to_string()
    };
    assert_eq!(msg("not.a.metric", 1.0), "unknown metric \"not.a.metric\"");
    assert_eq!(
        msg("cyclomatic", f64::NAN),
        "limit for \"cyclomatic\" must be a finite non-negative number; got NaN"
    );
    assert_eq!(
        msg("mi.original", f64::INFINITY),
        "limit for \"mi.original\" must be a finite number; got inf"
    );
    let err: &dyn std::error::Error = &LimitError::BadRatio { ratio: 0.0 };
    assert!(err.source().is_none());
}

#[test]
fn new_accepts_and_warns_per_domain() {
    let ok = |id: &str, limit: f64| {
        let checked = Threshold::check(id, limit).expect("accepted");
        assert_eq!(checked.threshold.id(), id);
        assert_eq!(checked.threshold.limit().to_bits(), limit.to_bits());
        assert!(std::ptr::eq(
            checked.threshold.info(),
            lookup(id).expect("known")
        ));
        checked.warning
    };
    assert_eq!(ok("cyclomatic", 0.0), None);
    assert_eq!(ok("cyclomatic", 15.0), None);
    // `-0.0` is accepted as the zero it compares equal to, and stored as
    // `+0.0` so it neither renders as `-0` nor divides to `-inf`.
    for id in ["cyclomatic", "mi.original"] {
        let t = Threshold::check(id, -0.0).expect("signed zero").threshold;
        assert_eq!(t.limit().to_bits(), 0.0_f64.to_bits(), "{id}");
        assert_eq!(t, th(id, 0.0), "{id}");
    }
    assert_eq!(ok("mi.original", -50.0), None);
    assert_eq!(ok("mi.sei", -1e6), None);
    assert_eq!(
        ok("mi.visual_studio", 100.0),
        None,
        "a floor at the top fires on everything but is legal"
    );
    assert_eq!(ok("mi.visual_studio", 0.5), None);
    for limit in [-1.0, 101.0] {
        assert_eq!(
            ok("mi.visual_studio", limit),
            Some(LimitWarning::OutOfDomain {
                id: "mi.visual_studio",
                limit,
                min: 0.0,
                max: 100.0
            }),
            "mi.visual_studio={limit}"
        );
    }
    assert_eq!(
        ok("mi.visual_studio", 0.0),
        Some(LimitWarning::CannotFire {
            id: "mi.visual_studio",
            limit: 0.0
        })
    );
}

#[test]
fn limit_warning_messages() {
    let warn = |id: &str, limit: f64| {
        Threshold::check(id, limit)
            .expect("accepted")
            .warning
            .expect("warned")
            .to_string()
    };
    assert_eq!(
        warn("mi.visual_studio", 0.0),
        "limit mi.visual_studio=0 cannot fire: 0 is the metric's floor; gate mi.original instead"
    );
    assert_eq!(
        warn("mi.visual_studio", -1.0),
        "limit mi.visual_studio=-1 is outside the metric's range [0, 100]; it can never fire"
    );
    assert_eq!(
        warn("mi.visual_studio", 101.0),
        "limit mi.visual_studio=101 is outside the metric's range [0, 100]; it fires on every value"
    );
    // An id the catalog does not know (there is no interval ceiling to
    // use) takes the higher-is-worse wording, and an id with no
    // unbounded sibling names none.
    let ceiling = LimitWarning::CannotFire {
        id: "synthetic.interval_ceiling",
        limit: 100.0,
    };
    assert_eq!(
        ceiling.to_string(),
        "limit synthetic.interval_ceiling=100 cannot fire: 100 is the metric's ceiling"
    );
    let below = LimitWarning::OutOfDomain {
        id: "synthetic.interval_ceiling",
        limit: -1.0,
        min: 0.0,
        max: 100.0,
    };
    assert!(below.to_string().ends_with("it fires on every value"));
}

#[test]
fn domain_warning_covers_a_ceiling() {
    let info = &INTERVAL_CEILING;
    assert_eq!(
        domain_warning(info, 100.0, 0.0, 100.0),
        Some(LimitWarning::CannotFire {
            id: info.id,
            limit: 100.0
        })
    );
    assert_eq!(domain_warning(info, 0.0, 0.0, 100.0), None);
}

#[test]
fn change_is_percent_for_ratio_and_points_for_interval() {
    let (cyc, mi) = (th("cyclomatic", 15.0), th("mi.original", 20.0));
    assert_eq!(cyc.change(10.0, 20.0), Change::Percent(100.0));
    assert_eq!(
        cyc.change(20.0, 10.0),
        Change::Percent(-50.0),
        "an improvement is negative"
    );
    // Reproducer R1 of #1621: a hundredth of a point, not +1069 %.
    match mi.change(0.001, -0.0097) {
        Change::Points(p) => assert!(approx_eq(p, 0.0107), "got {p}"),
        other => panic!("expected Points, got {other:?}"),
    }
    assert_eq!(mi.change(-10.0, -20.0), Change::Points(10.0));
    assert_eq!(
        mi.change(0.0, -12.0),
        Change::Points(12.0),
        "a zero record is not special on an interval scale"
    );
    assert_eq!(mi.change(-12.0, 0.0), Change::Points(-12.0));
    assert_eq!(cyc.change(0.0, 5.0), Change::FromZero);
    assert_eq!(cyc.change(5.0, f64::NAN), Change::NaN);
    assert_eq!(th("mi.sei", 20.0).change(5.0, f64::NAN), Change::NaN);
    // The percentage is of the record's magnitude, so its sign is the
    // direction's alone. No count is negative; the arithmetic is total.
    assert_eq!(cyc.change(-10.0, -5.0), Change::Percent(50.0));
    // A ratio floor drops by half: +50 % worse.
    assert_eq!(
        synthetic(&RATIO_FLOOR, 1.0).change(20.0, 10.0),
        Change::Percent(50.0)
    );
}

#[test]
fn drift_is_relative_for_ratio_and_in_units_for_interval() {
    let cyc = th("cyclomatic", 15.0);
    assert_eq!(cyc.drift(10.0, 20.0), 0.5);
    assert_eq!(cyc.drift(20.0, 10.0), 0.5);
    assert_eq!(cyc.drift(0.0, 0.0), 0.0);
    assert_eq!(cyc.drift(7.0, 7.0), 0.0);
    assert_eq!(cyc.drift(0.0, 4.0), 1.0);

    let d = th("mi.sei", 20.0).drift(0.001, -0.0097);
    assert!(approx_eq(d, 0.0107 / 17.1), "got {d}");
    assert!(d < 0.001, "a hundredth of a point stays small: {d}");

    let d = th("mi.original", 20.0).drift(-5.0, 5.0);
    assert!(approx_eq(d, 10.0 / 17.1), "got {d}");
    let d = th("mi.visual_studio", 20.0).drift(30.0, 15.0);
    assert!(approx_eq(d, 1.5), "got {d}");
}

#[test]
fn ratio_floor_severity_inverts_the_ratio() {
    let floor = synthetic(&RATIO_FLOOR, 20.0);
    assert_eq!(floor.severity(10.0), 2.0);
    assert_eq!(floor.severity(0.0), f64::INFINITY);
    // `20 / -0.0` and `20 / -1` are negative: the guard, not the
    // division, is what saturates a non-positive breach.
    assert_eq!(floor.severity(-0.0), f64::INFINITY);
    assert_eq!(floor.severity(-1.0), f64::INFINITY);
    assert!(floor.breaches(19.0));
    assert_eq!(floor.excess(15.0), 5.0);
}

#[test]
fn threshold_equality_compares_id_and_limit() {
    assert_eq!(th("cyclomatic", 15.0), th("cyclomatic", 15.0));
    assert_ne!(th("cyclomatic", 15.0), th("cognitive", 15.0));
    assert_ne!(th("cyclomatic", 15.0), th("cyclomatic", 16.0));
}

#[test]
fn scale_free_function_matches_the_row() {
    assert_eq!(scale("cyclomatic"), Some(Scale::Ratio));
    assert!(matches!(
        scale("mi.visual_studio"),
        Some(Scale::Interval { min, max, unit }) if min == 0.0 && max == 100.0 && unit == 10.0
    ));
    assert_eq!(scale("not.a.metric"), None);
}
