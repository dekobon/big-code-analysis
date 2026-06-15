// bca: suppress-file(halstead, loc, nargs, nom)
// This file is a flat catalogue of plain DTO structs plus mechanical
// `From<&Compute>` field-by-field projections — one struct and one
// projection per metric and container. The offenders are volume /
// field-count / many-fn aggregation artifacts (the same shape as `ops.rs`),
// not per-function logic complexity; there are no conditionals or loops
// here. cognitive / cyclomatic stay enforced (and do not fire).

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
//! for the recursive `FuncSpace`/`Ops` trees). This is the deliberate cost
//! of a single source of truth that also round-trips: a borrowing
//! serialize-only mirror would double the struct set and could not derive
//! `Deserialize`. Serialization runs once per file and the projection is
//! dropped immediately, so it is not on a tight inner loop.
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

// ---------------------------------------------------------------------------
// Per-metric wire structs. Field order mirrors each compute type's
// `Serialize` field order exactly so the emitted document is byte-identical.
// ---------------------------------------------------------------------------

/// Wire form of the `Abc` metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Abc {
    /// Sum of assignments across the space.
    pub assignments: u64,
    /// Sum of branches across the space.
    pub branches: u64,
    /// Sum of conditions across the space.
    pub conditions: u64,
    /// Euclidean ABC magnitude.
    #[serde(default = "nan_default", with = "non_finite")]
    pub magnitude: f64,
    /// Average assignments per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub assignments_average: f64,
    /// Average branches per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub branches_average: f64,
    /// Average conditions per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub conditions_average: f64,
    /// Minimum assignments in a single space.
    pub assignments_min: u64,
    /// Maximum assignments in a single space.
    pub assignments_max: u64,
    /// Minimum branches in a single space.
    pub branches_min: u64,
    /// Maximum branches in a single space.
    pub branches_max: u64,
    /// Minimum conditions in a single space.
    pub conditions_min: u64,
    /// Maximum conditions in a single space.
    pub conditions_max: u64,
}

impl From<&abc::Stats> for Abc {
    fn from(s: &abc::Stats) -> Self {
        Self {
            assignments: s.assignments_sum(),
            branches: s.branches_sum(),
            conditions: s.conditions_sum(),
            magnitude: s.magnitude_sum(),
            assignments_average: s.assignments_average(),
            branches_average: s.branches_average(),
            conditions_average: s.conditions_average(),
            assignments_min: s.assignments_min(),
            assignments_max: s.assignments_max(),
            branches_min: s.branches_min(),
            branches_max: s.branches_max(),
            conditions_min: s.conditions_min(),
            conditions_max: s.conditions_max(),
        }
    }
}

/// Wire form of the `Cognitive` metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cognitive {
    /// Cognitive-complexity sum across the space.
    pub sum: u64,
    /// Average cognitive complexity per function.
    #[serde(default = "nan_default", with = "non_finite")]
    pub average: f64,
    /// Minimum cognitive complexity in a single function.
    pub min: u64,
    /// Maximum cognitive complexity in a single function.
    pub max: u64,
}

impl From<&cognitive::Stats> for Cognitive {
    fn from(s: &cognitive::Stats) -> Self {
        Self {
            sum: s.cognitive_sum(),
            average: s.cognitive_average(),
            min: s.cognitive_min(),
            max: s.cognitive_max(),
        }
    }
}

/// Wire form of the modified-cyclomatic sub-record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CyclomaticModified {
    /// Modified-cyclomatic sum across the space.
    pub sum: u64,
    /// Average modified cyclomatic complexity per function.
    #[serde(default = "nan_default", with = "non_finite")]
    pub average: f64,
    /// Minimum modified cyclomatic complexity in a single function.
    pub min: u64,
    /// Maximum modified cyclomatic complexity in a single function.
    pub max: u64,
}

/// Wire form of the `Cyclomatic` metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cyclomatic {
    /// Cyclomatic-complexity sum across the space.
    pub sum: u64,
    /// Average cyclomatic complexity per function.
    #[serde(default = "nan_default", with = "non_finite")]
    pub average: f64,
    /// Minimum cyclomatic complexity in a single function.
    pub min: u64,
    /// Maximum cyclomatic complexity in a single function.
    pub max: u64,
    /// The modified-cyclomatic projection.
    pub modified: CyclomaticModified,
}

impl From<&cyclomatic::Stats> for Cyclomatic {
    fn from(s: &cyclomatic::Stats) -> Self {
        Self {
            sum: s.cyclomatic_sum(),
            average: s.cyclomatic_average(),
            min: s.cyclomatic_min(),
            max: s.cyclomatic_max(),
            modified: CyclomaticModified {
                sum: s.cyclomatic_modified_sum(),
                average: s.cyclomatic_modified_average(),
                min: s.cyclomatic_modified_min(),
                max: s.cyclomatic_modified_max(),
            },
        }
    }
}

/// Wire form of the `Nexits` (exit-points) metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Nexits {
    /// Exit-point sum across the space.
    pub sum: u64,
    /// Average exit points per function.
    #[serde(default = "nan_default", with = "non_finite")]
    pub average: f64,
    /// Minimum exit points in a single function.
    pub min: u64,
    /// Maximum exit points in a single function.
    pub max: u64,
}

impl From<&nexits::Stats> for Nexits {
    fn from(s: &nexits::Stats) -> Self {
        Self {
            sum: s.nexits_sum(),
            average: s.nexits_average(),
            min: s.nexits_min(),
            max: s.nexits_max(),
        }
    }
}

/// Wire form of the `Halstead` metric suite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Halstead {
    /// Number of distinct operators (`n1`).
    pub unique_operators: u64,
    /// Total operator occurrences (`N1`).
    pub total_operators: u64,
    /// Number of distinct operands (`n2`).
    pub unique_operands: u64,
    /// Total operand occurrences (`N2`).
    pub total_operands: u64,
    /// Program length (`N = N1 + N2`).
    pub length: u64,
    /// Estimated program length.
    #[serde(default = "nan_default", with = "non_finite")]
    pub estimated_program_length: f64,
    /// Purity ratio (estimated length / length).
    #[serde(default = "nan_default", with = "non_finite")]
    pub purity_ratio: f64,
    /// Program vocabulary (`n = n1 + n2`).
    pub vocabulary: u64,
    /// Program volume.
    #[serde(default = "nan_default", with = "non_finite")]
    pub volume: f64,
    /// Difficulty.
    #[serde(default = "nan_default", with = "non_finite")]
    pub difficulty: f64,
    /// Program level (inverse difficulty).
    #[serde(default = "nan_default", with = "non_finite")]
    pub level: f64,
    /// Effort.
    #[serde(default = "nan_default", with = "non_finite")]
    pub effort: f64,
    /// Estimated time to program (seconds).
    #[serde(default = "nan_default", with = "non_finite")]
    pub time: f64,
    /// Estimated number of delivered bugs.
    #[serde(default = "nan_default", with = "non_finite")]
    pub bugs: f64,
}

impl From<&halstead::Stats> for Halstead {
    fn from(s: &halstead::Stats) -> Self {
        Self {
            unique_operators: s.unique_operators(),
            total_operators: s.total_operators(),
            unique_operands: s.unique_operands(),
            total_operands: s.total_operands(),
            length: s.length(),
            estimated_program_length: s.estimated_program_length(),
            purity_ratio: s.purity_ratio(),
            vocabulary: s.vocabulary(),
            volume: s.volume(),
            difficulty: s.difficulty(),
            level: s.level(),
            effort: s.effort(),
            time: s.time(),
            bugs: s.bugs(),
        }
    }
}

/// Wire form of the `Loc` metric suite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Loc {
    /// Source lines of code.
    pub sloc: u64,
    /// Physical lines of code.
    pub ploc: u64,
    /// Logical lines of code.
    pub lloc: u64,
    /// Comment lines of code.
    pub cloc: u64,
    /// Blank lines.
    pub blank: u64,
    /// Average SLOC per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub sloc_average: f64,
    /// Average PLOC per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub ploc_average: f64,
    /// Average LLOC per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub lloc_average: f64,
    /// Average CLOC per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub cloc_average: f64,
    /// Average blank lines per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub blank_average: f64,
    /// Minimum SLOC in a single space.
    pub sloc_min: u64,
    /// Maximum SLOC in a single space.
    pub sloc_max: u64,
    /// Minimum CLOC in a single space.
    pub cloc_min: u64,
    /// Maximum CLOC in a single space.
    pub cloc_max: u64,
    /// Minimum PLOC in a single space.
    pub ploc_min: u64,
    /// Maximum PLOC in a single space.
    pub ploc_max: u64,
    /// Minimum LLOC in a single space.
    pub lloc_min: u64,
    /// Maximum LLOC in a single space.
    pub lloc_max: u64,
    /// Minimum blank lines in a single space.
    pub blank_min: u64,
    /// Maximum blank lines in a single space.
    pub blank_max: u64,
}

impl From<&loc::Stats> for Loc {
    fn from(s: &loc::Stats) -> Self {
        Self {
            sloc: s.sloc(),
            ploc: s.ploc(),
            lloc: s.lloc(),
            cloc: s.cloc(),
            blank: s.blank(),
            sloc_average: s.sloc_average(),
            ploc_average: s.ploc_average(),
            lloc_average: s.lloc_average(),
            cloc_average: s.cloc_average(),
            blank_average: s.blank_average(),
            sloc_min: s.sloc_min(),
            sloc_max: s.sloc_max(),
            cloc_min: s.cloc_min(),
            cloc_max: s.cloc_max(),
            ploc_min: s.ploc_min(),
            ploc_max: s.ploc_max(),
            lloc_min: s.lloc_min(),
            lloc_max: s.lloc_max(),
            blank_min: s.blank_min(),
            blank_max: s.blank_max(),
        }
    }
}

/// Wire form of the `Mi` (maintainability index) metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mi {
    /// Original maintainability index.
    #[serde(default = "nan_default", with = "non_finite")]
    pub original: f64,
    /// SEI-derivative maintainability index.
    #[serde(default = "nan_default", with = "non_finite")]
    pub sei: f64,
    /// Visual Studio-derivative maintainability index.
    #[serde(default = "nan_default", with = "non_finite")]
    pub visual_studio: f64,
}

impl From<&mi::Stats> for Mi {
    fn from(s: &mi::Stats) -> Self {
        Self {
            original: s.original(),
            sei: s.sei(),
            visual_studio: s.visual_studio(),
        }
    }
}

/// Wire form of the `NArgs` metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Nargs {
    /// Sum of function arguments.
    pub function_args: u64,
    /// Sum of closure arguments.
    pub closure_args: u64,
    /// Average function arguments per function.
    #[serde(default = "nan_default", with = "non_finite")]
    pub function_args_average: f64,
    /// Average closure arguments per closure.
    #[serde(default = "nan_default", with = "non_finite")]
    pub closure_args_average: f64,
    /// Total arguments (functions + closures).
    pub total: u64,
    /// Average arguments per function/closure.
    #[serde(default = "nan_default", with = "non_finite")]
    pub average: f64,
    /// Minimum function arguments in a single function.
    pub function_args_min: u64,
    /// Maximum function arguments in a single function.
    pub function_args_max: u64,
    /// Minimum closure arguments in a single closure.
    pub closure_args_min: u64,
    /// Maximum closure arguments in a single closure.
    pub closure_args_max: u64,
}

impl From<&nargs::Stats> for Nargs {
    fn from(s: &nargs::Stats) -> Self {
        Self {
            function_args: s.function_args_sum(),
            closure_args: s.closure_args_sum(),
            function_args_average: s.function_args_average(),
            closure_args_average: s.closure_args_average(),
            total: s.total(),
            average: s.average(),
            function_args_min: s.function_args_min(),
            function_args_max: s.function_args_max(),
            closure_args_min: s.closure_args_min(),
            closure_args_max: s.closure_args_max(),
        }
    }
}

/// Wire form of the `Nom` (number-of-methods) metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Nom {
    /// Sum of function definitions.
    pub functions: u64,
    /// Sum of closures.
    pub closures: u64,
    /// Average function definitions per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub functions_average: f64,
    /// Average closures per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub closures_average: f64,
    /// Total functions + closures.
    pub total: u64,
    /// Average functions + closures per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub average: f64,
    /// Minimum function definitions in a single space.
    pub functions_min: u64,
    /// Maximum function definitions in a single space.
    pub functions_max: u64,
    /// Minimum closures in a single space.
    pub closures_min: u64,
    /// Maximum closures in a single space.
    pub closures_max: u64,
}

impl From<&nom::Stats> for Nom {
    fn from(s: &nom::Stats) -> Self {
        Self {
            functions: s.functions_sum(),
            closures: s.closures_sum(),
            functions_average: s.functions_average(),
            closures_average: s.closures_average(),
            total: s.total(),
            average: s.average(),
            functions_min: s.functions_min(),
            functions_max: s.functions_max(),
            closures_min: s.closures_min(),
            closures_max: s.closures_max(),
        }
    }
}

/// Wire form of the `Npa` (number-of-public-attributes) metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Npa {
    /// Sum of public attributes across classes.
    pub class_npa_sum: u64,
    /// Sum of public attributes across interfaces.
    pub interface_npa_sum: u64,
    /// Sum of all class attributes.
    pub class_attributes: u64,
    /// Sum of all interface attributes.
    pub interface_attributes: u64,
    /// Class data accessibility ratio.
    #[serde(default = "nan_default", with = "non_finite")]
    pub class_cda: f64,
    /// Interface data accessibility ratio.
    #[serde(default = "nan_default", with = "non_finite")]
    pub interface_cda: f64,
    /// Total public attributes.
    pub total: u64,
    /// Total attributes.
    pub total_attributes: u64,
    /// Overall data accessibility ratio.
    #[serde(default = "nan_default", with = "non_finite")]
    pub cda: f64,
}

impl From<&npa::Stats> for Npa {
    fn from(s: &npa::Stats) -> Self {
        Self {
            class_npa_sum: s.class_npa_sum(),
            interface_npa_sum: s.interface_npa_sum(),
            class_attributes: s.class_na_sum(),
            interface_attributes: s.interface_na_sum(),
            class_cda: s.class_cda(),
            interface_cda: s.interface_cda(),
            total: s.total_npa(),
            total_attributes: s.total_na(),
            cda: s.total_cda(),
        }
    }
}

/// Wire form of the `Npm` (number-of-public-methods) metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Npm {
    /// Sum of public methods across classes.
    pub class_npm_sum: u64,
    /// Sum of public methods across interfaces.
    pub interface_npm_sum: u64,
    /// Sum of all class methods.
    pub class_methods: u64,
    /// Sum of all interface methods.
    pub interface_methods: u64,
    /// Class operation accessibility ratio.
    #[serde(default = "nan_default", with = "non_finite")]
    pub class_coa: f64,
    /// Interface operation accessibility ratio.
    #[serde(default = "nan_default", with = "non_finite")]
    pub interface_coa: f64,
    /// Total public methods.
    pub total: u64,
    /// Total methods.
    pub total_methods: u64,
    /// Overall operation accessibility ratio.
    #[serde(default = "nan_default", with = "non_finite")]
    pub coa: f64,
}

impl From<&npm::Stats> for Npm {
    fn from(s: &npm::Stats) -> Self {
        Self {
            class_npm_sum: s.class_npm_sum(),
            interface_npm_sum: s.interface_npm_sum(),
            class_methods: s.class_nm_sum(),
            interface_methods: s.interface_nm_sum(),
            class_coa: s.class_coa(),
            interface_coa: s.interface_coa(),
            total: s.total_npm(),
            total_methods: s.total_nm(),
            coa: s.total_coa(),
        }
    }
}

/// Wire form of the `Tokens` metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tokens {
    /// Token-count sum across the space.
    pub tokens: u64,
    /// Average tokens per space.
    #[serde(default = "nan_default", with = "non_finite")]
    pub average: f64,
    /// Minimum tokens in a single space.
    pub min: u64,
    /// Maximum tokens in a single space.
    pub max: u64,
}

impl From<&tokens::Stats> for Tokens {
    fn from(s: &tokens::Stats) -> Self {
        Self {
            tokens: s.tokens_sum(),
            average: s.tokens_average(),
            min: s.tokens_min(),
            max: s.tokens_max(),
        }
    }
}

/// Wire form of the `Wmc` (weighted-methods-per-class) metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Wmc {
    /// Sum of weighted methods across classes.
    pub class_wmc_sum: u64,
    /// Sum of weighted methods across interfaces.
    pub interface_wmc_sum: u64,
    /// Total weighted methods.
    pub total: u64,
}

impl From<&wmc::Stats> for Wmc {
    fn from(s: &wmc::Stats) -> Self {
        Self {
            class_wmc_sum: s.class_wmc_sum(),
            interface_wmc_sum: s.interface_wmc_sum(),
            total: s.total_wmc(),
        }
    }
}

// ---------------------------------------------------------------------------
// Container wire structs.
// ---------------------------------------------------------------------------

/// Wire form of [`crate::vcs::Stats`] — per-file change-history metrics.
///
/// Always-slim by design (issue #635): the row carries only the metrics
/// that vary per file. The four constant stamps that hold across every
/// row of a single response — `vcs_schema_version`, `risk_score_version`,
/// `long_window_days`, `recent_window_days` — live exactly once in the
/// enclosing envelope (`bca vcs`'s `Report`, `POST /vcs`'s response, and
/// the `/vcs/trend` document), never repeated per row or per trend point.
///
/// The remaining field names are the nested `vcs` object's output keys
/// verbatim (issue #684). All scores are ordinal. `hotspot_score` and
/// `author_ids` are elided when absent (no AST metrics alongside, and
/// `--emit-author-details` off, respectively). Gated behind the
/// `vcs-git` backend feature.
#[cfg(feature = "vcs-git")]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Vcs {
    /// Distinct commits in the long window.
    pub commits_long: u32,
    /// Distinct commits in the recent window.
    pub commits_recent: u32,
    /// Σ(added + deleted) lines in the long window.
    pub churn_long: u64,
    /// Σ(added + deleted) lines in the recent window.
    pub churn_recent: u64,
    /// Distinct authors in the long window.
    pub authors_long: u32,
    /// Distinct authors in the recent window.
    pub authors_recent: u32,
    /// Top-author edit share in `[0, 1]`.
    #[serde(default = "nan_default", with = "non_finite")]
    pub ownership_top_share: f64,
    /// `commits_recent / commits_long`, clamped to `[0, 1]`.
    #[serde(default = "nan_default", with = "non_finite")]
    pub burst: f64,
    /// Long-window bug-fix commit count.
    pub bug_fix_commits: u32,
    /// Long-window security-fix commit count.
    pub security_fix_commits: u32,
    /// Long-window revert commit count.
    pub revert_commits: u32,
    /// Days since the file's first in-window commit (capped at window).
    pub age_days: u32,
    /// Days since the file's most recent in-window commit.
    pub last_modified_days: u32,
    /// Change entropy (bits) over the long window — Hassan 2009 History
    /// Complexity Metric; higher means more scattered changes.
    #[serde(default = "nan_default", with = "non_finite")]
    pub change_entropy_long: f64,
    /// Change entropy (bits) over the recent window.
    #[serde(default = "nan_default", with = "non_finite")]
    pub change_entropy_recent: f64,
    /// Co-change graph entropy (bits) over the long window — arXiv
    /// 2504.18511; higher means changes ripple across more partners.
    /// `0.0` is computed (no co-changes), not missing.
    #[serde(default = "nan_default", with = "non_finite")]
    pub cochange_entropy_long: f64,
    /// Co-change graph entropy (bits) over the recent window.
    #[serde(default = "nan_default", with = "non_finite")]
    pub cochange_entropy_recent: f64,
    /// Ordinal composite risk score.
    #[serde(default = "nan_default", with = "non_finite")]
    pub risk_score: f64,
    /// Complexity × recent-churn hotspot score, when AST metrics were
    /// computed alongside the history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotspot_score: Option<f64>,
    /// SHA-256-hashed canonical author identities, under
    /// `--emit-author-details`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_ids: Option<Vec<String>>,
}

#[cfg(feature = "vcs-git")]
impl From<&crate::vcs::Stats> for Vcs {
    fn from(s: &crate::vcs::Stats) -> Self {
        Self {
            commits_long: s.commits_long,
            commits_recent: s.commits_recent,
            churn_long: s.churn_long,
            churn_recent: s.churn_recent,
            authors_long: s.authors_long,
            authors_recent: s.authors_recent,
            ownership_top_share: s.ownership_top_share,
            burst: s.burst,
            bug_fix_commits: s.bug_fix_commits,
            security_fix_commits: s.security_fix_commits,
            revert_commits: s.revert_commits,
            age_days: s.age_days,
            last_modified_days: s.last_modified_days,
            change_entropy_long: s.change_entropy_long,
            change_entropy_recent: s.change_entropy_recent,
            cochange_entropy_long: s.cochange_entropy_long,
            cochange_entropy_recent: s.cochange_entropy_recent,
            risk_score: s.risk_score,
            hotspot_score: s.hotspot_score,
            author_ids: s.author_ids.clone(),
        }
    }
}

/// Wire form of one ranked file in a [`VcsReport`]: its repository-
/// relative path plus the always-slim [`Vcs`] block, nested under a
/// `vcs` key like every other metric group (issue #684).
// Serialize-only (no `Deserialize`): `VcsAggregate` and its bus-factor
// family are serialize-only upstream, so `VcsReport` cannot round-trip
// through `Deserialize` the way [`Vcs`] / [`VcsTrend`] do. The only
// consumer (`bca vcs` / `POST /vcs` / Python `vcs.rank()`) serializes
// outward; nothing reads a report back in.
#[cfg(feature = "vcs-git")]
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VcsReportFile {
    /// Repository-relative path.
    pub path: String,
    /// The file's change-history metrics.
    pub vcs: Vcs,
}

/// Wire form of the file-ranking change-history report (issue #328) —
/// the single serialized shape shared by `bca vcs`, `POST /vcs`, and the
/// Python `vcs.rank()` (#664).
///
/// The four constant stamps (`long_window_days`, `recent_window_days`,
/// `risk_score_version`, `vcs_schema_version`) sit once at the top level
/// rather than per row (issue #635); each `files` row carries only the
/// per-file metrics under a nested `vcs` key (issue #684). `vcs_aggregate`
/// is the directory-/repo-level bus-factor summary, omitted when not
/// computed.
#[cfg(feature = "vcs-git")]
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VcsReport {
    /// Long window length, in days (constant across rows).
    pub long_window_days: u32,
    /// Recent window length, in days (constant across rows).
    pub recent_window_days: u32,
    /// Composite-formula version.
    pub risk_score_version: u32,
    /// Per-row metric-block shape version.
    pub vcs_schema_version: u32,
    /// Whether the history came from a shallow clone.
    pub truncated_shallow_clone: bool,
    /// Directory-/repo-level bus-factor aggregate, when computed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vcs_aggregate: Option<crate::vcs::VcsAggregate>,
    /// Files ranked by descending `vcs.risk_score`.
    pub files: Vec<VcsReportFile>,
}

/// Wire form of one sampled point in a historical metric trend (issue
/// #333): the sample timestamp plus the file's VCS block at that moment.
/// `as_of` leads; the metrics sit under a nested `vcs` key (issue #684),
/// the same always-slim [`Vcs`] row every other endpoint emits. The four
/// constant stamps are carried once on the enclosing [`VcsTrend`], never
/// repeated per point (issue #635).
#[cfg(feature = "vcs-git")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VcsTrendPoint {
    /// Unix-second timestamp this point was sampled at.
    pub as_of: i64,
    /// The file's change-history metrics at `as_of`.
    pub vcs: Vcs,
}

/// Wire form of one file's risk-score movement across the trend.
#[cfg(feature = "vcs-git")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VcsTrendDelta {
    /// Repository-relative path.
    pub path: String,
    /// Timestamp of the file's earliest present point.
    pub first_as_of: i64,
    /// Timestamp of the file's latest present point.
    pub last_as_of: i64,
    /// `risk_score` at the earliest present point.
    pub first_risk_score: f64,
    /// `risk_score` at the latest present point.
    pub last_risk_score: f64,
    /// `last_risk_score - first_risk_score`; negative means improved.
    pub delta: f64,
}

#[cfg(feature = "vcs-git")]
impl VcsTrendDelta {
    /// Project a compute-side [`crate::vcs::TrendDelta`], dropping a
    /// non-UTF-8 path (which cannot be a JSON key) by returning `None` —
    /// the same path policy the file map uses.
    fn from_delta(d: &crate::vcs::TrendDelta) -> Option<Self> {
        Some(Self {
            path: d.path.to_str()?.to_owned(),
            first_as_of: d.first_as_of,
            last_as_of: d.last_as_of,
            first_risk_score: d.first_risk_score,
            last_risk_score: d.last_risk_score,
            delta: d.delta,
        })
    }
}

/// Wire form of the improving / regressing delta summary.
#[cfg(feature = "vcs-git")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VcsTrendDeltas {
    /// Files whose risk fell, most-improved first.
    pub improved: Vec<VcsTrendDelta>,
    /// Files whose risk rose, most-regressed first.
    pub regressed: Vec<VcsTrendDelta>,
}

/// Wire form of a historical metric trend (issue #333) — the single
/// serialized shape shared by `bca vcs trend`, `POST /vcs/trend`, and the
/// Python `vcs_trend()`.
///
/// `as_of_points` lists the sample timestamps oldest-first; every file's
/// array in `files` aligns to it 1:1, with a `null` element where the file
/// did not exist at that point. `files` is keyed by repository-relative
/// path and ordered lexicographically for deterministic output.
#[cfg(feature = "vcs-git")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VcsTrend {
    /// Trend-document shape version
    /// ([`crate::vcs::TREND_SCHEMA_VERSION`]).
    pub trend_schema_version: u32,
    /// Per-point metric-block shape version.
    pub vcs_schema_version: u32,
    /// Composite-formula version.
    pub risk_score_version: u32,
    /// Long window length, in days (constant across points).
    pub long_window_days: u32,
    /// Recent window length, in days (constant across points).
    pub recent_window_days: u32,
    /// Whether any sampled snapshot came from a shallow clone.
    pub truncated_shallow_clone: bool,
    /// Sample timestamps, oldest-first.
    pub as_of_points: Vec<i64>,
    /// Per-file time series; `null` marks a point where the file was
    /// absent.
    pub files: std::collections::BTreeMap<String, Vec<Option<VcsTrendPoint>>>,
    /// The most-improved / most-regressed files by risk delta.
    pub deltas: VcsTrendDeltas,
}

#[cfg(feature = "vcs-git")]
impl VcsTrend {
    /// Project a compute-side [`crate::vcs::Trend`] into the wire shape,
    /// keeping the `top_files` highest-risk files (by their most-recent
    /// present `risk_score`; `0` keeps all) and the `top_deltas`
    /// strongest movers in each delta list.
    #[must_use]
    pub fn from_trend(trend: &crate::vcs::Trend, top_files: usize, top_deltas: usize) -> Self {
        let as_of_points = trend.as_of_points().to_vec();

        // Rank files by most-recent present risk so `top_files` keeps the
        // currently-riskiest. Reuse the shared `rank_by_risk` so the
        // descending-risk + path tie-break and the `top` truncation match
        // `bca vcs` / `POST /vcs` exactly (a non-UTF-8 path sorts as "" and
        // is dropped below).
        let mut ranked: Vec<(&std::path::PathBuf, &[Option<crate::vcs::Stats>], f64)> = trend
            .iter()
            .map(|(path, points)| (path, points, latest_present_risk(points)))
            .collect();
        crate::vcs::rank_by_risk(&mut ranked, top_files, |entry| {
            (entry.0.to_str().unwrap_or(""), entry.2)
        });

        let files = ranked
            .into_iter()
            .filter_map(|(path, points, _)| {
                // A non-UTF-8 path cannot be a JSON object key; drop it,
                // matching the per-file endpoints' policy.
                let key = path.to_str()?.to_owned();
                let series = points
                    .iter()
                    .zip(&as_of_points)
                    .map(|(stats, &as_of)| {
                        stats.as_ref().map(|s| VcsTrendPoint {
                            as_of,
                            vcs: Vcs::from(s),
                        })
                    })
                    .collect();
                Some((key, series))
            })
            .collect();

        let compute_deltas = trend.deltas(top_deltas);
        let deltas = VcsTrendDeltas {
            improved: compute_deltas
                .improved
                .iter()
                .filter_map(VcsTrendDelta::from_delta)
                .collect(),
            regressed: compute_deltas
                .regressed
                .iter()
                .filter_map(VcsTrendDelta::from_delta)
                .collect(),
        };

        Self {
            trend_schema_version: crate::vcs::TREND_SCHEMA_VERSION,
            vcs_schema_version: crate::vcs::stats::VCS_SCHEMA_VERSION,
            risk_score_version: crate::vcs::score::RISK_SCORE_VERSION,
            long_window_days: trend.long_window_days(),
            recent_window_days: trend.recent_window_days(),
            truncated_shallow_clone: trend.truncated_shallow_clone(),
            as_of_points,
            files,
            deltas,
        }
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
    pub spaces: Vec<FuncSpace>,
    /// The metrics of the space.
    pub metrics: CodeMetrics,
    /// In-source suppression markers applying to the space (elided when
    /// empty, matching the compute type's schema).
    #[serde(default, skip_serializing_if = "SuppressionScope::is_empty")]
    pub suppressed: SuppressionScope,
}

impl From<&crate::spaces::FuncSpace> for FuncSpace {
    fn from(f: &crate::spaces::FuncSpace) -> Self {
        Self {
            name: f.name.clone(),
            start_line: f.start_line,
            end_line: f.end_line,
            kind: f.kind,
            spaces: f.spaces.iter().map(FuncSpace::from).collect(),
            metrics: CodeMetrics::from(&f.metrics),
            suppressed: f.suppressed.clone(),
        }
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
    pub spaces: Vec<Ops>,
    /// The operands in the space.
    pub operands: Vec<String>,
    /// The operators in the space.
    pub operators: Vec<String>,
}

impl From<&ops::Ops> for Ops {
    fn from(o: &ops::Ops) -> Self {
        Self {
            name: o.name.clone(),
            name_was_lossy: o.name_was_lossy,
            start_line: o.start_line,
            end_line: o.end_line,
            kind: o.kind,
            spaces: o.spaces.iter().map(Ops::from).collect(),
            operands: o.operands.clone(),
            operators: o.operators.clone(),
        }
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
serialize_via_wire!(ops::Ops => Ops);
serialize_via_wire!(function::FunctionSpan => FunctionSpan);

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
    use crate::tools::check_func_space;

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
        assert_eq!(m.cognitive.as_ref().unwrap().sum, 3, "unit cognitive.sum");
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
        assert_eq!(
            classify.metrics.cyclomatic.as_ref().unwrap().sum,
            3,
            "classify cyclomatic.sum",
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
}
