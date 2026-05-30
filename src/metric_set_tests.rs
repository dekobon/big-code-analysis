// Sibling-file unit tests for `Metric` / `MetricSet`, wired in via
// `#[path = "metric_set_tests.rs"] mod tests;` so the production
// `metric_set.rs` stays under the `bca check` per-file metric caps.
// The `./**/*_tests.rs` rule in `.bcaignore` keeps this file out of
// the self-scan walker.

use super::*;

#[test]
fn empty_contains_nothing() {
    let set = MetricSet::empty();
    assert!(!set.contains(Metric::Loc));
    assert!(!set.contains(Metric::Halstead));
    assert!(!set.contains(Metric::Mi));
}

#[test]
fn all_contains_every_variant() {
    let set = MetricSet::all();
    for m in [
        Metric::Cognitive,
        Metric::Cyclomatic,
        Metric::Halstead,
        Metric::Loc,
        Metric::Nom,
        Metric::Tokens,
        Metric::NArgs,
        Metric::Exit,
        Metric::Abc,
        Metric::Npm,
        Metric::Npa,
        Metric::Mi,
        Metric::Wmc,
    ] {
        assert!(set.contains(m), "MetricSet::all() must contain {m}");
    }
}

#[test]
fn with_dependencies_pulls_in_mi_inputs() {
    let set = MetricSet::from_slice_with_deps(&[Metric::Mi]);
    assert!(set.contains(Metric::Mi));
    assert!(set.contains(Metric::Loc), "Mi depends on Loc");
    assert!(set.contains(Metric::Cyclomatic), "Mi depends on Cyclomatic");
    assert!(set.contains(Metric::Halstead), "Mi depends on Halstead");
    // Unrelated metrics stay out.
    assert!(!set.contains(Metric::Abc));
    assert!(!set.contains(Metric::Tokens));
}

#[test]
fn with_dependencies_pulls_in_wmc_inputs() {
    let set = MetricSet::from_slice_with_deps(&[Metric::Wmc]);
    assert!(set.contains(Metric::Wmc));
    assert!(
        set.contains(Metric::Cyclomatic),
        "Wmc depends on Cyclomatic"
    );
    assert!(set.contains(Metric::Nom), "Wmc depends on Nom");
}

// #428: Cognitive, Exit, and NArgs each compute a per-function
// average whose divisor is sourced from Nom. Selecting any of
// them alone must pull Nom into the closure so the divisor
// reflects the real function count instead of the zero default
// (which produced inf/NaN averages).
#[test]
fn with_dependencies_pulls_in_nom_for_averaging_metrics() {
    for m in [Metric::Cognitive, Metric::Exit, Metric::NArgs] {
        let set = MetricSet::from_slice_with_deps(&[m]);
        assert!(set.contains(m));
        assert!(
            set.contains(Metric::Nom),
            "{m:?} depends on Nom for its per-function average divisor (#428)"
        );
    }
}

// Listing a metric that is already in another entry's closure
// is a no-op and does not corrupt or duplicate state. Today's
// dependency graph is flat (Mi/Wmc both depend only on leaf
// metrics), so this test cannot exercise the worklist's
// transitive resolution — a single-pass implementation that
// pulls in only direct dependencies would also pass. When a
// derived-of-derived metric lands, replace this with a test
// that actually exercises the multi-hop closure (e.g. by
// feeding an entry whose dependency itself has a non-empty
// `dependencies()` list).
#[test]
fn closure_is_idempotent_for_mixed_input() {
    let a = MetricSet::from_slice_with_deps(&[Metric::Mi, Metric::Loc]);
    let b = MetricSet::from_slice_with_deps(&[Metric::Mi]);
    assert_eq!(a, b);
}

// The closure must terminate even when the input contains
// duplicates; the worklist algorithm guards against this by
// skipping bits already set.
#[test]
fn closure_handles_duplicate_input() {
    let set = MetricSet::from_slice_with_deps(&[Metric::Mi, Metric::Mi, Metric::Mi]);
    assert_eq!(set, MetricSet::from_slice_with_deps(&[Metric::Mi]));
}

#[test]
fn empty_slice_yields_empty_set() {
    assert_eq!(MetricSet::from_slice_with_deps(&[]), MetricSet::empty());
}

/// Every `Metric` variant. Tests that need to walk the enum
/// exhaustively reach for this constant. The array initialiser
/// itself has no exhaustiveness check, so the
/// `_all_variants_exhaustive_guard` function below pins the
/// invariant: it pattern-matches every variant on the table side
/// and emits a compile error (`non-exhaustive patterns`) if a
/// new `Metric` variant lands without an entry being added here.
const ALL_VARIANTS: &[Metric] = &[
    Metric::Cognitive,
    Metric::Cyclomatic,
    Metric::Halstead,
    Metric::Loc,
    Metric::Nom,
    Metric::Tokens,
    Metric::NArgs,
    Metric::Exit,
    Metric::Abc,
    Metric::Npm,
    Metric::Npa,
    Metric::Mi,
    Metric::Wmc,
];

/// Compile-time guard that every `Metric` variant appears in
/// [`ALL_VARIANTS`]. `Metric` is `#[non_exhaustive]` for downstream
/// crates, but within this crate (where the enum is defined) the
/// match is still exhaustiveness-checked — so adding
/// `Metric::Foo` without extending the array above triggers
/// `error[E0004]: non-exhaustive patterns` here. The match arms
/// must be kept in lock-step with [`ALL_VARIANTS`]; the
/// `bit_per_metric_is_unique` test additionally pins each variant
/// to a distinct bit, so a missing array entry surfaces twice.
///
/// **Placement note**: this guard lives inside `mod tests`, so the
/// E0004 fires under `cargo test` / `cargo check --tests`, not
/// under a bare `cargo build`. The workspace validation gate
/// (`make pre-commit` and CI) runs `cargo test --workspace
/// --all-features`, so any new variant lands with the guard
/// active — but a contributor running `cargo build` alone after
/// adding `Metric::Foo` will not see the error until the next
/// test invocation.
#[allow(dead_code)]
fn _all_variants_exhaustive_guard(m: Metric) {
    match m {
        Metric::Cognitive
        | Metric::Cyclomatic
        | Metric::Halstead
        | Metric::Loc
        | Metric::Nom
        | Metric::Tokens
        | Metric::NArgs
        | Metric::Exit
        | Metric::Abc
        | Metric::Npm
        | Metric::Npa
        | Metric::Mi
        | Metric::Wmc => (),
    }
}

#[test]
fn from_str_round_trips_every_variant_display_name() {
    // Reverting any single arm in `impl FromStr for Metric`
    // makes this fail on exactly that variant — the test is
    // load-bearing per `.claude/rules/testing.md`.
    for &m in ALL_VARIANTS {
        let parsed: Metric = m
            .to_string()
            .parse()
            .unwrap_or_else(|e| panic!("Display->FromStr round-trip failed for {m}: {e}"));
        assert_eq!(parsed, m, "round-trip mismatch for {m}");
    }
}

#[test]
fn from_str_accepts_nexits_alias_for_exit() {
    // `Metric::Exit` serialises as JSON key "nexits"; we accept
    // both spellings so consumers can name the metric by either
    // its enum-Display spelling or its JSON output key.
    assert_eq!("exit".parse::<Metric>().unwrap(), Metric::Exit);
    assert_eq!("nexits".parse::<Metric>().unwrap(), Metric::Exit);
}

#[test]
fn from_str_rejects_uppercase() {
    let err = "Loc".parse::<Metric>().unwrap_err();
    assert_eq!(err.to_string(), "unknown metric: Loc");
}

// Drift guard: every entry in `Metric::NAMES` must parse via
// `FromStr`, and every variant must have at least one entry
// in the table that parses to it (the `"exit"`/`"nexits"`
// alias means `Exit` is reached via the canonical `"nexits"`
// spelling, not via the Display arm). Adding a `Metric`
// variant without a `NAMES` entry — or vice versa — fails
// here before any pytest run.
#[test]
fn names_table_parses_to_every_variant() {
    use std::collections::HashSet;
    let mut seen: HashSet<Metric> = HashSet::new();
    for name in Metric::NAMES {
        let parsed = name
            .parse::<Metric>()
            .unwrap_or_else(|_| panic!("Metric::NAMES contains {name:?} but FromStr rejects it"));
        seen.insert(parsed);
    }
    for &m in ALL_VARIANTS {
        assert!(
            seen.contains(&m),
            "Metric::{m:?} is not represented in Metric::NAMES; \
             add the canonical spelling to the table",
        );
    }
}

// The error-message `valid: <list>` and the public
// `bca.METRIC_NAMES` tuple both surface this slice verbatim;
// pinning the alphabetised invariant catches accidental
// re-orderings on `cargo test`.
#[test]
fn names_table_is_alphabetised() {
    let mut sorted: Vec<&str> = Metric::NAMES.to_vec();
    sorted.sort_unstable();
    assert_eq!(
        Metric::NAMES,
        sorted.as_slice(),
        "Metric::NAMES must stay alphabetised",
    );
}

// `MetricsOptions::with_metric_set` consumes its argument
// verbatim — no closure resolution. Pinning the contrast with
// `with_only` (which DOES resolve deps) catches a future
// "helpful" refactor that adds auto-resolution to
// `with_metric_set`: such a change would silently fix some
// callers but invalidate the public-API contract documented
// on the builder, where "this set MUST be closed before it
// reaches this builder" is the load-bearing precondition.
//
// The test lives alongside `MetricSet` rather than in
// `spaces.rs` because the contrast is between two `MetricSet`
// operations: `from_slice_with_deps` (closure-resolving) vs.
// raw construction via `empty().with(...)` (no resolution).
#[test]
fn with_metric_set_does_not_resolve_dependencies() {
    // `from_slice_with_deps(&[Mi])` includes Loc, Cyclomatic,
    // Halstead alongside Mi…
    let resolved = MetricSet::from_slice_with_deps(&[Metric::Mi]);
    assert!(resolved.contains(Metric::Mi));
    assert!(resolved.contains(Metric::Loc));
    assert!(resolved.contains(Metric::Cyclomatic));
    assert!(resolved.contains(Metric::Halstead));

    // …whereas `empty().with(Mi)` does NOT auto-resolve, and
    // the caller-owned closure precondition documented on
    // `MetricsOptions::with_metric_set` is what guards
    // against MI being computed against zero-valued inputs.
    let bare = MetricSet::empty().with(Metric::Mi);
    assert!(bare.contains(Metric::Mi));
    assert!(!bare.contains(Metric::Loc), "with(Mi) must NOT pull Loc");
    assert!(
        !bare.contains(Metric::Cyclomatic),
        "with(Mi) must NOT pull Cyclomatic",
    );
    assert!(
        !bare.contains(Metric::Halstead),
        "with(Mi) must NOT pull Halstead",
    );
}

#[test]
fn from_str_rejects_unknown_name() {
    let err = "bogus".parse::<Metric>().unwrap_err();
    assert_eq!(err.to_string(), "unknown metric: bogus");
}

#[test]
fn distinct_bits_per_variant() {
    // Each variant must map to a distinct bit; otherwise the
    // bitfield silently aliases two metrics and gating one
    // toggles the other.
    let mut seen: u32 = 0;
    for &m in ALL_VARIANTS {
        let bit = m.bit();
        assert_ne!(bit, 0, "bit() must be non-zero for {m}");
        assert_eq!(seen & bit, 0, "duplicate bit for {m}: {bit:#b}");
        seen |= bit;
    }
    assert_eq!(seen, MetricSet::ALL_BITS);
}

// Every variant in `ALL_VARIANTS` must round-trip through
// `MetricSet::all().contains(m)`. Adding a `Metric` variant
// without extending `MetricSet::ALL_BITS` (the OR-chain in the
// impl) fails here — a missing entry in `ALL_BITS` leaves the
// new variant's bit clear in `all()` and this assert trips.
#[test]
fn all_variants_round_trip_through_all_contains() {
    let set = MetricSet::all();
    for &m in ALL_VARIANTS {
        assert!(
            set.contains(m),
            "MetricSet::all() must contain {m}; \
             did a new variant land without updating ALL_BITS?",
        );
    }
}

// `MetricSet`'s storage type must remain wide enough for every
// declared `Metric` variant; `bit()` shifts by `self as u32` so
// a 33rd variant would overflow the `u32` storage just as a
// 17th overflowed the previous `u16`. Pin the headroom so a
// future widening (u32 -> u64) is a deliberate, reviewed edit.
#[test]
fn storage_width_covers_every_variant() {
    // `Metric` discriminants are 0..N-1; the highest bit set by
    // any `bit()` call is `1 << (N-1)`. For u32 storage this
    // means N must stay <= 32.
    const STORAGE_BITS: usize = u32::BITS as usize;
    assert!(
        ALL_VARIANTS.len() <= STORAGE_BITS,
        "MetricSet storage exhausted: {} variants > {STORAGE_BITS}-bit storage; widen MetricSet",
        ALL_VARIANTS.len(),
    );
}
