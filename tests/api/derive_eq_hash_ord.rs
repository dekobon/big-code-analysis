//! Regression tests for issue #552: the compute-side metric value types
//! gained `PartialEq`, and small closed-domain enums gained `Hash` /
//! `Ord`.
//!
//! Two analyses of the *same* deterministic source run the identical
//! code path and produce bit-identical `f64` fields, so exact `==` on
//! the resulting `Stats` / `CodeMetrics` / `FuncSpace` is safe here —
//! this is not a cross-input float-equality assumption. A *different*
//! source must compare unequal.

use std::collections::{HashMap, HashSet};

use big_code_analysis::metric_catalog::Direction;
use big_code_analysis::{FuncSpace, LANG, MetricsOptions, Severity, Source, SpaceKind, analyze};

/// Analyze a Rust snippet via the public `analyze` entry point.
#[cfg(feature = "rust")]
fn analyze_rust(source: &str) -> FuncSpace {
    analyze(
        Source::new(LANG::Rust, source.as_bytes()).with_name(Some("eq.rs".to_string())),
        MetricsOptions::default(),
    )
    .expect("parser produced no FuncSpace for fixture")
}

#[cfg(feature = "rust")]
const SRC_A: &str = r#"fn classify(x: u8) -> &'static str {
    if x > 10 && x < 100 {
        "mid"
    } else if x >= 100 {
        "high"
    } else {
        "low"
    }
}
"#;

#[cfg(feature = "rust")]
const SRC_B: &str = r"fn noop() {}
";

#[cfg(feature = "rust")]
#[test]
fn cognitive_stats_partial_eq_same_and_different_source() {
    let a1 = analyze_rust(SRC_A).metrics.cognitive.clone();
    let a2 = analyze_rust(SRC_A).metrics.cognitive.clone();
    let b = analyze_rust(SRC_B).metrics.cognitive.clone();

    assert_eq!(a1, a2, "same source must yield equal cognitive Stats");
    assert_ne!(a1, b, "different source must yield unequal cognitive Stats");
}

#[cfg(feature = "rust")]
#[test]
fn halstead_stats_partial_eq_same_and_different_source() {
    let a1 = analyze_rust(SRC_A).metrics.halstead.clone();
    let a2 = analyze_rust(SRC_A).metrics.halstead.clone();
    let b = analyze_rust(SRC_B).metrics.halstead.clone();

    assert_eq!(a1, a2, "same source must yield equal halstead Stats");
    assert_ne!(a1, b, "different source must yield unequal halstead Stats");
}

#[cfg(feature = "rust")]
#[test]
fn loc_stats_partial_eq_same_and_different_source() {
    let a1 = analyze_rust(SRC_A).metrics.loc.clone();
    let a2 = analyze_rust(SRC_A).metrics.loc.clone();
    let b = analyze_rust(SRC_B).metrics.loc.clone();

    assert_eq!(a1, a2, "same source must yield equal loc Stats");
    assert_ne!(a1, b, "different source must yield unequal loc Stats");
}

#[cfg(feature = "rust")]
#[test]
fn code_metrics_and_func_space_partial_eq() {
    let a1 = analyze_rust(SRC_A);
    let a2 = analyze_rust(SRC_A);
    let b = analyze_rust(SRC_B);

    // CodeMetrics aggregates all 13 Stats plus the selected mask.
    assert_eq!(
        a1.metrics, a2.metrics,
        "same source must yield equal CodeMetrics"
    );
    assert_ne!(
        a1.metrics, b.metrics,
        "different source must yield unequal CodeMetrics"
    );

    // FuncSpace compares name, spans, kind, nested spaces, metrics, and
    // suppression markers structurally.
    assert_eq!(a1, a2, "same source must yield equal FuncSpace");
    assert_ne!(a1, b, "different source must yield unequal FuncSpace");
}

/// #1821: compute-side `loc::Stats ==` is same-source equality, not
/// equality of reported values. The walk seeds a private per-file hint
/// (`may_hold_bare_rows`, "could a bare `\`-continuation row exist")
/// into every `loc::Stats`, and the derived `PartialEq` reads it, so
/// two files whose rows all report the same can compare unequal when
/// one holds a trailing `\` the other lacks. Pinned here as the
/// decision taken in review rather than left incidental: hand-writing
/// `PartialEq` to drop the bit was declined because `==` was already
/// positional and walk-stateful before this field — `LineSet` compares
/// row identities, so moving a blank row inside a function makes two
/// spaces with identical counts `!=`, and #1533's `ConstraintReach` on
/// `cyclomatic::Stats` is never reset, so `requires (true)` and
/// `requires ( true)` already compare unequal at equal values. The wire
/// projection and `Display` are the cross-input comparison; `bca diff`
/// reads wire values.
///
/// The trailing `\` sits in a `//` comment, which Rust's `Loc` arm
/// reads as one comment row either way, so every reported value is the
/// same and only the hint differs. Discriminating power: with the field
/// excluded from `==` (a temporary manual `PartialEq` over the other
/// eight fields) the `assert_ne!` fails and nothing else in the crate's
/// suites moves; with the hint seeded unconditionally `true` it fails
/// beside the two `bare_row_scans` count tests in `loc::tests`.
#[cfg(feature = "rust")]
#[test]
fn loc_stats_partial_eq_sees_the_bare_row_hint_the_wire_form_omits() {
    const WITHOUT: &str = "// note\nfn f() {}\n";
    const WITH: &str = "// note \\\nfn f() {}\n";
    let without = analyze_rust(WITHOUT);
    let with = analyze_rust(WITH);

    assert_eq!(
        without.to_wire().metrics.loc,
        with.to_wire().metrics.loc,
        "the wire `loc` reports the same rows for both"
    );
    assert_eq!(
        without.metrics.loc.to_string(),
        with.metrics.loc.to_string(),
        "`Display` renders the same rows for both"
    );
    assert_ne!(
        without.metrics.loc, with.metrics.loc,
        "compute-side `loc::Stats` carries the per-file bare-row hint, which differs"
    );
}

#[test]
fn severity_orders_error_above_warning() {
    assert!(
        Severity::Error > Severity::Warning,
        "ordered scale: Error must rank above Warning"
    );
    assert_eq!(
        Severity::Warning.max(Severity::Error),
        Severity::Error,
        "max of the scale is the most severe tier"
    );
}

#[test]
fn severity_usable_as_hash_key() {
    let mut counts: HashMap<Severity, u32> = HashMap::new();
    *counts.entry(Severity::Warning).or_default() += 2;
    *counts.entry(Severity::Error).or_default() += 1;

    assert_eq!(counts.get(&Severity::Warning), Some(&2));
    assert_eq!(counts.get(&Severity::Error), Some(&1));

    let set: HashSet<Severity> = [Severity::Warning, Severity::Error].into_iter().collect();
    assert!(set.contains(&Severity::Error));
    assert!(set.contains(&Severity::Warning));
}

#[test]
fn space_kind_usable_as_hash_key() {
    let mut seen: HashSet<SpaceKind> = HashSet::new();
    seen.insert(SpaceKind::Function);
    seen.insert(SpaceKind::Unit);

    assert!(seen.contains(&SpaceKind::Function));
    assert!(seen.contains(&SpaceKind::Unit));
    assert!(!seen.contains(&SpaceKind::Class));
}

#[test]
fn direction_usable_as_hash_key() {
    let mut by_direction: HashMap<Direction, &str> = HashMap::new();
    by_direction.insert(Direction::HigherIsWorse, "cyclomatic");
    by_direction.insert(Direction::LowerIsWorse, "mi");

    assert_eq!(
        by_direction.get(&Direction::HigherIsWorse),
        Some(&"cyclomatic")
    );
    assert_eq!(by_direction.get(&Direction::LowerIsWorse), Some(&"mi"));
}
