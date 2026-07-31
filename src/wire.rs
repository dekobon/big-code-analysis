//! Plain, public data-transfer structs mirroring the serialized metric
//! wire shape — the single source of truth for the JSON / YAML / TOML /
//! CBOR output format and the only `Deserialize`-capable view of it.
//!
//! The compute types ([`crate::spaces::FuncSpace`],
//! [`crate::spaces::CodeMetrics`], the per-metric `Stats`, [`crate::Ops`],
//! [`crate::FunctionSpan`]) store *raw* state (e.g. Halstead keeps four
//! operator/operand counts and derives `volume`/`difficulty`/… on demand;
//! `cognitive` keeps a sum and a hidden space count and derives
//! `average`). Their serialized form is therefore a *projection*: a flat
//! record of already-derived values, several of which (the averages,
//! ratios, and Halstead/MI scores) cannot be inverted back to the private
//! state. A plain `#[derive(Deserialize)]` on the compute types is thus
//! impossible.
//!
//! This module defines a parallel struct per metric and per container
//! whose fields are *exactly* the serialized fields, deriving both
//! `Serialize` and `Deserialize`. The compute types' own `Serialize`
//! impls delegate here (via the `From<&Compute>` projections below), so
//! there is exactly one definition of the wire shape; deserialization
//! reads into these `wire` structs and round-trips byte-for-byte.
//!
//! Delegation materializes an owned projection per serialize (a deep clone
//! for the recursive `FuncSpace` tree). This is the deliberate cost
//! of a single source of truth that also round-trips: a borrowing
//! serialize-only mirror would double the struct set and could not derive
//! `Deserialize`. Serialization runs once per file and the projection is
//! dropped immediately, so it is not on a tight inner loop.
//!
//! [`Ops`] is the one measured exception: it serializes through a
//! borrowed mirror instead, for the reasons the private `ops_view`
//! submodule documents.
//!
//! ## Field conventions
//!
//! - Integer-valued metrics (counts, sums, min/max) are `u64` (#530).
//! - Derived / ratio / average fields are `f64` and carry the
//!   `non_finite` (de)serialization: a non-finite value (`NaN`/`±∞`,
//!   meaning "not applicable") serializes to a null uniformly across
//!   formats — native `null` in JSON/YAML/CBOR, an omitted key in TOML —
//!   and deserializes back to `f64::NAN` (#531). Finite values pass
//!   through unchanged, so the round-trip is symmetric and needs no
//!   `Option`.
//! - [`CodeMetrics`] elides unselected metrics (each is an `Option`
//!   skipped when `None`); on read, a present key ⇒ selected, absent ⇒
//!   unselected. [`CodeMetrics::selected`] reconstructs the
//!   [`MetricSet`] from the present keys.

use serde::{Deserialize, Serialize, Serializer};

use crate::metric_set::{Metric, MetricSet};
use crate::metrics::{
    abc, cognitive, cyclomatic, halstead, loc, mi, nargs, nexits, nom, npa, npm, tokens, wmc,
};
use crate::spaces::SpaceKind;
use crate::suppression::SuppressionScope;
use crate::{function, ops};

// The per-metric and VCS wire structs live in domain submodules; the
// `pub use` re-exports keep their public `wire::<Struct>` paths intact
// (these structs are a published deserialization API). The aggregate
// shapes (`CodeMetrics`, `FuncSpace`, `Ops`, `FunctionSpan`), the
// shared helpers, and the round-trip tests stay here.
mod metrics;
// `crate::Ops`'s borrowed serialize path. Nothing to re-export: it
// defines no public type, only the `Serialize` impl `Ops` delegates to.
mod ops_view;
// The VCS arm is wholly `vcs-git`-gated; gating the module (and its
// re-export) keeps default-feature builds free of unused-import noise.
#[cfg(feature = "vcs-git")]
mod vcs;

pub use metrics::*;
#[cfg(feature = "vcs-git")]
pub use vcs::*;

/// `serde(default)` for a non-finite-capable `f64` field: a key absent
/// from the document (TOML omits non-finite values, which have no null
/// literal there) deserializes back to `NaN`.
fn nan_default() -> f64 {
    f64::NAN
}

/// (De)serialization of a non-finite-capable `f64` for `#[serde(with)]`.
///
/// Serialize maps a non-finite value to the format's null
/// (`serialize_none` → native `null` in JSON/YAML/CBOR, omitted key in
/// TOML); deserialize maps a `null` (or, paired with
/// [`nan_default`], an absent key) back to `f64::NAN`. Finite values are
/// passed through verbatim. This is the structured-output arm of the
/// non-finite policy (#531); the human-readable arm lives in
/// `crate::output::numfmt`.
mod non_finite {
    use serde::{Deserialize, Deserializer, Serializer};

    // serde's `#[serde(with = ...)]` contract fixes this signature to
    // `(&T, S)`, so the by-reference `f64` is required, not a choice.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub(super) fn serialize<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
        if value.is_finite() {
            serializer.serialize_f64(*value)
        } else {
            serializer.serialize_none()
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        Ok(Option::<f64>::deserialize(deserializer)?.unwrap_or(f64::NAN))
    }
}

/// A file's `risk_score` at its most-recent present point, or `0.0` if it
/// has no present point (which the trend builder never produces). Used to
/// rank files for `top_files` truncation.
#[cfg(feature = "vcs-git")]
fn latest_present_risk(points: &[Option<crate::vcs::Stats>]) -> f64 {
    points
        .iter()
        .rev()
        .find_map(|s| s.as_ref().map(|s| s.risk_score))
        .unwrap_or(0.0)
}

#[cfg(all(test, feature = "vcs-git"))]
mod trend_wire_tests {
    use super::*;

    // Exact-equality on f64 is intentional: the values are the
    // exactly-representable literals fed into the fixtures.
    #[allow(clippy::float_cmp)]
    fn risk(points: &[Option<f64>]) -> f64 {
        let owned: Vec<Option<crate::vcs::Stats>> = points
            .iter()
            .map(|p| {
                p.map(|risk_score| crate::vcs::Stats {
                    risk_score,
                    ..Default::default()
                })
            })
            .collect();
        latest_present_risk(&owned)
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn latest_present_risk_picks_the_newest_present_point() {
        // Scans from the back: the most-recent present point wins, even
        // with later `None`s and earlier present points.
        assert_eq!(risk(&[Some(1.0), None, Some(3.0), None]), 3.0);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn latest_present_risk_defaults_to_zero_when_all_absent() {
        // The documented fallback for a file with no present point (which
        // the trend builder never produces, but the helper still defines).
        assert_eq!(risk(&[None, None]), 0.0);
        assert_eq!(risk(&[]), 0.0);
    }

    /// A `Vcs` row with finite values plus the optional blocks set.
    fn sample_vcs() -> Vcs {
        Vcs {
            commits_long: 12,
            commits_recent: 4,
            churn_long: 340,
            churn_recent: 90,
            authors_long: 3,
            authors_recent: 2,
            ownership_top_share: 0.625,
            burst: 0.333,
            bug_fix_commits: 2,
            security_fix_commits: 1,
            revert_commits: 0,
            age_days: 200,
            last_modified_days: 5,
            change_entropy_long: 1.5,
            change_entropy_recent: 0.5,
            cochange_entropy_long: 2.0,
            cochange_entropy_recent: 0.25,
            risk_score: 7.5,
            hotspot_score: Some(3.25),
            author_ids: Some(vec!["deadbeef".to_owned()]),
        }
    }

    /// Issue #702: the `Vcs` derived/ratio f64 fields must carry the #531
    /// `non_finite` (de)serialization — a NaN serializes to a format null
    /// and round-trips back to NaN, instead of erroring `to_string` (NaN is
    /// invalid JSON). Covers JSON, YAML, and CBOR.
    #[test]
    fn vcs_non_finite_floats_round_trip_as_null() {
        let mut row = sample_vcs();
        row.risk_score = f64::NAN;
        row.burst = f64::INFINITY;
        row.cochange_entropy_recent = f64::NEG_INFINITY;

        // JSON: serialization must succeed (would error without non_finite)
        // and the non-finite fields appear as null.
        let json = serde_json::to_string(&row).expect("serialize Vcs with NaN to JSON");
        assert!(json.contains("\"risk_score\":null"), "got {json}");
        let from_json: Vcs = serde_json::from_str(&json).expect("parse Vcs from JSON");
        assert!(from_json.risk_score.is_nan());
        assert!(from_json.burst.is_nan());
        assert!(from_json.cochange_entropy_recent.is_nan());
        // Finite fields are unchanged.
        assert_eq!(from_json.commits_long, row.commits_long);
        assert!((from_json.ownership_top_share - row.ownership_top_share).abs() < 1e-12);

        // YAML round-trip.
        let yaml = serde_yaml::to_string(&row).expect("serialize Vcs to YAML");
        let from_yaml: Vcs = serde_yaml::from_str(&yaml).expect("parse Vcs from YAML");
        assert!(from_yaml.risk_score.is_nan() && from_yaml.burst.is_nan());

        // CBOR round-trip.
        let mut bytes = Vec::new();
        ciborium::into_writer(&row, &mut bytes).expect("serialize Vcs to CBOR");
        let from_cbor: Vcs = ciborium::from_reader(bytes.as_slice()).expect("parse Vcs from CBOR");
        assert!(from_cbor.risk_score.is_nan() && from_cbor.cochange_entropy_recent.is_nan());
    }

    /// Issue #702: `VcsTrendPoint` and `VcsTrend` carry the metric block
    /// under a nested `vcs` key (not `#[serde(flatten)]`). CBOR is a
    /// *written* trend format but was never *read back* in tests — pin the
    /// round-trip for both YAML and CBOR.
    #[test]
    fn vcs_trend_point_round_trips_through_yaml_and_cbor() {
        let point = VcsTrendPoint {
            as_of: 1_700_000_000,
            vcs: sample_vcs(),
        };

        let yaml = serde_yaml::to_string(&point).expect("serialize VcsTrendPoint to YAML");
        let from_yaml: VcsTrendPoint = serde_yaml::from_str(&yaml).expect("parse point from YAML");
        assert_eq!(from_yaml, point);

        let mut bytes = Vec::new();
        ciborium::into_writer(&point, &mut bytes).expect("serialize VcsTrendPoint to CBOR");
        let from_cbor: VcsTrendPoint =
            ciborium::from_reader(bytes.as_slice()).expect("parse point from CBOR");
        assert_eq!(from_cbor, point);
    }

    #[test]
    fn vcs_trend_round_trips_through_yaml_and_cbor() {
        let trend = VcsTrend {
            trend_schema_version: 1,
            vcs_schema_version: 2,
            risk_score_version: 2,
            long_window_days: 365,
            recent_window_days: 90,
            truncated_shallow_clone: false,
            as_of_points: vec![1_699_000_000, 1_700_000_000],
            files: std::collections::BTreeMap::from([(
                "src/lib.rs".to_owned(),
                vec![
                    None,
                    Some(VcsTrendPoint {
                        as_of: 1_700_000_000,
                        vcs: sample_vcs(),
                    }),
                ],
            )]),
            deltas: VcsTrendDeltas::default(),
        };

        let yaml = serde_yaml::to_string(&trend).expect("serialize VcsTrend to YAML");
        let from_yaml: VcsTrend = serde_yaml::from_str(&yaml).expect("parse VcsTrend from YAML");
        assert_eq!(from_yaml, trend);

        let mut bytes = Vec::new();
        ciborium::into_writer(&trend, &mut bytes).expect("serialize VcsTrend to CBOR");
        let from_cbor: VcsTrend =
            ciborium::from_reader(bytes.as_slice()).expect("parse VcsTrend from CBOR");
        assert_eq!(from_cbor, trend);
    }
}

/// Wire form of [`crate::spaces::CodeMetrics`].
///
/// Each metric is an `Option` skipped when `None`: an unselected metric
/// (or a class-only metric flagged `is_disabled`) is absent from the
/// document. On read, a present key ⇒ the metric was selected; absent ⇒
/// unselected. [`CodeMetrics::selected`] rebuilds the [`MetricSet`].
///
/// Field order matches the compute type's `Serialize` order exactly so
/// the emitted record is byte-identical.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CodeMetrics {
    /// `NArgs` metric, if selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nargs: Option<Nargs>,
    /// `Nexits` metric, if selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nexits: Option<Nexits>,
    /// `Cognitive` metric, if selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cognitive: Option<Cognitive>,
    /// `Cyclomatic` metric, if selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cyclomatic: Option<Cyclomatic>,
    /// `Halstead` metric, if selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub halstead: Option<Halstead>,
    /// `Loc` metric, if selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loc: Option<Loc>,
    /// `Nom` metric, if selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nom: Option<Nom>,
    /// `Tokens` metric, if selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<Tokens>,
    /// `Mi` metric, if selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mi: Option<Mi>,
    /// `Abc` metric, if selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abc: Option<Abc>,
    /// `Wmc` metric, if selected and not disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wmc: Option<Wmc>,
    /// `Npm` metric, if selected and not disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npm: Option<Npm>,
    /// `Npa` metric, if selected and not disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npa: Option<Npa>,
    /// Change-history (VCS) metrics, present only for the file-level
    /// space when a history walk supplied them. Gated behind `vcs-git`.
    #[cfg(feature = "vcs-git")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vcs: Option<Vcs>,
}

impl From<&crate::spaces::CodeMetrics> for CodeMetrics {
    fn from(c: &crate::spaces::CodeMetrics) -> Self {
        let sel = c.selected;
        // The class-only metrics carry their own disabled flag (a
        // non-class language never emits them) alongside the selection
        // mask, mirroring the compute `Serialize` impl exactly.
        let on = |m: Metric| sel.contains(m);
        Self {
            nargs: on(Metric::Nargs).then(|| Nargs::from(&c.nargs)),
            nexits: on(Metric::Nexits).then(|| Nexits::from(&c.nexits)),
            cognitive: on(Metric::Cognitive).then(|| Cognitive::from(&c.cognitive)),
            cyclomatic: on(Metric::Cyclomatic).then(|| Cyclomatic::from(&c.cyclomatic)),
            halstead: on(Metric::Halstead).then(|| Halstead::from(&c.halstead)),
            loc: on(Metric::Loc).then(|| Loc::from(&c.loc)),
            nom: on(Metric::Nom).then(|| Nom::from(&c.nom)),
            tokens: on(Metric::Tokens).then(|| Tokens::from(&c.tokens)),
            mi: on(Metric::Mi).then(|| Mi::from(&c.mi)),
            abc: on(Metric::Abc).then(|| Abc::from(&c.abc)),
            wmc: (on(Metric::Wmc) && !c.wmc.is_disabled()).then(|| Wmc::from(&c.wmc)),
            npm: (on(Metric::Npm) && !c.npm.is_disabled()).then(|| Npm::from(&c.npm)),
            npa: (on(Metric::Npa) && !c.npa.is_disabled()).then(|| Npa::from(&c.npa)),
            // VCS data is injected post-analysis, so its presence — not
            // the selection mask — governs emission.
            #[cfg(feature = "vcs-git")]
            vcs: c.vcs.as_ref().map(Vcs::from),
        }
    }
}

impl CodeMetrics {
    /// Reconstruct the [`MetricSet`] from the metrics present on the wire.
    ///
    /// A metric key present in the deserialized document means it was
    /// selected when the document was produced; absent means it was
    /// pruned (unselected, or a disabled class-only metric). This is the
    /// inverse of the selection eliding in [`From`].
    #[must_use]
    pub fn selected(&self) -> MetricSet {
        let mut set = MetricSet::empty();
        let mut mark = |present: bool, metric: Metric| {
            if present {
                set.insert(metric);
            }
        };
        mark(self.nargs.is_some(), Metric::Nargs);
        mark(self.nexits.is_some(), Metric::Nexits);
        mark(self.cognitive.is_some(), Metric::Cognitive);
        mark(self.cyclomatic.is_some(), Metric::Cyclomatic);
        mark(self.halstead.is_some(), Metric::Halstead);
        mark(self.loc.is_some(), Metric::Loc);
        mark(self.nom.is_some(), Metric::Nom);
        mark(self.tokens.is_some(), Metric::Tokens);
        mark(self.mi.is_some(), Metric::Mi);
        mark(self.abc.is_some(), Metric::Abc);
        mark(self.wmc.is_some(), Metric::Wmc);
        mark(self.npm.is_some(), Metric::Npm);
        mark(self.npa.is_some(), Metric::Npa);
        set
    }
}

/// Greatest space-nesting depth [`FuncSpace`] and [`Ops`] will serialize.
///
/// Both are trees, and `serde` cannot emit a tree without one native stack
/// frame per level, so the depth has to be bounded somewhere: past it, the
/// runtime aborts the process instead of raising a catchable panic
/// (#1056). A space tree deeper than this fails serialization with an
/// ordinary serializer error naming the limit.
///
/// The value matches the recursion limit `serde_json`'s `Deserializer`
/// already imposes on the same documents, and is generous against both
/// ends of that comparison. On the read side, a `FuncSpace` costs *two*
/// JSON nesting levels (its object plus its `spaces` array), so parsing
/// one of these documents back caps out near 61 levels — the emit limit
/// is the more permissive of the two. On the source side, the deepest
/// space nesting across the 14 450-file corpus under `tests/repositories`
/// is 10 levels.
pub const MAX_SPACE_SERIALIZE_DEPTH: usize = 128;

/// Maps a recursive compute-side tree onto its wire form using an explicit
/// work stack.
///
/// The natural `children.iter().map(Self::from).collect()` recursion costs
/// one stack frame per nesting level and overflows a default 2 MiB thread
/// stack at roughly 900 levels of nested functions — a `SIGABRT`, not a
/// catchable panic (#1056). Space nesting is attacker-controlled, so the
/// conversion is iterative: `build` is called on each node exactly once,
/// bottom-up, with that node's already-converted children.
fn map_tree<'a, Src, Dst>(
    root: &'a Src,
    children_of: fn(&'a Src) -> &'a [Src],
    build: fn(&'a Src, Vec<Dst>) -> Dst,
) -> Dst {
    // The root frame is held outside the stack so that popping a completed
    // frame always has somewhere to deposit it, and so the loop needs no
    // fallible "the stack cannot be empty here" step.
    let mut root_frame = MapFrame::new(root, children_of);
    let mut descendants = Vec::new();
    loop {
        let frame = match descendants.last_mut() {
            Some(frame) => frame,
            None => &mut root_frame,
        };
        let source = frame.source;
        if let Some(child) = children_of(source).get(frame.next_child) {
            frame.next_child += 1;
            descendants.push(MapFrame::new(child, children_of));
            continue;
        }
        // The current frame has no children left: fold it into its parent,
        // or stop once that frame is the root.
        let Some(done) = descendants.pop() else { break };
        let converted = build(done.source, done.children);
        match descendants.last_mut() {
            Some(parent) => parent.children.push(converted),
            None => root_frame.children.push(converted),
        }
    }
    build(root_frame.source, root_frame.children)
}

/// One in-progress node of a [`map_tree`] walk.
struct MapFrame<'a, Src, Dst> {
    /// The compute-side node being converted.
    source: &'a Src,
    /// How many of `source`'s children have been pushed onto the walk.
    next_child: usize,
    /// Wire forms of the children completed so far, in source order.
    children: Vec<Dst>,
}

impl<'a, Src, Dst> MapFrame<'a, Src, Dst> {
    fn new(source: &'a Src, children_of: fn(&'a Src) -> &'a [Src]) -> Self {
        Self {
            source,
            next_child: 0,
            children: Vec::with_capacity(children_of(source).len()),
        }
    }
}

/// Serializes a [`FuncSpace`]'s children one nesting level deeper,
/// refusing to descend past [`MAX_SPACE_SERIALIZE_DEPTH`].
fn serialize_spaces<S: Serializer>(spaces: &[FuncSpace], serializer: S) -> Result<S::Ok, S::Error> {
    crate::recursion::serialize_bounded(spaces, MAX_SPACE_SERIALIZE_DEPTH, "FuncSpace", serializer)
}

/// Serializes an [`Ops`] node's children one nesting level deeper,
/// refusing to descend past [`MAX_SPACE_SERIALIZE_DEPTH`].
fn serialize_ops_spaces<S: Serializer>(spaces: &[Ops], serializer: S) -> Result<S::Ok, S::Error> {
    crate::recursion::serialize_bounded(spaces, MAX_SPACE_SERIALIZE_DEPTH, "Ops", serializer)
}

/// Wire form of [`crate::spaces::FuncSpace`] — a recursive metric tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FuncSpace {
    /// The name of the space (file path or AST-derived identifier).
    pub name: Option<String>,
    /// The first line of the space.
    pub start_line: usize,
    /// The last line of the space.
    pub end_line: usize,
    /// The space kind.
    pub kind: SpaceKind,
    /// All nested subspaces.
    #[serde(serialize_with = "serialize_spaces")]
    pub spaces: Vec<FuncSpace>,
    /// The metrics of the space.
    pub metrics: CodeMetrics,
    /// In-source suppression markers applying to the space (elided when
    /// empty, matching the compute type's schema).
    #[serde(default, skip_serializing_if = "SuppressionScope::is_empty")]
    pub suppressed: SuppressionScope,
}

// The wire tree mirrors the compute tree's nesting, so its `Drop` needs
// the same de-recursion (#1056). See [`crate::recursion`].
crate::recursion::impl_iterative_drop!(FuncSpace, spaces);

impl From<&crate::spaces::FuncSpace> for FuncSpace {
    fn from(f: &crate::spaces::FuncSpace) -> Self {
        map_tree(
            f,
            |source| &source.spaces,
            |source, spaces| Self {
                name: source.name.clone(),
                start_line: source.start_line,
                end_line: source.end_line,
                kind: source.kind,
                spaces,
                metrics: CodeMetrics::from(&source.metrics),
                suppressed: source.suppressed.clone(),
            },
        )
    }
}

/// Wire form of [`crate::Ops`] — a recursive operator/operand tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ops {
    /// The name of the space (file path or AST-derived identifier).
    pub name: Option<String>,
    /// Whether [`Ops::name`] was derived via lossy UTF-8 conversion.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub name_was_lossy: bool,
    /// The first line of the space.
    pub start_line: usize,
    /// The last line of the space.
    pub end_line: usize,
    /// The space kind.
    pub kind: SpaceKind,
    /// All nested subspaces.
    #[serde(serialize_with = "serialize_ops_spaces")]
    pub spaces: Vec<Ops>,
    /// The operands in the space.
    pub operands: Vec<String>,
    /// The operators in the space.
    pub operators: Vec<String>,
}

// The wire tree mirrors the compute tree's nesting, so its `Drop` needs
// the same de-recursion (#1056). See [`crate::recursion`].
crate::recursion::impl_iterative_drop!(Ops, spaces);

// Owned `Ops` projections built on this thread.
//
// Both projections emit the same document, so no assertion on the output
// can tell which path ran and a revert to `serialize_via_wire!` would be
// silent. This is the observable `serializing_ops_builds_no_owned_projection`
// reads. One `Cell` bump per whole-tree conversion costs nothing against it.
crate::observation::counter!(owned_ops_projections);

impl From<&ops::Ops> for Ops {
    fn from(o: &ops::Ops) -> Self {
        owned_ops_projections::record();
        map_tree(
            o,
            |source| &source.spaces,
            |source, spaces| Self {
                name: source.name.clone(),
                name_was_lossy: source.name_was_lossy,
                start_line: source.start_line,
                end_line: source.end_line,
                kind: source.kind,
                spaces,
                operands: source.operands.clone(),
                operators: source.operators.clone(),
            },
        )
    }
}

/// Wire form of [`crate::FunctionSpan`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionSpan {
    /// The function name, or `null` when it could not be resolved.
    pub name: Option<String>,
    /// The first line of the function.
    pub start_line: usize,
    /// The last line of the function.
    pub end_line: usize,
}

impl From<&function::FunctionSpan> for FunctionSpan {
    fn from(f: &function::FunctionSpan) -> Self {
        Self {
            name: f.name.clone(),
            start_line: f.start_line,
            end_line: f.end_line,
        }
    }
}

// ---------------------------------------------------------------------------
// Delegating `Serialize` impls: the compute types serialize *through* the
// wire projection, so the wire structs above are the single source of the
// emitted format.
// ---------------------------------------------------------------------------

/// Implement `Serialize` for a compute type by projecting it to its wire
/// form and serializing that, keeping the wire struct the sole definition
/// of the output shape.
macro_rules! serialize_via_wire {
    ($compute:ty => $wire:ident) => {
        impl Serialize for $compute {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                $wire::from(self).serialize(serializer)
            }
        }
    };
}

serialize_via_wire!(abc::Stats => Abc);
serialize_via_wire!(cognitive::Stats => Cognitive);
serialize_via_wire!(cyclomatic::Stats => Cyclomatic);
serialize_via_wire!(nexits::Stats => Nexits);
serialize_via_wire!(halstead::Stats => Halstead);
serialize_via_wire!(loc::Stats => Loc);
serialize_via_wire!(mi::Stats => Mi);
serialize_via_wire!(nargs::Stats => Nargs);
serialize_via_wire!(nom::Stats => Nom);
serialize_via_wire!(npa::Stats => Npa);
serialize_via_wire!(npm::Stats => Npm);
serialize_via_wire!(tokens::Stats => Tokens);
serialize_via_wire!(wmc::Stats => Wmc);
serialize_via_wire!(crate::spaces::CodeMetrics => CodeMetrics);
serialize_via_wire!(crate::spaces::FuncSpace => FuncSpace);
// `ops::Ops` is absent on purpose: it serializes through the borrowed
// mirror in `ops_view`.
serialize_via_wire!(function::FunctionSpan => FunctionSpan);

// Own file so their prose does not spend this file's `loc.sloc`
// budget (#1066); `.bcaignore` keeps `*_tests.rs` out of the self-scan.
#[cfg(test)]
#[path = "wire_ops_tests.rs"]
mod ops_tests;

#[cfg(test)]
// The round-trip assertions compare floats exactly on purpose: CBOR stores
// raw IEEE-754 bits, YAML/TOML emit full precision, and serde_json's
// `float_roundtrip` feature (enabled in Cargo.toml) makes its parser
// bit-exact — so a value serialized and read back equals the original
// down to the last bit. Exactness is the property under test.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::RustParser;
    use crate::test_support::check_func_space;

    /// A branchy multi-function Rust fixture so several metrics are
    /// non-trivial (cyclomatic > 1, multiple spaces, real Halstead/MI).
    const FIXTURE: &str = "\
fn classify(x: i32) -> i32 {
    if x > 0 {
        x * 2
    } else if x < 0 {
        -x
    } else {
        0
    }
}

fn run() {
    let adder = |a: i32, b: i32| a + b;
    let _ = adder(classify(3), classify(-4));
}
";

    /// Independent oracle of the `FIXTURE` tree's hand-verified integer
    /// metrics. `assert_eq!(back, fs.to_wire())` alone cannot catch a
    /// mismapped `From` field — both sides flow through the same projection,
    /// so a swap corrupts them identically — so the round-trip tests anchor
    /// against these known values to break the closed loop. (Grammar bumps
    /// may shift them; update alongside the metric snapshot tests.)
    fn assert_fixture_oracle(tree: &FuncSpace) {
        // Two top-level functions: `classify` and `run`.
        assert_eq!(tree.kind, SpaceKind::Unit);
        assert_eq!(tree.spaces.len(), 2, "classify + run");

        let m = &tree.metrics;
        assert_eq!(m.cyclomatic.as_ref().unwrap().sum, 6, "unit cyclomatic.sum");
        // The unit's *own* cyclomatic is the base 1 (no decisions at file
        // top level), while `sum` rolls up both functions and the closure
        // (#958). `value != sum` here is the whole point of the field.
        assert_eq!(
            m.cyclomatic.as_ref().unwrap().value,
            1,
            "unit cyclomatic.value (own, excludes children)"
        );
        assert_eq!(m.cognitive.as_ref().unwrap().sum, 3, "unit cognitive.sum");
        assert_eq!(
            m.cognitive.as_ref().unwrap().value,
            0,
            "unit cognitive.value (own)"
        );
        assert_eq!(m.loc.as_ref().unwrap().sloc, 14, "unit loc.sloc");
        assert_eq!(m.nom.as_ref().unwrap().total, 3, "unit nom.total");
        // ABC is finite and distinguishes assignments/branches/conditions —
        // a swap of those accessors in `From` would surface here.
        let abc = m.abc.as_ref().unwrap();
        assert_eq!((abc.assignments, abc.branches, abc.conditions), (2, 3, 4));

        let classify = tree
            .spaces
            .iter()
            .find(|s| s.name.as_deref() == Some("classify"))
            .expect("classify space");
        let classify_cyclo = classify.metrics.cyclomatic.as_ref().unwrap();
        assert_eq!(classify_cyclo.sum, 3, "classify cyclomatic.sum");
        // `classify` is a leaf, so its own value equals its subtree sum.
        assert_eq!(classify_cyclo.value, 3, "classify cyclomatic.value (leaf)");

        // `run` is an interior space: it owns the `adder` closure child.
        // Its own cyclomatic is the base 1, but `sum` (2) folds in the
        // closure's base 1 — the exact interior-space case #958 closes.
        let run = tree
            .spaces
            .iter()
            .find(|s| s.name.as_deref() == Some("run"))
            .expect("run space");
        let run_cyclo = run.metrics.cyclomatic.as_ref().unwrap();
        assert_eq!(run_cyclo.sum, 2, "run cyclomatic.sum (run + adder closure)");
        assert_eq!(
            run_cyclo.value, 1,
            "run cyclomatic.value (own, excludes closure)"
        );
    }

    /// The acceptance criterion: a `FuncSpace` serialized to JSON parses
    /// back into a `wire::FuncSpace` that re-serializes byte-for-byte, is
    /// structurally equal to the source projection, and carries the
    /// hand-verified metric values.
    #[test]
    fn json_round_trips() {
        check_func_space::<RustParser, _>(FIXTURE, "fixture.rs", |fs| {
            let json = serde_json::to_string(&fs).expect("serialize FuncSpace to JSON");
            let back: FuncSpace = serde_json::from_str(&json).expect("parse wire::FuncSpace");
            assert_eq!(
                back,
                fs.to_wire(),
                "deserialized wire tree must equal the projection"
            );
            assert_eq!(
                serde_json::to_string(&back).expect("re-serialize wire"),
                json,
                "re-serialized wire must be byte-identical to the original JSON",
            );
            // Independent oracle: guards `From`-projection correctness, which
            // the closed serialize→deserialize loop above cannot.
            assert_fixture_oracle(&back);
        });
    }

    #[test]
    fn yaml_round_trips() {
        check_func_space::<RustParser, _>(FIXTURE, "fixture.rs", |fs| {
            let yaml = serde_yaml::to_string(&fs).expect("serialize to YAML");
            let back: FuncSpace = serde_yaml::from_str(&yaml).expect("parse wire from YAML");
            assert_eq!(back, fs.to_wire());
            assert_eq!(serde_yaml::to_string(&back).expect("re-serialize"), yaml);
        });
    }

    #[test]
    fn toml_round_trips() {
        check_func_space::<RustParser, _>(FIXTURE, "fixture.rs", |fs| {
            let toml = toml::to_string(&fs).expect("serialize to TOML");
            let back: FuncSpace = toml::from_str(&toml).expect("parse wire from TOML");
            assert_eq!(back, fs.to_wire());
            assert_eq!(toml::to_string(&back).expect("re-serialize"), toml);
        });
    }

    #[test]
    fn cbor_round_trips() {
        check_func_space::<RustParser, _>(FIXTURE, "fixture.rs", |fs| {
            let mut bytes = Vec::new();
            ciborium::into_writer(&fs, &mut bytes).expect("serialize to CBOR");
            let back: FuncSpace =
                ciborium::from_reader(bytes.as_slice()).expect("parse wire from CBOR");
            assert_eq!(back, fs.to_wire());
            let mut re = Vec::new();
            ciborium::into_writer(&back, &mut re).expect("re-serialize");
            assert_eq!(re, bytes, "CBOR re-serialization must be byte-identical");
        });
    }

    /// `FunctionSpan` (#536 shape: `name: Option<String>`, no `error`
    /// field) round-trips through JSON for both a resolved name and an
    /// unresolved one (`None` → JSON `null`), and the serialized object
    /// carries no `error` key.
    #[test]
    fn function_span_round_trips() {
        let resolved = FunctionSpan {
            name: Some("foo".to_owned()),
            start_line: 1,
            end_line: 4,
        };
        let unresolved = FunctionSpan {
            name: None,
            start_line: 7,
            end_line: 8,
        };

        for span in [resolved, unresolved] {
            let json = serde_json::to_string(&span).expect("serialize FunctionSpan");
            assert!(
                !json.contains("error"),
                "FunctionSpan JSON must not carry an `error` key, got {json}",
            );
            let back: FunctionSpan = serde_json::from_str(&json).expect("parse FunctionSpan");
            assert_eq!(back, span, "FunctionSpan must round-trip through JSON");
        }

        // The unresolved span emits `name: null`, never an empty string.
        let json = serde_json::to_string(&FunctionSpan {
            name: None,
            start_line: 7,
            end_line: 8,
        })
        .expect("serialize");
        assert!(
            json.contains(r#""name":null"#),
            "unresolved name must serialize to JSON null, got {json}",
        );
    }

    /// A non-finite float (`NaN`/`±∞`) serializes to the format's null and
    /// deserializes back to `NaN`: native `null` (JSON/YAML/CBOR) and an
    /// omitted key (TOML, which has no null literal, recovered via the
    /// field default). Mi fields are the simplest plain-`f64` carrier.
    #[test]
    fn non_finite_floats_round_trip_as_null_or_omission() {
        for probe in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mi = Mi {
                original: probe,
                sei: 1.5,
                visual_studio: 2.0,
            };

            let json = serde_json::to_string(&mi).expect("JSON");
            assert!(
                json.contains(r#""original":null"#),
                "non-finite must serialize to JSON null, got {json}",
            );
            assert!(
                serde_json::from_str::<Mi>(&json)
                    .expect("parse")
                    .original
                    .is_nan(),
                "JSON null must deserialize back to NaN",
            );

            let yaml = serde_yaml::to_string(&mi).expect("YAML");
            assert!(
                yaml.contains("original: null"),
                "non-finite must serialize to YAML null, got {yaml}",
            );
            assert!(
                serde_yaml::from_str::<Mi>(&yaml)
                    .expect("parse")
                    .original
                    .is_nan()
            );

            let toml = toml::to_string(&mi).expect("TOML");
            assert!(
                !toml.contains("original"),
                "TOML must omit the non-finite key (no null literal), got {toml}",
            );
            assert!(
                toml::from_str::<Mi>(&toml)
                    .expect("parse")
                    .original
                    .is_nan(),
                "omitted TOML key must default back to NaN",
            );

            // CBOR: serialize to bytes, confirm the field decodes as a null
            // token, and that it deserializes back to NaN.
            let mut cbor = Vec::new();
            ciborium::into_writer(&mi, &mut cbor).expect("CBOR");
            let value: ciborium::value::Value =
                ciborium::from_reader(cbor.as_slice()).expect("parse cbor value");
            let ciborium::value::Value::Map(map) = &value else {
                panic!("CBOR root is not a map");
            };
            let original_key = ciborium::value::Value::Text("original".to_owned());
            let original = map
                .iter()
                .find_map(|(k, v)| (*k == original_key).then_some(v));
            assert_eq!(
                original,
                Some(&ciborium::value::Value::Null),
                "non-finite must serialize to CBOR null",
            );
            assert!(
                ciborium::from_reader::<Mi, _>(cbor.as_slice())
                    .expect("parse")
                    .original
                    .is_nan(),
                "CBOR null must deserialize back to NaN",
            );

            // Finite siblings are unaffected.
            let back = serde_json::from_str::<Mi>(&json).expect("parse");
            assert_eq!(back.sei, 1.5);
            assert_eq!(back.visual_studio, 2.0);
        }
    }

    /// `selected()` reconstructs the `MetricSet` from the metric keys
    /// present on the wire: a full tree marks every metric, a pruned tree
    /// (here keeping only `loc`) marks exactly that one.
    #[test]
    fn selected_is_inferred_from_present_keys() {
        check_func_space::<RustParser, _>(FIXTURE, "fixture.rs", |fs| {
            let full = fs.metrics.to_wire();
            let selected = full.selected();
            assert!(selected.contains(Metric::Loc));
            assert!(selected.contains(Metric::Cyclomatic));

            // A pruned document (only `loc` present) infers only `loc`.
            let json = serde_json::to_string(&full).expect("serialize metrics");
            let mut value: serde_json::Value = serde_json::from_str(&json).expect("parse value");
            let obj = value.as_object_mut().expect("metrics object");
            obj.retain(|k, _| k == "loc");
            let pruned: CodeMetrics =
                serde_json::from_value(value).expect("parse pruned wire metrics");
            let pruned_selected = pruned.selected();
            assert!(pruned_selected.contains(Metric::Loc));
            assert!(!pruned_selected.contains(Metric::Cyclomatic));
            assert!(pruned.cyclomatic.is_none());
        });
    }

    // -----------------------------------------------------------------
    // Stack-depth regression tests (#1056)
    //
    // The #700 / #709 small-stack tests cover the *dump* walk and build
    // `FuncSpace` values by hand, so nothing exercised the wire
    // conversion or `Serialize`. These drive `analyze` and then convert /
    // serialize, because the hazard scales with `FuncSpace` nesting, not
    // AST depth — nested parentheses reach depth 200 000 while opening a
    // single space, so testing the wrong shape looks like a pass.
    // -----------------------------------------------------------------

    /// The size of a `bca` consumer thread and of a `tokio` blocking
    /// thread — the stack the guarded limits are dimensioned against.
    const PRODUCTION_STACK: usize = 2 * 1024 * 1024;

    /// Deliberately far below `PRODUCTION_STACK`: a re-recursed `From` or
    /// `Drop` fails loudly here instead of riding on the test runner's
    /// generous stack.
    const TIGHT_STACK: usize = 512 * 1024;

    /// Rust source nesting `depth` functions, one `FuncSpace` per level
    /// below the file-level `Unit`.
    pub(super) fn nested_functions(depth: usize) -> String {
        use std::fmt::Write as _;
        let mut source = String::with_capacity(depth * 14);
        for level in 0..depth {
            let _ = writeln!(source, "fn f{level}() {{");
        }
        for _ in 0..depth {
            source.push_str("}\n");
        }
        source
    }

    /// Analyses [`nested_functions`], computing only `loc` so the cost of
    /// unrelated metrics does not dominate a deep fixture.
    fn analyze_nested(depth: usize) -> crate::FuncSpace {
        crate::analyze(
            crate::Source::new(crate::LANG::Rust, nested_functions(depth).as_bytes())
                .with_name(Some("nested.rs".to_owned())),
            crate::MetricsOptions::default().with_only(&[Metric::Loc]),
        )
        .expect("nested-function fixture must analyse")
    }

    /// Longest chain of nested spaces in `space`, measured without
    /// recursing so the measurement cannot overflow before the code
    /// under test does.
    fn wire_nesting_depth(space: &FuncSpace) -> usize {
        let mut deepest = 0;
        let mut stack = vec![(space, 1_usize)];
        while let Some((node, depth)) = stack.pop() {
            deepest = deepest.max(depth);
            for child in &node.spaces {
                stack.push((child, depth + 1));
            }
        }
        deepest
    }

    /// Runs `body` on a thread with an explicit stack so the result does
    /// not depend on the test harness's own stack size.
    fn on_stack<T: Send + 'static>(bytes: usize, body: impl FnOnce() -> T + Send + 'static) -> T {
        std::thread::Builder::new()
            .stack_size(bytes)
            .spawn(body)
            .expect("spawn bounded-stack thread")
            .join()
            .expect("bounded-stack thread must not overflow")
    }

    /// A chain of `depth` nested spaces below the root, built directly.
    ///
    /// `analyze` is the more faithful fixture and the deep tests below
    /// use it, but only to a depth the remaining quadratic ancestor walks
    /// (#1062) make affordable. This builds the same shape for free, so
    /// the stack properties can be pinned an order of magnitude deeper
    /// than an analysed fixture could reach in a debug build.
    fn space_chain(depth: usize) -> crate::FuncSpace {
        let leaf = || crate::FuncSpace {
            name: Some("f".to_owned()),
            start_line: 1,
            end_line: 1,
            kind: SpaceKind::Function,
            spaces: Vec::new(),
            metrics: crate::CodeMetrics::default(),
            suppressed: SuppressionScope::default(),
        };
        let mut root = leaf();
        let mut cursor = &mut root;
        for _ in 0..depth {
            cursor.spaces.push(leaf());
            cursor = cursor.spaces.last_mut().expect("just pushed");
        }
        root
    }

    #[test]
    fn deeply_nested_spaces_convert_to_wire_without_stack_overflow() {
        // `From<&spaces::FuncSpace>` walks an explicit work stack: the
        // former `spaces.iter().map(FuncSpace::from).collect()` recursed
        // once per level and aborted the process at roughly 900 levels on
        // a 2 MiB thread — under 100 on this one. Analysed fixture, so
        // the whole `analyze` → `to_wire` pipeline is covered.
        const DEPTH: usize = 2_000;
        let depth = on_stack(TIGHT_STACK, || {
            let space = analyze_nested(DEPTH);
            wire_nesting_depth(&space.to_wire())
        });
        // The file-level `Unit` plus one `Function` space per nested `fn`.
        assert_eq!(depth, DEPTH + 1, "the whole chain must survive conversion");
    }

    #[test]
    fn a_pathologically_deep_space_chain_converts_and_tears_down() {
        // Both `Drop` impls at a depth no recursive teardown survives:
        // the compiler-generated glue overflowed this thread's stack at
        // roughly 4 000 levels, and the compute tree, the wire tree, and
        // the wire tree's own nested `Vec`s all unwind inside it.
        const DEPTH: usize = 100_000;
        let depth = on_stack(TIGHT_STACK, || {
            let space = space_chain(DEPTH);
            wire_nesting_depth(&space.to_wire())
        });
        assert_eq!(depth, DEPTH + 1, "the whole chain must survive conversion");
    }

    #[test]
    fn spaces_deeper_than_the_limit_fail_serialization_rather_than_abort() {
        // The reported symptom: `bca metrics -O json` on ~1 000 nested
        // functions overflowed the stack, and a stack overflow is a
        // `SIGABRT`, not a catchable panic — `bca-web`'s `spawn_blocking`
        // wrapper cannot contain it. It must now be an ordinary error.
        const DEPTH: usize = 2_000;
        let message = on_stack(PRODUCTION_STACK, || {
            let space = analyze_nested(DEPTH);
            serde_json::to_string(&space)
                .expect_err("nesting past the limit must fail, not serialize")
                .to_string()
        });
        assert!(
            message.contains("FuncSpace nesting is deeper than the serialization limit of 128"),
            "the error must name the type and the limit, got: {message}"
        );
    }

    #[test]
    fn space_nesting_at_the_serialize_limit_is_accepted_and_one_deeper_is_not() {
        // `depth` counts non-empty child lists, so `n` nested functions
        // reach depth `n`: the file-level `Unit` down to the last `fn`
        // that still contains another one.
        let (accepted, rejected) = on_stack(PRODUCTION_STACK, || {
            let at_limit = analyze_nested(MAX_SPACE_SERIALIZE_DEPTH);
            let past_limit = analyze_nested(MAX_SPACE_SERIALIZE_DEPTH + 1);
            (
                [
                    serde_json::to_string(&at_limit).is_ok(),
                    serde_yaml::to_string(&at_limit).is_ok(),
                    toml::to_string(&at_limit).is_ok(),
                    {
                        let mut bytes = Vec::new();
                        ciborium::into_writer(&at_limit, &mut bytes).is_ok()
                    },
                ],
                [
                    serde_json::to_string(&past_limit).is_ok(),
                    serde_yaml::to_string(&past_limit).is_ok(),
                    toml::to_string(&past_limit).is_ok(),
                    {
                        let mut bytes = Vec::new();
                        ciborium::into_writer(&past_limit, &mut bytes).is_ok()
                    },
                ],
            )
        });
        assert_eq!(
            accepted, [true; 4],
            "exactly {MAX_SPACE_SERIALIZE_DEPTH} levels must serialize in every format"
        );
        assert_eq!(
            rejected, [false; 4],
            "one level past the limit must be refused in every format"
        );
    }

    #[test]
    fn deeply_nested_ops_convert_and_serialize_without_stack_overflow() {
        // `Ops` mirrors `FuncSpace`'s nesting and had the same recursive
        // `From` and `Serialize`, so it needs the same coverage.
        const DEPTH: usize = 2_000;
        let (converted_depth, message) = on_stack(PRODUCTION_STACK, || {
            let ops = crate::Ast::parse(crate::Source::new(
                crate::LANG::Rust,
                nested_functions(DEPTH).as_bytes(),
            ))
            .expect("nested-function fixture must parse")
            .ops()
            .expect("nested-function fixture must yield ops");
            let wire = Ops::from(&ops);
            let mut deepest = 0;
            let mut stack = vec![(&wire, 1_usize)];
            while let Some((node, depth)) = stack.pop() {
                deepest = deepest.max(depth);
                for child in &node.spaces {
                    stack.push((child, depth + 1));
                }
            }
            let message = serde_json::to_string(&ops)
                .expect_err("nesting past the limit must fail, not serialize")
                .to_string();
            (deepest, message)
        });
        assert_eq!(converted_depth, DEPTH + 1, "the whole chain must convert");
        assert!(
            message.contains("Ops nesting is deeper than the serialization limit of 128"),
            "the error must name the type and the limit, got: {message}"
        );
    }
}
