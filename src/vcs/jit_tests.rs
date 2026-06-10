//! Unit tests for the pure JIT score function, on synthetic feature
//! vectors (no repository needed). Acceptance criterion #331: size,
//! diffusion, and file-prior contributions are exercised here at the
//! formula level; the end-to-end repo path is covered in
//! `tests/vcs_jit.rs`.

// Exact-equality on f64 is intentional: the compared values are exact
// literals (0.0) produced by the formula's own floor / zero terms.
#![allow(clippy::float_cmp)]

use super::*;

/// A zero feature vector and a non-fix, non-revert purpose.
fn zero() -> (JitFeatures, JitPurpose) {
    (JitFeatures::default(), JitPurpose::default())
}

#[test]
fn version_constants_pinned() {
    // A bump here is a deliberate formula / shape change; this pins the
    // current values so an accidental edit fails loudly.
    assert_eq!(JIT_SCORE_VERSION, 1);
    // Schema 3: renamed score keys `score` → `risk_score` and
    // `partial_score` → `partial_risk_score` (#591). (Schema 2 added the
    // `source` discriminator, #642.)
    assert_eq!(JIT_SCHEMA_VERSION, 3);
}

#[test]
fn empty_change_scores_zero() {
    let (features, purpose) = zero();
    let (total, contributions) = score(&features, purpose);
    assert_eq!(total, 0.0);
    assert_eq!(contributions, JitContributions::default());
}

#[test]
fn contributions_sum_to_score_when_non_negative() {
    let mut features = JitFeatures::default();
    features.size.lines_added = 120;
    features.size.lines_deleted = 30;
    features.size.files_touched = 4;
    features.diffusion.subsystems = 3;
    let purpose = JitPurpose {
        is_fix: true,
        ..JitPurpose::default()
    };
    let (total, c) = score(&features, purpose);
    let sum = c.size + c.diffusion + c.history + c.purpose + c.experience;
    // No experience here, so the sum is positive and equals the score
    // exactly (the floor is inactive).
    assert!((total - sum).abs() < 1e-12, "score {total} != sum {sum}");
}

#[test]
fn size_is_monotonic_in_churn() {
    let small = JitFeatures {
        size: JitSize {
            lines_added: 10,
            lines_deleted: 0,
            files_touched: 1,
            hunks: 1,
        },
        ..JitFeatures::default()
    };
    let big = JitFeatures {
        size: JitSize {
            lines_added: 1000,
            lines_deleted: 200,
            files_touched: 1,
            hunks: 1,
        },
        ..JitFeatures::default()
    };
    let purpose = JitPurpose::default();
    let (small_total, small_c) = score(&small, purpose);
    let (big_total, big_c) = score(&big, purpose);
    assert!(
        big_c.size > small_c.size,
        "more churn must raise the size term"
    );
    assert!(big_total > small_total);
}

#[test]
fn diffusion_is_monotonic_in_spread() {
    let focused = JitFeatures {
        diffusion: JitDiffusion {
            subsystems: 1,
            directories: 1,
            entropy: 0.0,
        },
        ..JitFeatures::default()
    };
    let scattered = JitFeatures {
        diffusion: JitDiffusion {
            subsystems: 5,
            directories: 9,
            entropy: 2.5,
        },
        ..JitFeatures::default()
    };
    let purpose = JitPurpose::default();
    let (_, focused_c) = score(&focused, purpose);
    let (_, scattered_c) = score(&scattered, purpose);
    // A change confined to one subsystem and one directory contributes
    // zero spread (saturating_sub(1) on both, entropy 0).
    assert_eq!(focused_c.diffusion, 0.0);
    assert!(scattered_c.diffusion > 0.0);
}

#[test]
fn history_weights_security_double_bug() {
    let bug = JitFeatures {
        history: JitHistory {
            prior_bug_fix_commits: 2,
            ..JitHistory::default()
        },
        ..JitFeatures::default()
    };
    let security = JitFeatures {
        history: JitHistory {
            prior_security_fix_commits: 1,
            ..JitHistory::default()
        },
        ..JitFeatures::default()
    };
    let purpose = JitPurpose::default();
    let (_, bug_c) = score(&bug, purpose);
    let (_, sec_c) = score(&security, purpose);
    // 1 security fix carries weight 2, exactly matching 2 bug fixes, so
    // the two history contributions are identical.
    assert!(
        (bug_c.history - sec_c.history).abs() < 1e-12,
        "1 security fix should weigh as 2 bug fixes: {} vs {}",
        bug_c.history,
        sec_c.history
    );
}

#[test]
fn file_prior_risk_raises_history() {
    let baseline = JitFeatures::default();
    let risky = JitFeatures {
        history: JitHistory {
            file_risk_max: 12.0,
            file_risk_mean: 6.0,
            ..JitHistory::default()
        },
        ..JitFeatures::default()
    };
    let purpose = JitPurpose::default();
    let (_, base_c) = score(&baseline, purpose);
    let (_, risky_c) = score(&risky, purpose);
    assert!(
        risky_c.history > base_c.history,
        "a high touched-file risk prior must raise the history term"
    );
}

#[test]
fn experience_lowers_the_score() {
    // The signature Kamei direction: holding the change fixed, a more
    // experienced author yields a LOWER score. Verified by construction
    // (the experience contribution is negative).
    let change = JitFeatures {
        size: JitSize {
            lines_added: 200,
            lines_deleted: 50,
            files_touched: 3,
            hunks: 5,
        },
        ..JitFeatures::default()
    };
    let novice = change;
    let veteran = JitFeatures {
        experience: JitExperience {
            author_prior_commits: 800,
            author_recent_commits: 40,
        },
        ..change
    };
    let purpose = JitPurpose::default();
    let (novice_total, novice_c) = score(&novice, purpose);
    let (veteran_total, veteran_c) = score(&veteran, purpose);
    assert_eq!(novice_c.experience, 0.0);
    assert!(
        veteran_c.experience < 0.0,
        "experience term must be negative"
    );
    assert!(
        veteran_total < novice_total,
        "an experienced author must score lower for the same change"
    );
}

#[test]
fn purpose_fix_security_revert() {
    let features = JitFeatures {
        size: JitSize {
            lines_added: 100,
            lines_deleted: 100,
            files_touched: 2,
            hunks: 3,
        },
        ..JitFeatures::default()
    };
    let plain = score(&features, JitPurpose::default());
    let fix = score(
        &features,
        JitPurpose {
            is_fix: true,
            ..JitPurpose::default()
        },
    );
    let security = score(
        &features,
        JitPurpose {
            is_fix: true,
            is_security_fix: true,
            ..JitPurpose::default()
        },
    );
    let revert = score(
        &features,
        JitPurpose {
            is_revert: true,
            ..JitPurpose::default()
        },
    );
    assert!(fix.0 > plain.0, "a fix should score above a non-fix");
    assert!(
        security.0 > fix.0,
        "a security fix should score above a plain fix"
    );
    assert!(revert.0 < plain.0, "a revert should dampen the score");
}

#[test]
fn score_floors_at_zero() {
    // A tiny change by a very experienced author: the negative experience
    // term outweighs the change, but the score never goes below zero.
    let features = JitFeatures {
        size: JitSize {
            lines_added: 1,
            lines_deleted: 0,
            files_touched: 1,
            hunks: 1,
        },
        experience: JitExperience {
            author_prior_commits: 5000,
            author_recent_commits: 300,
        },
        ..JitFeatures::default()
    };
    let (total, c) = score(&features, JitPurpose::default());
    assert_eq!(total, 0.0, "score must floor at zero");
    // The raw (un-floored) sum is genuinely negative here, proving the
    // floor is what produced the 0.
    let raw = c.size + c.diffusion + c.history + c.purpose + c.experience;
    assert!(raw < 0.0, "expected a negative pre-floor sum, got {raw}");
}
