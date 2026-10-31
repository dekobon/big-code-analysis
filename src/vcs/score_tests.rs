// The percentile no-op test compares an exactly-representable f64
// literal (42.0); `float_cmp` is a false positive there.
#![allow(clippy::float_cmp)]

use super::*;

/// A neutral baseline input; tests tweak one axis and assert the score
/// moves in the expected direction. Comparative only — never asserts an
/// exact float (the score is ordinal).
fn baseline() -> ScoreInput {
    ScoreInput {
        churn_recent: 50,
        churn_long: 200,
        commits_recent: 3,
        commits_long: 10,
        authors_long: 3,
        ownership_top_share: 0.8,
        bug_fix_commits: 1,
        security_fix_commits: 0,
        sloc: 300,
        age_days: 200,
        recent_window_days: 90,
        change_entropy_recent: 1.0,
        cochange_entropy_recent: 1.0,
    }
}

#[test]
fn version_is_two() {
    // A single RISK_SCORE_VERSION versions BOTH the weighted formula and
    // the percentile blend; finalize stamps it in either mode (stats.rs)
    // and the cache keys reuse on it. A bump to either formula moves this.
    assert_eq!(RISK_SCORE_VERSION, 2);
}

#[test]
fn more_recent_churn_scores_higher() {
    let low = weighted(&baseline());
    let high = weighted(&ScoreInput {
        churn_recent: 5_000,
        ..baseline()
    });
    assert!(
        high > low,
        "high recent churn should outrank low: {high} vs {low}"
    );
}

#[test]
fn diluted_ownership_scores_higher_than_concentrated() {
    let concentrated = weighted(&ScoreInput {
        ownership_top_share: 1.0,
        ..baseline()
    });
    let diluted = weighted(&ScoreInput {
        ownership_top_share: 0.2,
        ..baseline()
    });
    assert!(diluted > concentrated, "{diluted} vs {concentrated}");
}

#[test]
fn more_authors_scores_higher() {
    let few = weighted(&ScoreInput {
        authors_long: 2,
        ..baseline()
    });
    let many = weighted(&ScoreInput {
        authors_long: 12,
        ..baseline()
    });
    assert!(many > few, "{many} vs {few}");
}

#[test]
fn dev_count_thresholds_bump_the_score() {
    // Ownership pinned to 1.0 zeroes the dilution term so only the
    // author count varies.
    let at = |authors_long| {
        weighted(&ScoreInput {
            authors_long,
            ownership_top_share: 1.0,
            ..baseline()
        })
    };
    // Crossing the 6- and 9-developer thresholds applies a
    // multiplicative bump (0.15, then a further jump to 0.35) that
    // dwarfs the smooth ln(author) growth: each *crossing* step lifts
    // the score by far more than an adjacent *within-band* step. The
    // 1.10 ratios sit above the bump (~1.15×/~1.17×) and well above
    // smooth growth (~1.005×), so this fails if MID_DEV_BONUS /
    // HIGH_DEV_BONUS are removed — unlike a bare `six > five` monotonic
    // check, which the strictly-increasing author factor passes anyway.
    assert!(
        at(6) > at(5) * 1.10,
        "6-dev crossing: {} vs {}",
        at(6),
        at(5)
    );
    assert!(
        at(9) > at(8) * 1.10,
        "9-dev crossing: {} vs {}",
        at(9),
        at(8)
    );
    // A within-band step (7→8, both in the mid band) does NOT jump by
    // that ratio — this is what distinguishes the categorical bump from
    // monotone author growth.
    assert!(at(8) < at(7) * 1.10, "within-band step is smooth");
}

#[test]
fn recent_churn_outweighs_long_churn() {
    // Route the *same* churn magnitude exclusively through the recent vs
    // the long window (mirroring `change_entropy_outweighs_cochange_entropy`),
    // so the comparison turns purely on the relative coefficients: recent is
    // weighted 0.30, long only 0.05. Holding `churn_long` constant across
    // both inputs (the prior construction) cancelled the long term and made
    // the test pass for any positive recent coefficient — it did not pin the
    // ordering. With exclusive routing, the difference is
    // (0.30 − 0.05)·ln1p(1000)·multiplier > 0 as written, but a formula that
    // swapped the two coefficients flips it to (0.05 − 0.30)·… < 0 and fails.
    let recent_heavy = weighted(&ScoreInput {
        churn_recent: 1_000,
        churn_long: 0,
        ..baseline()
    });
    let long_heavy = weighted(&ScoreInput {
        churn_recent: 0,
        churn_long: 1_000,
        ..baseline()
    });
    assert!(
        recent_heavy > long_heavy,
        "recent churn weighted higher: {recent_heavy} vs {long_heavy}"
    );
}

#[test]
fn security_fixes_weigh_double_bug_fixes() {
    let bugs = weighted(&ScoreInput {
        bug_fix_commits: 4,
        security_fix_commits: 0,
        ..baseline()
    });
    let security = weighted(&ScoreInput {
        bug_fix_commits: 0,
        security_fix_commits: 4,
        ..baseline()
    });
    assert!(
        security > bugs,
        "security fixes double-weighted: {security} vs {bugs}"
    );
}

#[test]
fn higher_change_entropy_scores_higher() {
    // A file caught up in scattered, distributed changes (Hassan) outranks
    // an otherwise-identical file whose changes were focused.
    let focused = weighted(&ScoreInput {
        change_entropy_recent: 0.0,
        ..baseline()
    });
    let scattered = weighted(&ScoreInput {
        change_entropy_recent: 4.0,
        ..baseline()
    });
    assert!(scattered > focused, "{scattered} vs {focused}");
}

#[test]
fn higher_cochange_entropy_scores_higher() {
    // A file whose changes ripple across many different partners outranks
    // one that always co-changes with the same neighbour.
    let narrow = weighted(&ScoreInput {
        cochange_entropy_recent: 0.0,
        ..baseline()
    });
    let wide = weighted(&ScoreInput {
        cochange_entropy_recent: 4.0,
        ..baseline()
    });
    assert!(wide > narrow, "{wide} vs {narrow}");
}

#[test]
fn change_entropy_outweighs_cochange_entropy() {
    // Pins the v2 weight ordering (0.10 vs 0.05): the same entropy
    // magnitude routed through change entropy must lift the score more
    // than through co-change entropy. A formula that swapped or equalised
    // the two coefficients fails here.
    let change_heavy = weighted(&ScoreInput {
        change_entropy_recent: 3.0,
        cochange_entropy_recent: 0.0,
        ..baseline()
    });
    let cochange_heavy = weighted(&ScoreInput {
        change_entropy_recent: 0.0,
        cochange_entropy_recent: 3.0,
        ..baseline()
    });
    assert!(
        change_heavy > cochange_heavy,
        "change entropy weighted higher: {change_heavy} vs {cochange_heavy}"
    );
}

#[test]
fn negative_entropy_is_clamped_not_propagated() {
    // Defensive: a (impossible) negative entropy must not drag the score
    // below the zero-entropy baseline.
    let zero = weighted(&ScoreInput {
        change_entropy_recent: 0.0,
        cochange_entropy_recent: 0.0,
        ..baseline()
    });
    let negative = weighted(&ScoreInput {
        change_entropy_recent: -5.0,
        cochange_entropy_recent: -5.0,
        ..baseline()
    });
    assert!((zero - negative).abs() < 1e-12, "{zero} vs {negative}");
}

#[test]
fn new_file_bump_applies_below_recent_window() {
    let old = weighted(&ScoreInput {
        age_days: 200,
        ..baseline()
    });
    let new = weighted(&ScoreInput {
        age_days: 10,
        ..baseline()
    });
    assert!(
        new > old,
        "a new file gets the new-file bump: {new} vs {old}"
    );
}

#[test]
fn percentile_ranks_busiest_file_highest() {
    let mut stats = vec![
        Stats {
            churn_recent: 10,
            commits_recent: 1,
            commits_long: 1,
            authors_long: 1,
            ..Stats::default()
        },
        Stats {
            churn_recent: 100,
            commits_recent: 5,
            commits_long: 8,
            authors_long: 4,
            ..Stats::default()
        },
        Stats {
            churn_recent: 1_000,
            commits_recent: 20,
            commits_long: 40,
            authors_long: 10,
            ..Stats::default()
        },
    ];
    apply_percentile(&mut stats);
    assert!(stats[2].risk_score > stats[1].risk_score);
    assert!(stats[1].risk_score > stats[0].risk_score);
    // Percentile scores are bounded to [0, 100].
    for s in &stats {
        assert!((0.0..=100.0).contains(&s.risk_score));
    }
}

#[test]
fn percentile_blend_includes_entropy_signals() {
    // Two files identical on every v1 signal; the only difference is the
    // recent entropy pair. The percentile blend must therefore rank the
    // higher-entropy file strictly above the other — proving the v2
    // extractors are load-bearing, not decorative.
    let mut stats = vec![
        Stats {
            commits_long: 5,
            churn_long: 100,
            change_entropy_recent: 0.0,
            cochange_entropy_recent: 0.0,
            ..Stats::default()
        },
        Stats {
            commits_long: 5,
            churn_long: 100,
            change_entropy_recent: 2.5,
            cochange_entropy_recent: 2.5,
            ..Stats::default()
        },
    ];
    apply_percentile(&mut stats);
    assert!(
        stats[1].risk_score > stats[0].risk_score,
        "higher-entropy file should rank up: {} vs {}",
        stats[1].risk_score,
        stats[0].risk_score
    );
}

#[test]
fn percentile_is_a_noop_below_two_files() {
    let mut one = vec![Stats {
        risk_score: 42.0,
        ..Stats::default()
    }];
    apply_percentile(&mut one);
    assert_eq!(one[0].risk_score, 42.0, "single-file set is left untouched");
}

// ---------------------------------------------------------------------
// #1267: the O(n log n) mid-rank must be bit-identical to the pairwise
// O(n²) form it replaced. The reference below is that pre-#1267 loop,
// frozen verbatim — including its tie predicates, its `n - 1`
// denominator over the whole population, and its clamp — so the tests
// compare against the formula `RISK_SCORE_VERSION` 2 was stamped on,
// not against a re-derivation.
// ---------------------------------------------------------------------

/// The tie tolerance `RISK_SCORE_VERSION` 2 was stamped on, frozen here
/// rather than imported: if production's `TIE_TOLERANCE` ever moves, the
/// oracle must keep describing v2 so the differential rows *fail* and
/// force the version bump, instead of moving in step and hiding it.
const V2_TIE: f64 = 1e-9;

#[test]
fn production_tolerance_is_the_v2_tolerance() {
    assert_eq!(TIE_TOLERANCE.to_bits(), V2_TIE.to_bits());
}

/// Pre-#1267 mid-rank kernel: for each value, count the population
/// strictly below `v - V2_TIE` and the population within the open
/// band `(v - V2_TIE, v + V2_TIE)`, both by scanning the
/// whole vector, and return the clamped mid-rank in `[0, 1]`.
#[allow(clippy::cast_precision_loss)]
fn pairwise_mid_ranks(values: &[f64]) -> Vec<f64> {
    let n = values.len();
    values
        .iter()
        .map(|&v| {
            let less = values.iter().filter(|&&o| o < v - V2_TIE).count();
            let equal = values.iter().filter(|&&o| (o - v).abs() < V2_TIE).count();
            let pct = (less as f64 + (equal as f64 - 1.0) / 2.0) / (n as f64 - 1.0);
            pct.clamp(0.0, 1.0)
        })
        .collect()
}

/// Pre-#1267 `apply_percentile`, frozen with its own copy of the v2
/// extractor list so a drift in production's list is caught rather than
/// shared. Deliberately duplicates production rather than calling into
/// it — this is the oracle.
#[allow(clippy::cast_precision_loss)]
fn pairwise_apply_percentile(stats: &mut [Stats]) {
    if stats.len() < 2 {
        return;
    }
    let extractors: [fn(&Stats) -> f64; 10] = [
        |s| s.churn_recent as f64,
        |s| s.churn_long as f64,
        |s| f64::from(s.commits_recent),
        |s| f64::from(s.commits_long),
        |s| f64::from(s.authors_long),
        |s| 1.0 - s.ownership_top_share,
        |s| f64::from(s.bug_fix_commits),
        |s| f64::from(s.security_fix_commits),
        |s| s.change_entropy_recent.max(0.0),
        |s| s.cochange_entropy_recent.max(0.0),
    ];
    let n = stats.len();
    let mut blended = vec![0.0_f64; n];
    for extract in extractors {
        let values: Vec<f64> = stats.iter().map(extract).collect();
        for (i, pct) in pairwise_mid_ranks(&values).into_iter().enumerate() {
            blended[i] += pct;
        }
    }
    let signal_count = extractors.len() as f64;
    for (s, blend) in stats.iter_mut().zip(blended) {
        s.risk_score = (blend / signal_count) * 100.0;
    }
}

fn bits(values: &[f64]) -> Vec<u64> {
    values.iter().map(|v| v.to_bits()).collect()
}

/// Asserts the production kernel and the frozen pairwise kernel agree
/// bit-for-bit on `values`, naming the case on failure.
#[track_caller]
fn assert_kernel_matches(label: &str, values: &[f64]) {
    let expected = pairwise_mid_ranks(values);
    let actual = mid_rank_percentiles(values);
    assert_eq!(
        bits(&actual),
        bits(&expected),
        "{label}: mid-ranks diverge from the pairwise reference\n values: {values:?}\n   new: {actual:?}\n   ref: {expected:?}"
    );
}

/// Asserts `apply_percentile` and the frozen pairwise oracle produce
/// bit-identical `risk_score`s and leave every other field untouched.
#[track_caller]
fn assert_blend_matches(label: &str, stats: &[Stats]) {
    let mut actual = stats.to_vec();
    let mut expected = stats.to_vec();
    apply_percentile(&mut actual);
    pairwise_apply_percentile(&mut expected);
    for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
        assert_eq!(
            a.risk_score.to_bits(),
            e.risk_score.to_bits(),
            "{label}: file {i} risk_score {} != reference {}",
            a.risk_score,
            e.risk_score
        );
        assert_fields_except_score_unchanged(label, i, a, &stats[i]);
    }
}

/// Asserts every field of `after` except `risk_score` is bit-identical
/// to `before`. The float fields are compared through `to_bits` — a
/// `PartialEq` compare fails an *unchanged* NaN (NaN != NaN) and a
/// `Debug` compare would let a canonicalised NaN sign or payload
/// through; the rest go through `PartialEq` with the floats masked.
#[track_caller]
fn assert_fields_except_score_unchanged(label: &str, i: usize, after: &Stats, before: &Stats) {
    let floats = |s: &Stats| {
        [
            s.ownership_top_share,
            s.burst,
            s.change_entropy_long,
            s.change_entropy_recent,
            s.cochange_entropy_long,
            s.cochange_entropy_recent,
            s.hotspot_score.unwrap_or(0.0),
        ]
        .map(f64::to_bits)
    };
    assert_eq!(
        floats(after),
        floats(before),
        "{label}: file {i} moved a float field other than risk_score"
    );
    let masked = |s: &Stats| Stats {
        risk_score: 0.0,
        ownership_top_share: 0.0,
        burst: 0.0,
        change_entropy_long: 0.0,
        change_entropy_recent: 0.0,
        cochange_entropy_long: 0.0,
        cochange_entropy_recent: 0.0,
        hotspot_score: s.hotspot_score.map(|_| 0.0),
        ..s.clone()
    };
    assert_eq!(
        masked(after),
        masked(before),
        "{label}: file {i} moved a non-float field other than risk_score"
    );
}

/// Deterministic xorshift64* generator, so the sweep is reproducible
/// without a `rand` dev-dependency.
struct XorShift64Star(u64);

impl XorShift64Star {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        let index = usize::try_from(self.next_u64() % items.len() as u64).expect("index < len");
        items[index]
    }
}

#[test]
fn mid_rank_kernel_exact_tolerance_boundary() {
    // `o - v == -V2_TIE` exactly: strictly outside the open tie
    // band, and NOT below `v - V2_TIE`, so the pairwise form
    // counts it as neither "less" nor "equal". Both ranks are 0 — a
    // quirk of the v2 formula, preserved on purpose. A rewrite that
    // closes the left endpoint (`o >= v - T`) or evaluates `v - T` on
    // the other side of the comparison reads this row as a tie.
    let values = [0.0, V2_TIE];
    assert_kernel_matches("exact boundary", &values);
    assert_eq!(mid_rank_percentiles(&values), vec![0.0, 0.0]);
}

#[test]
fn mid_rank_kernel_rounded_upper_boundary() {
    // `0.5 + 1e-9` rounds to a double whose difference from 0.5 is
    // ~9.9999997e-10 < V2_TIE, so the pairwise `abs(o - v) < T`
    // calls it a tie. A rewrite that compares `o < v + T` instead sees
    // `o == fl(v + T)` and rejects it. Both ranks must be 0.5.
    let values = [0.5, 0.5 + V2_TIE];
    assert_kernel_matches("rounded upper boundary", &values);
    assert_eq!(mid_rank_percentiles(&values), vec![0.5, 0.5]);
}

#[test]
fn mid_rank_kernel_chained_near_tie_is_not_transitive() {
    // a ≈ b, b ≈ c, a ≉ c. The middle value ties both neighbours; the
    // outer two do not tie each other. Expected pcts 0.25 / 0.5 / 0.75
    // — a transitive grouping would give 0.5 / 0.5 / 0.5.
    let values = [0.0, 0.6 * V2_TIE, 1.2 * V2_TIE];
    assert_kernel_matches("chained near-tie", &values);
    assert_eq!(mid_rank_percentiles(&values), vec![0.25, 0.5, 0.75]);
}

#[test]
fn mid_rank_kernel_handles_ties_and_order() {
    assert_kernel_matches("all equal", &[3.0; 7]);
    assert_kernel_matches("n=2 tied", &[1.0, 1.0]);
    assert_kernel_matches("n=2 distinct", &[1.0, 2.0]);
    assert_kernel_matches("integer ties", &[5.0, 1.0, 5.0, 3.0, 1.0, 5.0]);
    assert_kernel_matches("sorted", &[0.0, 1.0, 2.0, 3.0, 4.0]);
    assert_kernel_matches("reversed", &[4.0, 3.0, 2.0, 1.0, 0.0]);
    assert_kernel_matches("one ulp apart", &[1.0, 1.0_f64.next_up()]);
    assert_kernel_matches(
        "mixed magnitudes",
        &[0.0, 1e-12, 1.0, 1e3, 1e6, 1e6 + 1.0, 1e9, 2.5, 2.5],
    );
}

#[test]
fn mid_rank_kernel_handles_floating_point_extremes() {
    let tiny = f64::from_bits(1);
    assert_kernel_matches("signed zeros", &[-0.0, 0.0, -0.0, 0.0]);
    assert_kernel_matches("subnormals", &[0.0, tiny, f64::MIN_POSITIVE, -tiny]);
    assert_kernel_matches(
        "overflowing differences",
        &[f64::MAX, -f64::MAX, 0.0, f64::MAX],
    );
    assert_kernel_matches("infinities", &[f64::INFINITY, 0.0, f64::NEG_INFINITY, 1.0]);
    assert_kernel_matches("all +inf", &[f64::INFINITY, f64::INFINITY]);
    assert_kernel_matches("all -inf", &[f64::NEG_INFINITY, f64::NEG_INFINITY]);
}

#[test]
fn mid_rank_kernel_handles_nans_of_either_sign() {
    // NaN is never "less" and never "equal" to anything, itself included,
    // so a NaN element is invisible to every other element and ranks at
    // the clamp floor. The stats accumulator cannot produce one, but the
    // function is `pub` over `pub` fields and the dilution extractor
    // passes a NaN share through, so the kernel must not depend on where
    // `total_cmp` would otherwise sort a negative-signed NaN.
    let pos_nan = f64::NAN.abs();
    let neg_nan = -f64::NAN.abs();
    assert!(pos_nan.is_sign_positive() && neg_nan.is_sign_negative());
    assert_kernel_matches("+nan first", &[pos_nan, 0.0, 1.0, 1.0]);
    assert_kernel_matches("-nan first", &[neg_nan, 0.0, 1.0, 1.0]);
    assert_kernel_matches("-nan last", &[0.0, 1.0, 1.0, neg_nan]);
    assert_kernel_matches("both nans", &[neg_nan, 2.0, pos_nan, 2.0, 0.0]);
    assert_kernel_matches("all nan", &[neg_nan, pos_nan, neg_nan]);
    assert_kernel_matches(
        "nan and inf",
        &[neg_nan, f64::INFINITY, 0.0, f64::NEG_INFINITY],
    );
}

#[test]
fn mid_rank_kernel_seeded_sweep_matches_pairwise() {
    // Values are drawn from a base set plus offsets that land on exact
    // ties, 1-ULP ties, and both sides of the tolerance band, so the
    // sweep exercises tie groups and near-ties by construction rather
    // than by luck; the counters below make that non-vacuous. The
    // generator emits no NaN or infinity — those have named rows above.
    let bases = [0.0, 0.25, 0.5, 1.0, 2.0, 3.0, 7.0, 1e3, 1e6];
    let offsets = [
        0.0,
        0.0,
        0.0,
        f64::EPSILON,
        -f64::EPSILON,
        0.5 * V2_TIE,
        -0.5 * V2_TIE,
        V2_TIE,
        -V2_TIE,
        2.0 * V2_TIE,
        -2.0 * V2_TIE,
        1.0,
    ];
    let mut rng = XorShift64Star(0x1267_1267_1267_1267);
    let mut tie_groups = 0usize;
    let mut near_ties = 0usize;
    let mut vectors = 0usize;
    for n in [2usize, 3, 7, 50] {
        for _ in 0..100 {
            let values: Vec<f64> = (0..n)
                .map(|_| rng.pick(&bases) + rng.pick(&offsets))
                .collect();
            for (i, &a) in values.iter().enumerate() {
                for &b in &values[i + 1..] {
                    if a == b {
                        tie_groups += 1;
                    } else if (a - b).abs() < V2_TIE {
                        near_ties += 1;
                    }
                }
            }
            assert_kernel_matches(&format!("sweep n={n}"), &values);
            vectors += 1;
        }
    }
    assert_eq!(vectors, 400);
    assert!(
        tie_groups > 100,
        "sweep generated too few exact ties: {tie_groups}"
    );
    assert!(
        near_ties > 100,
        "sweep generated too few near-ties: {near_ties}"
    );
}

#[test]
fn mid_rank_kernel_arbitrary_bit_patterns_match_pairwise() {
    // Arbitrary 64-bit patterns: mostly ordinary floats across the full
    // exponent range, with NaN reachable (1 in 2048 draws). The named
    // rows above are what pin each float class deterministically; this
    // sweep is the check against inputs nobody hand-wrote.
    let mut rng = XorShift64Star(0xDEAD_BEEF_1267_0000);
    let mut saw_nan = false;
    for n in [2usize, 3, 5, 16] {
        for _ in 0..100 {
            let values: Vec<f64> = (0..n).map(|_| f64::from_bits(rng.next_u64())).collect();
            saw_nan |= values.iter().any(|v| v.is_nan());
            assert_kernel_matches(&format!("bits n={n}"), &values);
        }
    }
    assert!(saw_nan, "arbitrary-bit sweep never produced a NaN");
}

fn entropy_files(entropies: &[f64]) -> Vec<Stats> {
    entropies
        .iter()
        .map(|&e| Stats {
            commits_long: 4,
            churn_long: 40,
            ownership_top_share: 0.5,
            change_entropy_recent: e,
            ..Stats::default()
        })
        .collect()
}

#[test]
fn percentile_blend_matches_pairwise_on_mixed_signals() {
    let stats = vec![
        Stats {
            churn_recent: 10,
            churn_long: 200,
            commits_recent: 1,
            commits_long: 9,
            authors_long: 2,
            ownership_top_share: 0.75,
            bug_fix_commits: 1,
            security_fix_commits: 0,
            change_entropy_recent: 0.3,
            cochange_entropy_recent: 1.5,
            risk_score: 11.0,
            ..Stats::default()
        },
        Stats {
            churn_recent: 10,
            churn_long: 50,
            commits_recent: 5,
            commits_long: 9,
            authors_long: 4,
            ownership_top_share: 0.5,
            bug_fix_commits: 0,
            security_fix_commits: 2,
            change_entropy_recent: 2.0,
            cochange_entropy_recent: 1.5,
            risk_score: 22.0,
            ..Stats::default()
        },
        Stats {
            churn_recent: 1_000,
            churn_long: 1_000,
            commits_recent: 20,
            commits_long: 40,
            authors_long: 10,
            ownership_top_share: 0.2,
            bug_fix_commits: 3,
            security_fix_commits: 2,
            change_entropy_recent: 2.0,
            cochange_entropy_recent: 0.0,
            risk_score: 33.0,
            ..Stats::default()
        },
        Stats::default(),
    ];
    assert_blend_matches("mixed signals", &stats);
    let mut permuted = stats.clone();
    permuted.swap(0, 3);
    permuted.swap(1, 2);
    assert_blend_matches("permuted", &permuted);
}

#[test]
fn percentile_blend_only_moves_risk_score_and_keeps_association() {
    let mut stats = vec![
        Stats {
            commits_long: 1,
            risk_score: 5.0,
            ..Stats::default()
        },
        Stats {
            commits_long: 3,
            risk_score: 6.0,
            ..Stats::default()
        },
        Stats {
            commits_long: 2,
            risk_score: 7.0,
            ..Stats::default()
        },
    ];
    let before = stats.clone();
    apply_percentile(&mut stats);
    let mut permuted = vec![before[2].clone(), before[0].clone(), before[1].clone()];
    apply_percentile(&mut permuted);
    // Only `commits_long` differs, so each file's score is its
    // commits_long rank: (9 × 0.5 + rank) / 10 × 100. The top rank sums
    // ten terms in extractor order and lands one ULP above 55, which is
    // the bit pattern the pairwise loop produced too; pin it exactly
    // rather than with a tolerance, since bit-identity is the claim.
    let top = (0..10).fold(0.0_f64, |acc, i| acc + if i == 3 { 1.0 } else { 0.5 }) / 10.0 * 100.0;
    assert_eq!(stats[0].risk_score, 45.0);
    assert_eq!(stats[1].risk_score.to_bits(), top.to_bits());
    assert_eq!(stats[2].risk_score, 50.0);
    assert_eq!(permuted[0].risk_score, 50.0);
    assert_eq!(permuted[1].risk_score, 45.0);
    assert_eq!(permuted[2].risk_score.to_bits(), top.to_bits());
    for (i, (after, orig)) in stats.iter().zip(&before).enumerate() {
        assert_fields_except_score_unchanged("association", i, after, orig);
    }
}

#[test]
fn percentile_blend_preserves_every_seeded_field() {
    // Every field that the blend must not touch is seeded to a value
    // that differs from its default and from the other file's, so a
    // mutant that resets, clears or zeroes one cannot pass by landing
    // on the value the fixture started at.
    let seeded = |k: u32| Stats {
        vcs_schema_version: 7 + k,
        risk_score_version: 9 + k,
        long_window_days: 365 + k,
        recent_window_days: 90 + k,
        commits_long: 11 + k,
        commits_recent: 3 + k,
        churn_long: 1_000 + u64::from(k),
        churn_recent: 100 + u64::from(k),
        authors_long: 5 + k,
        authors_recent: 2 + k,
        ownership_top_share: 0.6 + f64::from(k) * 0.1,
        burst: 0.3 + f64::from(k) * 0.1,
        bug_fix_commits: 1 + k,
        security_fix_commits: 2 + k,
        revert_commits: 3 + k,
        age_days: 40 + k,
        last_modified_days: 4 + k,
        change_entropy_long: 1.5 + f64::from(k),
        change_entropy_recent: 0.5 + f64::from(k),
        cochange_entropy_long: 2.5 + f64::from(k),
        cochange_entropy_recent: 1.25 + f64::from(k),
        risk_score: 60.0 + f64::from(k),
        hotspot_score: Some(12.5 + f64::from(k)),
        author_ids: Some(vec![format!("author-{k}")]),
    };
    let stats = vec![seeded(0), seeded(1), seeded(2)];
    assert_blend_matches("seeded fields", &stats);
    let mut after = stats.clone();
    apply_percentile(&mut after);
    assert!(
        after
            .iter()
            .zip(&stats)
            .all(|(a, b)| a.risk_score != b.risk_score),
        "the blend must overwrite the seeded risk_score"
    );
}

#[test]
fn percentile_blend_boundary_rows_reach_the_final_score() {
    // The three kernel boundary cases, driven through the entropy
    // extractor (which passes its value through verbatim), with the
    // other nine signals tied at 0.5: final = (9 × 0.5 + pct) / 10 × 100.
    let exact = entropy_files(&[0.0, V2_TIE]);
    assert_blend_matches("exact boundary blend", &exact);
    let mut exact = exact;
    apply_percentile(&mut exact);
    assert_eq!(exact[0].risk_score, 45.0);
    assert_eq!(exact[1].risk_score, 45.0);

    let rounded = entropy_files(&[0.5, 0.5 + V2_TIE]);
    assert_blend_matches("rounded boundary blend", &rounded);
    let mut rounded = rounded;
    apply_percentile(&mut rounded);
    assert_eq!(rounded[0].risk_score, 50.0);
    assert_eq!(rounded[1].risk_score, 50.0);

    let chained = entropy_files(&[0.0, 0.6 * V2_TIE, 1.2 * V2_TIE]);
    assert_blend_matches("chained blend", &chained);
    let mut chained = chained;
    apply_percentile(&mut chained);
    assert_eq!(chained[0].risk_score, 47.5);
    assert_eq!(chained[1].risk_score, 50.0);
    assert_eq!(chained[2].risk_score, 52.5);
}

#[test]
fn percentile_blend_matches_pairwise_on_nonfinite_public_input() {
    // `Stats` fields are `pub`; a caller can hand in NaN or infinity.
    // The v2 behaviour for those is whatever the pairwise loop did —
    // not an error, not a panic — and must be preserved bit-for-bit.
    //
    // The entropy extractors `.max(0.0)` their input, which turns a NaN
    // into `0.0` before the kernel sees it (`f64::max` ignores NaN), so
    // only `+inf` survives that path. The dilution extractor `1 - share`
    // propagates a NaN share, so this is the path by which a NaN of
    // either sign can reach the kernel from a public `Stats`. Which sign
    // `1 - NaN` carries is not something Rust guarantees, so both signs
    // are fed in and neither is asserted on; the kernel-level
    // `mid_rank_kernel_handles_nans_of_either_sign` row is what pins the
    // sort's placement of each sign, with no arithmetic in between.
    let files = entropy_files(&[f64::NAN, 1.0, f64::INFINITY, 0.0]);
    assert_blend_matches("inf entropies, nan scrubbed", &files);
    let mut shares = entropy_files(&[1.0, 2.0, 3.0, 4.0]);
    shares[1].ownership_top_share = f64::NAN.abs();
    shares[2].ownership_top_share = f64::INFINITY;
    shares[3].ownership_top_share = -f64::NAN.abs();
    assert_blend_matches("nan and inf shares", &shares);
    let all_inf = entropy_files(&[f64::INFINITY, f64::INFINITY]);
    assert_blend_matches("all +inf entropies", &all_inf);
}

#[test]
fn percentile_blend_is_a_noop_on_empty_input() {
    let mut none: Vec<Stats> = vec![];
    apply_percentile(&mut none);
    assert_eq!(none.len(), 0);
}
