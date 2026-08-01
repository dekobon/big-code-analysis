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
fn analyze_rust(source: &str) -> FuncSpace {
    analyze(
        Source::new(LANG::Rust, source.as_bytes()).with_name(Some("eq.rs".to_string())),
        MetricsOptions::default(),
    )
    .expect("parser produced no FuncSpace for fixture")
}

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

const SRC_B: &str = r"fn noop() {}
";

#[test]
fn cognitive_stats_partial_eq_same_and_different_source() {
    let a1 = analyze_rust(SRC_A).metrics.cognitive.clone();
    let a2 = analyze_rust(SRC_A).metrics.cognitive.clone();
    let b = analyze_rust(SRC_B).metrics.cognitive.clone();

    assert_eq!(a1, a2, "same source must yield equal cognitive Stats");
    assert_ne!(a1, b, "different source must yield unequal cognitive Stats");
}

#[test]
fn halstead_stats_partial_eq_same_and_different_source() {
    let a1 = analyze_rust(SRC_A).metrics.halstead.clone();
    let a2 = analyze_rust(SRC_A).metrics.halstead.clone();
    let b = analyze_rust(SRC_B).metrics.halstead.clone();

    assert_eq!(a1, a2, "same source must yield equal halstead Stats");
    assert_ne!(a1, b, "different source must yield unequal halstead Stats");
}

#[test]
fn loc_stats_partial_eq_same_and_different_source() {
    let a1 = analyze_rust(SRC_A).metrics.loc.clone();
    let a2 = analyze_rust(SRC_A).metrics.loc.clone();
    let b = analyze_rust(SRC_B).metrics.loc.clone();

    assert_eq!(a1, a2, "same source must yield equal loc Stats");
    assert_ne!(a1, b, "different source must yield unequal loc Stats");
}

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
