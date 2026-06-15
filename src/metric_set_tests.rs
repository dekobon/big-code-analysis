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
        Metric::Nargs,
        Metric::Nexits,
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
    for m in [Metric::Cognitive, Metric::Nexits, Metric::Nargs] {
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

// #743: `resolved()` closes a verbatim-built set under its
// dependencies. `empty().with(Mi)` carries only the `Mi` bit until
// resolved; afterwards it must carry the full Mi closure.
#[test]
fn resolved_closes_unresolved_set() {
    let unresolved = MetricSet::empty().with(Metric::Mi);
    assert!(unresolved.contains(Metric::Mi));
    assert!(
        !unresolved.contains(Metric::Loc),
        "test premise: the verbatim set must be missing Mi's deps"
    );

    let resolved = unresolved.resolved();
    assert!(resolved.contains(Metric::Mi));
    assert!(resolved.contains(Metric::Loc), "Mi depends on Loc");
    assert!(
        resolved.contains(Metric::Cyclomatic),
        "Mi depends on Cyclomatic"
    );
    assert!(
        resolved.contains(Metric::Halstead),
        "Mi depends on Halstead"
    );
    // Unrelated metrics stay out.
    assert!(!resolved.contains(Metric::Abc));
    assert!(!resolved.contains(Metric::Tokens));
    // Equivalent to building the closure straight from the slice.
    assert_eq!(resolved, MetricSet::from_slice_with_deps(&[Metric::Mi]));
}

// Resolving an already-closed set is a no-op: idempotence.
#[test]
fn resolved_is_idempotent() {
    let once = MetricSet::empty().with(Metric::Mi).resolved();
    assert_eq!(once, once.resolved(), "resolved() must be idempotent");
    // A fully-populated set is unchanged.
    assert_eq!(MetricSet::all().resolved(), MetricSet::all());
    // The empty set has no dependencies to pull in.
    assert_eq!(MetricSet::empty().resolved(), MetricSet::empty());
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
    Metric::Nargs,
    Metric::Nexits,
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
        | Metric::Nargs
        | Metric::Nexits
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
fn from_str_rejects_retired_exit_alias() {
    // `nexits` is the only accepted spelling. The pre-#536 `exit`
    // alias was retired in the 2.0 accessor-vocabulary alignment
    // (#588); it must no longer parse.
    assert_eq!("nexits".parse::<Metric>().unwrap(), Metric::Nexits);
    assert!("exit".parse::<Metric>().is_err());
}

#[test]
fn nexits_canonical_spelling_is_consistent() {
    // The canonical `nexits` spelling round-trips through Display and
    // appears in NAMES; the retired `exit` alias does not appear in
    // NAMES (so it stays out of help / error listings).
    assert_eq!(Metric::Nexits.to_string(), "nexits");
    assert!(Metric::NAMES.contains(&"nexits"));
    assert!(!Metric::NAMES.contains(&"exit"));
}

#[test]
fn serde_uses_canonical_display_spelling() {
    // The hand-written serde impls must emit the canonical `Display`
    // spelling, NOT serde's `rename_all = "snake_case"` form (which
    // would yield `n_args` / `n_exits`). `nargs` and `nexits` are the
    // exact pair that distinguishes the two.
    assert_eq!(serde_json::to_string(&Metric::Nargs).unwrap(), "\"nargs\"",);
    assert_eq!(
        serde_json::to_string(&Metric::Nexits).unwrap(),
        "\"nexits\"",
    );
    // Deserialize back to the same variant.
    assert_eq!(
        serde_json::from_str::<Metric>("\"nargs\"").unwrap(),
        Metric::Nargs,
    );
    assert_eq!(
        serde_json::from_str::<Metric>("\"nexits\"").unwrap(),
        Metric::Nexits,
    );
}

#[test]
fn serde_round_trips_every_variant() {
    for &m in ALL_VARIANTS {
        let json = serde_json::to_string(&m).unwrap();
        let back: Metric = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m, "serde round-trip mismatch for {m}");
    }
}

#[test]
fn serde_round_trips_every_variant_through_non_borrowing_format() {
    // Regression for the `<&str>::deserialize` bug (#555 review): a
    // borrowing deserialize rejects reader-based / non-self-describing
    // formats, which is exactly how `Metric` reaches the wire as the
    // element type of `SuppressionScope::Some`'s `BTreeSet` in CBOR /
    // YAML / TOML output. CBOR (`ciborium`) is reader-based and cannot
    // hand back a borrowed `&str`, so it fails fast if the impl ever
    // regresses to `<&str>::deserialize`.
    for &m in ALL_VARIANTS {
        let mut buf = Vec::new();
        ciborium::into_writer(&m, &mut buf).unwrap();
        let back: Metric = ciborium::from_reader(buf.as_slice()).unwrap();
        assert_eq!(back, m, "CBOR round-trip mismatch for {m}");
    }
}

#[test]
fn deserialize_rejects_retired_exit_alias() {
    // The pre-#536 `exit` alias was retired in #588; deserializing a
    // payload that still spells the metric `exit` must now fail rather
    // than silently resolving to `Metric::Nexits`.
    assert!(serde_json::from_str::<Metric>("\"exit\"").is_err());
}

#[test]
fn deserialize_rejects_unknown_naming_the_input() {
    // The serde error must carry the rejected token (via
    // `ParseMetricError`) so a malformed scope is diagnosable.
    let err = serde_json::from_str::<Metric>("\"no_such_metric\"").unwrap_err();
    assert!(
        err.to_string().contains("no_such_metric"),
        "serde error must name the offending input; got: {err}",
    );
}

#[test]
fn btree_set_iteration_is_declaration_order() {
    use std::collections::BTreeSet;
    // `Ord` follows declaration order; a `BTreeSet<Metric>` therefore
    // iterates deterministically. Insert in scrambled order and confirm
    // the iteration order matches the enum declaration order, not the
    // insertion order or alphabetical order.
    let set: BTreeSet<Metric> =
        BTreeSet::from([Metric::Wmc, Metric::Cognitive, Metric::Nexits, Metric::Abc]);
    let ordered: Vec<Metric> = set.into_iter().collect();
    assert_eq!(
        ordered,
        vec![Metric::Cognitive, Metric::Nexits, Metric::Abc, Metric::Wmc],
    );
}

#[test]
fn suppressible_excludes_tokens_only() {
    let suppressible: Vec<Metric> = Metric::suppressible().collect();
    assert!(
        !suppressible.contains(&Metric::Tokens),
        "tokens has no threshold and must not be suppressible",
    );
    // Every other variant is suppressible.
    for &m in ALL_VARIANTS {
        if m == Metric::Tokens {
            continue;
        }
        assert!(suppressible.contains(&m), "{m} must be suppressible");
    }
    assert_eq!(suppressible.len(), ALL_VARIANTS.len() - 1);
}

#[test]
fn from_str_rejects_uppercase() {
    let err = "Loc".parse::<Metric>().unwrap_err();
    assert_eq!(err.to_string(), "unknown metric: Loc");
}

// Drift guard: every entry in `Metric::NAMES` must parse via
// `FromStr`, and every variant must have at least one entry
// in the table that parses to it. Adding a `Metric`
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

// `MetricSet::with` is the low-level bit primitive: it sets one
// bit and does NOT resolve dependencies. Closure resolution is a
// distinct, explicit step (`resolved` / `from_slice_with_deps`),
// applied by the builders (`with_only`, `with_metric_set`) before
// the set reaches the walker. Pinning this contrast guards against
// a "helpful" refactor that bakes auto-resolution into the
// primitive itself, where it would surprise every caller that
// composes sets bit-by-bit (#743 keeps resolution at the builder
// boundary, not in `with`).
//
// The test lives alongside `MetricSet` rather than in
// `spaces.rs` because the contrast is between two `MetricSet`
// operations: `from_slice_with_deps` / `resolved`
// (closure-resolving) vs. raw construction via `empty().with(...)`
// (no resolution).
#[test]
fn with_primitive_does_not_resolve_dependencies() {
    // `from_slice_with_deps(&[Mi])` includes Loc, Cyclomatic,
    // Halstead alongside Mi…
    let resolved = MetricSet::from_slice_with_deps(&[Metric::Mi]);
    assert!(resolved.contains(Metric::Mi));
    assert!(resolved.contains(Metric::Loc));
    assert!(resolved.contains(Metric::Cyclomatic));
    assert!(resolved.contains(Metric::Halstead));

    // …whereas `empty().with(Mi)` does NOT auto-resolve. The
    // `with_metric_set` builder applies `resolved()` itself (#743),
    // so an unresolved set never reaches the walker through the
    // public API — but the bit primitive stays minimal.
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

// The `with_metric_set` builder is the public seam where an unresolved
// `MetricSet` becomes the closed set the walker computes. Per #743 it
// MUST close the caller's set under `Metric::dependencies` (unlike the
// raw `with` primitive pinned above): `empty().with(Mi)` reaching the
// builder has to gain Loc/Cyclomatic/Halstead, or the MI formula runs
// against zero-valued inputs. The neighbouring
// `with_primitive_does_not_resolve_dependencies` only exercises
// `MetricSet::with`, so it never touches the builder; this test closes
// that gap by actually calling `with_metric_set`, so dropping
// `.resolved()` from the builder flips an assertion here (#927).
#[test]
fn with_metric_set_resolves_dependencies() {
    let unresolved = MetricSet::empty().with(Metric::Mi);
    assert!(
        !unresolved.contains(Metric::Loc),
        "test premise: the verbatim set must omit Mi's deps",
    );

    let opts = crate::MetricsOptions::default().with_metric_set(unresolved);
    // `MetricsOptions::metrics` is the crate-internal selection the
    // walker reads; assert the builder closed the dependency set.
    assert!(opts.metrics.contains(Metric::Mi));
    assert!(
        opts.metrics.contains(Metric::Loc),
        "with_metric_set must pull Loc (Mi dependency)",
    );
    assert!(
        opts.metrics.contains(Metric::Cyclomatic),
        "with_metric_set must pull Cyclomatic (Mi dependency)",
    );
    assert!(
        opts.metrics.contains(Metric::Halstead),
        "with_metric_set must pull Halstead (Mi dependency)",
    );
}

#[test]
fn from_str_rejects_unknown_name() {
    let err = "bogus".parse::<Metric>().unwrap_err();
    assert_eq!(err.to_string(), "unknown metric: bogus");
    // The additive `input()` accessor (#536) recovers the rejected
    // string programmatically, not just via `Display`.
    assert_eq!(err.input(), "bogus");
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
