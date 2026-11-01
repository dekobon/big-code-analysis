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
    // the result-scaled sig-fig snap of the time, which then printed
    // 0.855001 (#1621 code review). Measured in true ulps of 0.855 — a
    // bit distance, since
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

// --- soft_limit_rounded (#373, #1621, #1648) ---
//
// The rounding, the snap and the hard-side clamp lived in the CLI's
// `scale_threshold` until #1648; the rows below are its tests, moved
// with the code, plus the synthetic direction/scale pairs the catalog
// has no row for.

/// [`Threshold::soft_limit_rounded`] for the limit `limit` of `id`,
/// unwrapped: the ratio is always valid here.
fn rounded(id: &str, limit: f64, ratio: f64) -> f64 {
    th(id, limit)
        .soft_limit_rounded(ratio)
        .expect("valid ratio")
        .limit()
}

/// `soft_limit_rounded` must trim the float-multiplication artifact so
/// the emitted limit is the readable value the retired Python helper
/// produced (`{:.6g}`). The canonical case is `nargs = 7`: `7 * 0.95`
/// is `6.6499999999999995` in IEEE-754, which must round to exactly
/// `6.65`.
#[test]
fn soft_limit_rounded_trims_float_artifact_to_six_sig_figs() {
    assert_eq!(rounded("nargs", 7.0, 0.95), 6.65);
}

/// The rounded threshold is still a threshold of the same metric, so a
/// caller can gate on it directly, and at `ratio == 1` it is the hard
/// threshold itself.
#[test]
fn soft_limit_rounded_keeps_the_metric() {
    let hard = th("mi.original", 20.0);
    let soft = hard.soft_limit_rounded(0.9).expect("ratio ok");
    assert_eq!(soft.id(), "mi.original");
    assert_eq!(soft.limit(), 21.71);
    assert!(soft.breaches(21.0), "the rounded floor gates");
    assert!(!hard.breaches(21.0));
    assert_eq!(hard.soft_limit_rounded(1.0).expect("ratio ok"), hard);
}

/// The direction of the scaling is the whole of #1166, so pin both
/// arithmetics in one test: a higher-is-worse limit is a ceiling and
/// must come *down*; a lower-is-worse `mi.*` limit is a floor and must
/// go *up*. Asserted together so a future "simplify" cannot invert both
/// and stay green — which is exactly what a single-direction test would
/// have allowed.
#[test]
fn soft_limit_rounded_raises_a_floor_and_lowers_a_ceiling() {
    // Ceiling: 15 * 0.9. Exact in decimal, and the float product is
    // exact too, so this is a bit-exact expectation.
    assert_eq!(rounded("cyclomatic", 15.0, 0.9), 13.5);
    // Floor: 20 + 0.1 × 17.1 (one Visual Studio band on the 171-point
    // scale), above the hard floor it derives from (#1621).
    assert_eq!(rounded("mi.original", 20.0, 0.9), 21.71);
    // The bug this pins: the floor must never land at or below its own
    // hard limit, which `20 * 0.9 == 18` did.
    assert!(rounded("mi.original", 20.0, 0.9) > 20.0);
}

/// An `mi.*` floor moves by `(1 - ratio)` of the metric's unit, not by
/// a factor (#1621): one Visual Studio band is 17.1 points of
/// `mi.original` / `mi.sei` and 10 of `mi.visual_studio`. Dividing a
/// negative floor would *loosen* it — `-50 / 0.95` is `-52.63`, below
/// the hard floor — so the negative case is the one this pins.
#[test]
fn soft_limit_rounded_shifts_an_interval_floor_by_its_unit() {
    assert_eq!(rounded("mi.original", 20.0, 0.95), 20.855);
    assert_eq!(rounded("mi.sei", 20.0, 0.95), 20.855);
    assert_eq!(rounded("mi.visual_studio", 20.0, 0.95), 20.5);
    let negative = rounded("mi.original", -50.0, 0.95);
    assert_eq!(negative, -49.145);
    assert!(
        negative > -50.0,
        "a soft floor must be stricter: {negative}"
    );
    // Clamped at the domain's top: a floor of 99.8 cannot rise past 100.
    assert_eq!(rounded("mi.visual_studio", 99.8, 0.95), 100.0);
}

/// The floor's last significant figure rounds **up**, never down: a
/// floor rounded down is a band that fires marginally late. Pins the
/// direction with a value whose 6-sig-fig nearest-rounding would go the
/// other way: `20 + 0.0001 × 17.1` is `20.00171`, which is `20.0017` to
/// nearest and `20.0018` up.
#[test]
fn soft_limit_rounded_floor_rounds_up_not_to_nearest() {
    let resolved = rounded("mi.original", 20.0, 0.9999);
    assert_eq!(resolved, 20.0018);
    assert!(
        resolved >= 20.0 + 0.0001 * 17.1,
        "a floor must not resolve below the exact value; got {resolved}"
    );
    // A value that *is* on the grid but carries float residue snaps to
    // it rather than being promoted a tick: `0.07 × 17.1 = 1.197`.
    assert_eq!(rounded("mi.original", 20.0, 0.93), 21.197);
}

/// The additive step's float residue is a larger share of a floor near
/// zero than of the operands it came from, and larger still when the
/// step cancels against a floor of the other sign: `-1 + 0.855` is
/// `-0.14499999999999957`, which a snap measured on the *result* read as
/// a genuine remainder and promoted to `-0.144999` (#1621 review). The
/// snap is measured on the operands, so every realistic floor and ratio
/// resolves to the decimal a reader expects.
#[test]
fn soft_limit_rounded_floor_snaps_to_the_decimal_near_zero() {
    for (limit, ratio, expected) in [
        (0.0, 0.95, 0.855),
        (-1.0, 0.95, -0.145),
        (1.0, 0.95, 1.855),
        (-0.5, 0.95, 0.355),
        (0.0, 0.9, 1.71),
        (-1.0, 0.9, 0.71),
        (0.0, 0.99, 0.171),
        (-1.0, 0.99, -0.829),
        (20.0, 0.95, 20.855),
        (-50.0, 0.95, -49.145),
        (-20.0, 0.95, -19.145),
        // The unit term of the snap's operand: without it this floor
        // resolves to -1.44699 (its residue read on its own scale).
        (-1.96, 0.97, -1.447),
        // A step that cancels the floor exactly is zero, not the ±1e-16
        // residue the sum carries.
        (-0.855, 0.95, 0.0),
        (-1.71, 0.9, 0.0),
        (-0.171, 0.99, 0.0),
    ] {
        assert_eq!(
            rounded("mi.original", limit, ratio),
            expected,
            "mi.original = {limit} at {ratio}"
        );
    }
    for (limit, ratio, expected) in [
        (0.0, 0.95, 0.5),
        (0.0, 0.9, 1.0),
        (20.0, 0.95, 20.5),
        (-0.3, 0.97, 0.0),
    ] {
        assert_eq!(
            rounded("mi.visual_studio", limit, ratio),
            expected,
            "mi.visual_studio = {limit} at {ratio}"
        );
    }
}

/// Rounding never crosses the hard limit. A floor far smaller than its
/// unit carries a genuine remainder inside the snap tolerance;
/// unclamped, it lands on the wrong side of the hard limit and the CLI's
/// `build_tiered` rejects a valid configuration as looser-than-hard. A
/// ceiling with more figures than the grid holds, scaled by a hair, can
/// sit within the snap tolerance of the tick *above* it and snap past
/// the hard limit. The `ratio == 1` rows never reach the rounding
/// (`soft_limit_rounded_ratio_one_is_identity`).
#[test]
fn soft_limit_rounded_never_crosses_the_hard_limit() {
    let tiny_floor = 0.000_010_000_000_01;
    assert!(rounded("mi.original", tiny_floor, 0.999_999_999_999_999_9) >= tiny_floor);
    assert!(rounded("mi.original", 2.821_610_071_901_938e-7, 1.0) >= 2.821_610_071_901_938e-7);
    // 9_999_999.999_999_998 · (1 − ε) is two ulps under the hard limit
    // and within the snap tolerance of 10_000_000 — above the ceiling
    // it was scaled from — so only the clamp keeps it under.
    let off_grid_ceiling = 9_999_999.999_999_998;
    assert_eq!(
        rounded("cyclomatic", off_grid_ceiling, 1.0 - f64::EPSILON),
        off_grid_ceiling
    );
    // 9_999_996 · (1 − ε) is 9_999_995.999…: too far from either tick
    // to snap, so it rounds down to 9_999_990 (nearest read it as
    // 10_000_000, the case the clamp was first added for).
    assert_eq!(
        rounded("cyclomatic", 9_999_996.0, 1.0 - f64::EPSILON),
        9_999_990.0
    );
    for (metric, ceiling) in [("halstead.effort", 123_456.7), ("cyclomatic", 6.666_666_6)] {
        assert_eq!(
            rounded(metric, ceiling, 1.0),
            ceiling,
            "{metric} at 1.0 is the identity"
        );
    }
}

/// Exact products must pass through untouched: `50_000 * 0.95` is
/// representable as `47_500.0`, so rounding must not perturb it. Pins
/// that the 6-sig-fig rounding preserves full precision for the
/// largest threshold in `bca.toml` (`halstead.effort`).
#[test]
fn soft_limit_rounded_preserves_large_exact_products() {
    assert_eq!(rounded("halstead.effort", 50_000.0, 0.95), 47_500.0);
}

/// A ceiling whose product has more figures than the grid holds rounds
/// **down**, never to nearest: `13801 · 0.95` is `13110.95`, which
/// nearest read as `13111`, and the strict `>` of `breaches` then let a
/// file of exactly 13111 lines through the soft tier the exact band
/// would have reported. The rows are the shapes a six-figure grid
/// cannot hold: a five-digit count with a two-figure ratio, a
/// four-digit count with a three-figure one, and a tiny ratio whose
/// product is so much smaller than its operand that the operand's ulps
/// once swallowed its genuine remainder and snapped it up to `8`.
#[test]
fn soft_limit_rounded_ceiling_never_exceeds_the_exact_product() {
    for (metric, limit, ratio, expected, first_breach) in [
        ("loc.sloc", 799_999_999.0, 1e-8, 7.999_99, 8.0),
        ("loc.sloc", 13_801.0, 0.95, 13_110.9, 13_111.0),
        ("loc.sloc", 10_201.0, 0.99, 10_098.9, 10_099.0),
        ("cyclomatic", 1_002.0, 0.999, 1_000.99, 1_001.0),
        ("cyclomatic", 1_489.0, 0.955, 1_421.99, 1_422.0),
        (
            "halstead.effort",
            1_234_567.0,
            0.95,
            1_172_830.0,
            1_172_839.0,
        ),
    ] {
        let soft = th(metric, limit)
            .soft_limit_rounded(ratio)
            .expect("ratio ok");
        assert_eq!(soft.limit(), expected, "{metric} = {limit} at {ratio}");
        assert!(
            soft.breaches(first_breach),
            "{first_breach} must breach the soft {metric} band {expected}"
        );
    }
}

/// Over every integer limit up to 200 000 and the ratios a band is
/// written with, the rounded soft ceiling never exceeds the decimal
/// product `limit · ratio`, and never falls more than one grid tick
/// under it; a product the grid holds is the product itself. Each
/// ratio is an integer fraction so the decimal product is computed
/// exactly, as an integer over a power of ten, rather than through the
/// float multiplication under test.
#[test]
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn soft_limit_rounded_ceiling_sweep_stays_under_the_product() {
    const RATIOS: [(u64, u64); 6] = [
        (95, 100),
        (99, 100),
        (9, 10),
        (999, 1_000),
        (955, 1_000),
        (5, 10),
    ];
    let hard = |limit: u64| th("loc.sloc", limit as f64);
    for limit in 1..=200_000_u64 {
        for (numerator, denominator) in RATIOS {
            let ratio = numerator as f64 / denominator as f64;
            let product = limit * numerator;
            // Correctly rounded, since both operands are exact.
            let exact = product as f64 / denominator as f64;
            let soft = hard(limit).soft_limit_rounded(ratio).expect("ratio ok");
            let resolved = soft.limit();
            assert!(
                resolved <= exact,
                "loc.sloc = {limit} at {ratio}: {resolved} is above the product {exact}"
            );
            let tick = 10f64.powi(exact.log10().floor() as i32 - (SOFT_LIMIT_SIG_FIGS - 1));
            assert!(
                exact - resolved < tick,
                "loc.sloc = {limit} at {ratio}: {resolved} is a whole tick under {exact}"
            );
            let first_breach = (product / denominator + 1) as f64;
            assert!(
                soft.breaches(first_breach),
                "loc.sloc = {limit} at {ratio}: {first_breach} escapes the band {resolved}"
            );
            let figures = product.to_string().trim_end_matches('0').len();
            if figures <= usize::try_from(SOFT_LIMIT_SIG_FIGS).expect("a positive count") {
                assert_eq!(
                    resolved, exact,
                    "loc.sloc = {limit} at {ratio} fits the grid"
                );
            }
        }
    }
}

/// `ratio == 1.0` is the documented no-op: every limit must survive
/// scaling byte-for-byte so `--headroom 1.0` is a true parity run with
/// the hard gate.
#[test]
fn soft_limit_rounded_ratio_one_is_identity() {
    // `8.3`'s double sits a hair above the decimal, so `8.3 * 1e5` is
    // `830000.0000000001`: a bare `ceil` in the floor direction once
    // promoted it to `8.30001`, a soft floor strictly *above* the hard
    // one on a run documented as parity. The grid snap absorbs that
    // residue now, and the identity short-circuit keeps every ratio-1
    // row from reaching the rounding at all; the rows further down with
    // more figures than the grid holds are the ones only the
    // short-circuit keeps.
    for &limit in &[0.0, 7.0, 8.3, 15.0, 300.0, 50_000.0] {
        for metric in ["cyclomatic", "mi.original"] {
            assert_eq!(
                rounded(metric, limit, 1.0).to_bits(),
                limit.to_bits(),
                "limit {limit} metric {metric}",
            );
        }
    }
    assert_eq!(rounded("mi.original", -50.0, 1.0), -50.0);
    // More figures than the six-figure grid holds, each rounding toward
    // the strict side — a ceiling down, a floor up — where the hard-side
    // clamp cannot restore it: only skipping the rounding keeps these.
    for (metric, limit) in [
        ("halstead.effort", 1_234_564.0),
        ("halstead.effort", 22_476.924),
        ("cyclomatic", 5_366.234),
        ("mi.original", 92.669_605),
        ("mi.original", -14.774_235_32),
    ] {
        assert_eq!(
            rounded(metric, limit, 1.0).to_bits(),
            limit.to_bits(),
            "limit {limit} metric {metric}"
        );
    }
    // A floor the domain clamp handed back unchanged did not move
    // either, whatever the ratio: out of domain above 100, with more
    // figures than the grid holds, so only the short-circuit keeps it.
    assert_eq!(
        rounded("mi.visual_studio", 100.123_456, 0.9).to_bits(),
        100.123_456_f64.to_bits()
    );
}

/// A configured limit of `0` ("no value permitted") must stay `0`
/// after scaling. `0 · ratio` is `0`, so the identity short-circuit
/// returns it before any rounding; the rounding's own guard against
/// `log10(0)` is reached by a ceiling the domain clamp takes to zero
/// (`soft_limit_rounded_covers_the_synthetic_direction_scale_pairs`)
/// and by `round_to_soft_grid(0.0)`.
#[test]
fn soft_limit_rounded_zero_limit_stays_zero() {
    assert_eq!(rounded("cyclomatic", 0.0, 0.5).to_bits(), 0.0_f64.to_bits());
}

/// A subnormal-range limit must not poison the result with `NaN`: the
/// sig-fig `factor` overflows to infinity for such inputs, so the
/// rounding hands the scaled value back unrounded. No real threshold is
/// this small, but `soft_limit_rounded` must stay total — a `NaN` would
/// later be rejected by the CLI's `ThresholdSet::build` with a confusing
/// error. Pinned to the scaled value, not merely to "finite": `f64::min`
/// ignores a NaN, so the hard-side clamp alone would turn an unguarded
/// NaN into the hard limit, a finite answer that is still the wrong one.
#[test]
fn soft_limit_rounded_subnormal_limit_stays_finite() {
    let limit = 1e-320;
    assert_eq!(rounded("cyclomatic", limit, 0.5), limit * 0.5);
}

/// An invalid ratio is `BadRatio`, never a silent scale.
#[test]
fn soft_limit_rounded_rejects_a_ratio_outside_the_band() {
    let err = th("cyclomatic", 10.0)
        .soft_limit_rounded(1.5)
        .expect_err("a ratio above 1 loosens the tier");
    assert_eq!(err, LimitError::BadRatio { ratio: 1.5 });
}

/// The direction/scale pairs the catalog has no row for. A ratio-scale
/// floor is a quotient, so its snap operand is the limit alone (no unit
/// term) and its last figure still goes up; an interval ceiling rounds
/// down like any ceiling and is held at or below the hard limit.
#[test]
fn soft_limit_rounded_covers_the_synthetic_direction_scale_pairs() {
    let rounded_synthetic = |info: &'static MetricInfo, limit: f64, ratio: f64| {
        synthetic(info, limit)
            .soft_limit_rounded(ratio)
            .expect("ratio ok")
            .limit()
    };
    // 7 / 0.95 is 7.368421052631579…: up to 7.36843, not 7.36842.
    assert_eq!(rounded_synthetic(&RATIO_FLOOR, 7.0, 0.95), 7.368_43);
    // 20 / 0.8 is exactly 25, on the grid, so no promotion.
    assert_eq!(rounded_synthetic(&RATIO_FLOOR, 20.0, 0.8), 25.0);
    assert!(rounded_synthetic(&RATIO_FLOOR, 7.0, 0.95) > 7.0);
    // `soft_limit` caps the quotient at `f64::MAX` to keep it finite;
    // rounding that *up* would overflow to infinity, so the cap stands.
    assert_eq!(rounded_synthetic(&RATIO_FLOOR, 1e300, 1e-10), f64::MAX);

    // 50 − 0.1 × 10 and 50 − 0.05 × 10, both exact.
    assert_eq!(rounded_synthetic(&INTERVAL_CEILING, 50.0, 0.9), 49.0);
    assert_eq!(rounded_synthetic(&INTERVAL_CEILING, 50.0, 0.95), 49.5);
    // Clamped at the domain minimum, then held under the hard limit.
    assert_eq!(rounded_synthetic(&INTERVAL_CEILING, 3.0, 0.5), 0.0);
    // Out of domain: the exact arithmetic already hands the hard limit
    // back, so the rounded form is the identity too.
    assert_eq!(
        rounded_synthetic(&INTERVAL_CEILING, -5.0, 0.9).to_bits(),
        (-5.0_f64).to_bits()
    );
}

/// The grid `soft_limit_rounded` resolves to, offered to a renderer for
/// the quantities it derives from a rounded limit.
#[test]
fn round_to_soft_grid_rounds_to_nearest_six_figures() {
    assert_eq!(Threshold::round_to_soft_grid(1.0 - 0.95), 0.05);
    assert_eq!(Threshold::round_to_soft_grid(20.855 - 20.0), 0.855);
    assert_eq!(Threshold::round_to_soft_grid(7.0 * 0.95), 6.65);
    assert_eq!(Threshold::round_to_soft_grid(1_234_567.0), 1_234_570.0);
    assert_eq!(Threshold::round_to_soft_grid(-1_234_564.0), -1_234_560.0);
    // Nearest, not toward a strict side: that direction is
    // `soft_limit_rounded`'s alone.
    assert_eq!(Threshold::round_to_soft_grid(20.00171), 20.0017);
    // The degenerate inputs pass through: zero, the non-finite values,
    // and a subnormal whose sig-fig factor would overflow.
    assert_eq!(
        Threshold::round_to_soft_grid(0.0).to_bits(),
        0.0_f64.to_bits()
    );
    assert!(Threshold::round_to_soft_grid(f64::NAN).is_nan());
    assert_eq!(Threshold::round_to_soft_grid(f64::INFINITY), f64::INFINITY);
    assert_eq!(
        Threshold::round_to_soft_grid(f64::NEG_INFINITY),
        f64::NEG_INFINITY
    );
    assert_eq!(Threshold::round_to_soft_grid(1e-320), 1e-320);
    // The top of the grid: `f64::MAX` rounds to the finite tick below it.
    assert!(Threshold::round_to_soft_grid(f64::MAX).is_finite());
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
