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
