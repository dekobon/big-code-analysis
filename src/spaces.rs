// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
// Metric counts (token, function, branch, argument, etc.) are stored as
// `usize` and crossed with `f64` averages, ratios, and Halstead scores
// across the cyclomatic / MI / Halstead computations. The `usize as f64`
// and `f64 as usize` casts are intentional and snapshot-anchored — every
// site is bounded by the count it came from. Allowing the lints at the
// module level keeps the metric arithmetic legible.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;
use std::sync::Arc;

use crate::int_hash::IntKeyHashMap;

/// Per-node nesting state that `Cognitive` inherits down the walk.
///
/// A struct rather than a `(usize, usize, usize)` because the three are
/// same-typed and summed symmetrically (`conditional + function_depth +
/// lambda`), so a transposition is invisible at the point of use and
/// surfaces only in the arms that read one field alone. Naming them
/// makes the boundary — where the walk reads and writes this — the
/// place a mix-up is visible.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct Nesting {
    /// Structural nesting from enclosing control constructs.
    pub(crate) conditional: usize,
    /// Count of enclosing functions.
    pub(crate) function_depth: usize,
    /// Count of enclosing lambdas / closures.
    pub(crate) lambda: usize,
}

/// Node-id keyed [`Nesting`] state; see
/// `spaces::compute::propagate_nesting_to_children`.
pub(crate) type NestingMap = IntKeyHashMap<usize, Nesting>;

use crate::langs::LANG;
use crate::metric_set::{Metric, MetricSet};
use crate::preproc::PreprocResults;

use crate::checker::Checker;
use crate::error::MetricsError;
use crate::node::{Ancestors, Cursor, Node};
use crate::suppression::{
    Suppression, SuppressionKind, SuppressionScope, parse_marker as parse_suppression_marker,
};

use crate::abc::{self, Abc};
use crate::cognitive::{self, Cognitive};
use crate::cyclomatic::{self, Cyclomatic};
use crate::getter::Getter;
use crate::halstead::{self, Halstead, HalsteadMaps};
use crate::loc::{self, Loc};
use crate::mi::{self, Mi};
use crate::nargs::{self, NArgs};
use crate::nexits::{self, Exit};
use crate::nom::{self, Nom};
use crate::npa::{self, Npa};
use crate::npm::{self, Npm};
use crate::tokens::{self, Tokens};
use crate::wmc::{self, Wmc};

use crate::traits::*;

mod compute;

// Inherent / trait impl blocks for the public types defined below live
// in per-type child modules to keep `spaces.rs` under its size budget.
// Method and trait resolution is by type, not module path, so every
// public path (`crate::spaces::Ast::parse`, etc.) is preserved.
mod ast;
mod code_metrics;
mod options;
mod source;
mod space_kind;

// `analyze` is `pub` — re-exported from `lib.rs`, so it must stay
// reachable at `crate::spaces::analyze`.
pub use compute::analyze;
// `metrics_inner` and `push_children` are `pub(crate)` — `metrics_inner`
// is re-exported from `lib.rs` and `push_children` is consumed by
// `crate::ops`, so both must stay reachable at their `crate::spaces::`
// paths.
pub(crate) use compute::{metrics_inner, push_children};
// The inline `mod tests` drives `apply_suppression` via
// `super::apply_suppression`; re-import the name into this module
// (test-only, so it does not warn as unused in production builds) to
// keep that path resolving after the move into `compute`.
#[cfg(test)]
use compute::apply_suppression;

/// The list of supported space kinds.
// New space kinds land as languages are added (a future module-, mixin-,
// or enum-style space), so this is marked `#[non_exhaustive]` to keep
// such additions additive rather than a 2.0 break. CLI/web consumers
// matching on it already carry a `_ =>` arm.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum SpaceKind {
    /// An unknown space
    #[default]
    Unknown,
    /// A function space
    Function,
    /// A class space
    Class,
    /// A struct space
    Struct,
    /// A `Rust` trait space
    Trait,
    /// A `Rust` implementation space
    Impl,
    /// A general space
    Unit,
    /// A `C/C++` namespace
    Namespace,
    /// An interface
    Interface,
}

/// All metrics data.
///
/// The set of metrics actually computed is governed by
/// [`MetricsOptions::with_only`]. By default every metric is
/// populated; when `with_only` restricts the set, unselected fields
/// remain at their `Default` value and are elided from
/// `Serialize` output. The `selected` mask is the source of truth
/// for which fields are populated — read it via
/// [`CodeMetrics::selected`].
#[derive(Default, Debug, Clone, PartialEq)]
pub struct CodeMetrics {
    /// `NArgs` data
    pub nargs: nargs::Stats,
    /// `NExits` data
    pub nexits: nexits::Stats,
    /// `Cognitive` data
    pub cognitive: cognitive::Stats,
    /// `Cyclomatic` data
    pub cyclomatic: cyclomatic::Stats,
    /// `Halstead` data
    pub halstead: halstead::Stats,
    /// `Loc` data
    pub loc: loc::Stats,
    /// `Nom` data
    pub nom: nom::Stats,
    /// `Tokens` data
    pub tokens: tokens::Stats,
    /// `Mi` data
    pub mi: mi::Stats,
    /// `Abc` data
    pub abc: abc::Stats,
    /// `Wmc` data
    pub wmc: wmc::Stats,
    /// `Npm` data
    pub npm: npm::Stats,
    /// `Npa` data
    pub npa: npa::Stats,
    /// Change-history (VCS) data for this space.
    ///
    /// Unlike every other field, this is *not* AST-derived and *not*
    /// computed during the analysis walk: it is a signal set injected by
    /// the caller after [`analyze`]. The top-level (file-level)
    /// [`FuncSpace`] carries the per-file block projected from a
    /// [`crate::vcs::HistoryIndex`]; nested function / method / class
    /// spaces carry a per-function block derived from `git blame` only
    /// when the caller opts into per-function attribution
    /// ([`crate::vcs::PerFunctionBlame`], issue #329), and stay `None`
    /// otherwise. Note the two levels use **different** computations:
    /// the file block is windowed added+deleted churn, the per-function
    /// block is current-blame surviving-line attribution, so their
    /// `churn` figures are not comparable. `None` also distinguishes an
    /// untracked file from a tracked one with zero in-window activity.
    /// Gated behind the `vcs-git` backend feature.
    #[cfg(feature = "vcs-git")]
    pub vcs: Option<crate::vcs::Stats>,
    /// Which metrics were actually computed for this space.
    ///
    /// Default is [`MetricSet::all`] — every metric was run, matching
    /// the pre-#257 behaviour. After
    /// [`MetricsOptions::with_only`] the bitfield is restricted to the
    /// caller's selection plus auto-added dependencies.
    ///
    /// The [`Serialize`] impl consults this set to elide fields the
    /// caller did not select. The field itself is not serialized.
    pub selected: MetricSet,
}

/// Function space data.
///
/// `Serialize` is provided in [`crate::wire`] (it delegates to
/// [`crate::wire::FuncSpace`], the single definition of the output shape);
/// read the wire form back with `serde` via that module.
#[derive(Debug, Clone, PartialEq)]
pub struct FuncSpace {
    /// The name of a function space.
    ///
    /// For the top-level (file-level) `FuncSpace`, this is the value
    /// supplied via `Source::name` to [`analyze`] — typically a file
    /// path or other display identifier chosen by the caller. The
    /// library no longer derives this from a `&Path` or applies lossy
    /// UTF-8 conversion; callers are expected to pass an
    /// already-stringified identifier (or `None` if they have no
    /// meaningful name to attach).
    ///
    /// For nested spaces, `None` means an error occurred in parsing the
    /// name of the function space from the AST.
    pub name: Option<String>,
    /// The first line of a function space
    pub start_line: usize,
    /// The last line of a function space
    pub end_line: usize,
    /// The space kind
    pub kind: SpaceKind,
    /// All subspaces contained in a function space
    pub spaces: Vec<FuncSpace>,
    /// All metrics of a function space
    pub metrics: CodeMetrics,
    /// In-source suppression markers that apply to this space.
    ///
    /// Populated during the spaces pass from comment-embedded
    /// directives. Each marker carries a [`SuppressionScope`] naming
    /// the metrics it silences. The top-level (file-level) `FuncSpace`
    /// aggregates every file-scoped marker; nested function spaces
    /// aggregate every function-scoped marker whose comment lies
    /// inside their source range. Metric computation itself is
    /// unaffected — this field is consumed by downstream
    /// *threshold-check* code (e.g. `bca check`), which consults a
    /// [`crate::SuppressionPolicy`] to decide whether to honour the
    /// markers or surface every violation regardless.
    ///
    /// Defaults to `SuppressionScope::default()` (an empty `Some`), so
    /// pre-existing code paths that do not honor suppressions see no
    /// behaviour change. The field is elided from JSON output when
    /// empty so the existing schema is unchanged for files without
    /// markers.
    pub suppressed: SuppressionScope,
}

// Space nesting is caller-controlled, so the compiler-generated `Drop`
// glue would recurse once per level and abort the process on a deep tree
// (#1056). See [`crate::recursion`].
crate::recursion::impl_iterative_drop!(FuncSpace, spaces);

impl FuncSpace {
    /// Project this space into its [`crate::wire::FuncSpace`] form — the
    /// plain, `Deserialize`-capable record that defines the serialized
    /// shape. Serializing a `FuncSpace` produces exactly the same bytes as
    /// serializing `self.to_wire()`.
    #[must_use]
    pub fn to_wire(&self) -> crate::wire::FuncSpace {
        crate::wire::FuncSpace::from(self)
    }

    fn new<'a, T: Getter>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
        kind: SpaceKind,
        selected: MetricSet,
    ) -> Self {
        let (start_position, end_position) = match kind {
            SpaceKind::Unit => {
                if node.child_count() == 0 {
                    (0, 0)
                } else {
                    (node.start_row() + 1, node.end_row())
                }
            }
            _ => (node.start_row() + 1, node.end_row() + 1),
        };

        // The top-level Unit's name is overwritten by `metrics_inner`
        // with the caller-supplied name before returning, so computing
        // it here is wasted work. Other kinds keep the AST-derived name.
        let name = (kind != SpaceKind::Unit)
            .then(|| {
                T::get_func_space_name(node, code, ancestors)
                    .map(|name| name.split_whitespace().collect::<Vec<_>>().join(" "))
            })
            .flatten();

        let mut metrics = CodeMetrics::with_selected(selected);
        // Seed the cyclomatic per-function divisor: each function/closure
        // space contributes 1 to `function_spaces`, which `Stats::merge`
        // then sums across the subtree. Sourced here from the space kind
        // rather than from the `Nom` metric so the cyclomatic averages
        // stay correct even when `Nom` is not selected (#512).
        if kind == SpaceKind::Function {
            metrics.cyclomatic.note_function_space();
        }

        Self {
            name,
            spaces: Vec::new(),
            metrics,
            kind,
            start_line: start_position,
            end_line: end_position,
            suppressed: SuppressionScope::default(),
        }
    }
}

#[derive(Debug, Clone)]
struct State<'a> {
    space: FuncSpace,
    halstead_maps: HalsteadMaps<'a>,
}

/// In-memory source bundle handed to [`analyze`].
///
/// `Source` decouples the *display name* of the top-level
/// [`FuncSpace`] (`Source::name`) from the optional *filesystem path*
/// used by the C++ preprocessor lookup (`Source::preproc_path`). For
/// in-memory snippets, code fetched over the network, or test
/// fixtures, callers pass `Source` directly without manufacturing a
/// `Path`.
///
/// Marked `#[non_exhaustive]` so future input fields can land
/// additively. Downstream callers must construct via
/// [`Source::new`] plus the `with_*` builder setters rather than
/// struct-literal syntax (rustc rejects external struct literals on
/// non-exhaustive types with E0639).
///
/// # Examples
///
/// Analysing an in-memory snippet with no on-disk path:
///
/// ```
/// use big_code_analysis::{analyze, MetricsOptions, Source, LANG};
///
/// let source = Source::new(LANG::Rust, b"fn main() {}")
///     .with_name(Some("snippet.rs".to_owned()));
/// let space = analyze(source, MetricsOptions::default()).unwrap();
/// assert_eq!(space.name.as_deref(), Some("snippet.rs"));
/// // `Source` applies no normalisation: the bytes reach the parser
/// // exactly as given, trailing newline or not, and the one-line
/// // snippet above counts as one source line either way (#1067).
/// assert_eq!(space.metrics.loc.sloc(), 1);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct Source<'a> {
    /// The source language used to select the parser.
    pub(crate) lang: LANG,
    /// Raw source bytes, borrowed ([`Source::new`]) or owned
    /// ([`Source::from_bytes`]). The parser needs an owned buffer:
    /// borrowed bytes are copied at parse time, owned bytes move
    /// through without a copy (the CLI walk's hot path).
    pub(crate) code: Cow<'a, [u8]>,
    /// Display / identifier name for the top-level [`FuncSpace`].
    /// If `None`, the top-level [`FuncSpace::name`] is left `None`.
    pub(crate) name: Option<String>,
    /// Optional path used only by the C++ preprocessor lookup
    /// (`get_fake_code`) to resolve macro definitions in
    /// [`PreprocResults`]. For non-C++ languages this is ignored.
    /// Defaults to `None`.
    pub(crate) preproc_path: Option<&'a Path>,
    /// Preprocessor results paired with `Source::preproc_path`.
    /// Same shape as the `pr` arg on the deprecated entry points.
    pub(crate) preproc: Option<Arc<PreprocResults>>,
}

/// Parse-once, compute-many handle.
///
/// Owns the parsed [`tree_sitter::Tree`] and the source bytes it was parsed
/// from, so callers can run [`Ast::metrics`] repeatedly against the same
/// parse — with different [`MetricsOptions`] subsets, interleaved with
/// custom `tree_sitter` traversal via [`Ast::as_tree_sitter`], or cached
/// across configuration changes in an analysis pipeline.
///
/// Build one via [`Ast::parse`] (the seam behind [`analyze`]) or
/// [`Ast::from_tree_sitter`] to reuse a caller-supplied
/// [`tree_sitter::Tree`], carrying an explicit display name.
///
/// `Ast` is a snapshot — it does not pick up changes to the source after
/// construction. Incremental reparse via [`tree_sitter::InputEdit`] is out
/// of scope for this seam.
///
/// # C++ preprocessor
///
/// When [`Ast::parse`] is given a [`Source`] carrying preprocessor inputs
/// and the language is [`LANG::Cpp`], [`Ast::source`] returns the *expanded*
/// bytes the parser actually saw (the macro pre-pass runs before
/// `tree-sitter` does). [`Ast::from_tree_sitter`] adopts whatever tree the
/// caller supplied; whatever expansion they applied before building it is
/// what [`Ast::source`] reflects.
///
/// # Examples
///
/// Parse once, run two disjoint metric subsets without re-parsing:
///
/// ```
/// use big_code_analysis::{Ast, LANG, Metric, MetricsOptions, Source};
///
/// let ast = Ast::parse(
///     Source::new(LANG::Rust, b"fn f() { if true { 1 } else { 2 }; }"),
/// )
/// .expect("rust feature enabled");
///
/// let loc = ast
///     .metrics(MetricsOptions::default().with_only(&[Metric::Loc]))
///     .expect("walker succeeds");
/// let cyc = ast
///     .metrics(MetricsOptions::default().with_only(&[Metric::Cyclomatic]))
///     .expect("walker succeeds");
/// // Each call's `with_only` filters to its requested family — the other
/// // metric stays at its `Default` (zero) value, confirming options are
/// // honored per call rather than carried over.
/// assert!(loc.metrics.loc.ploc() > 0);
/// assert_eq!(loc.metrics.cyclomatic.cyclomatic_sum(), 0);
/// assert!(cyc.metrics.cyclomatic.cyclomatic_sum() > 0);
/// assert_eq!(cyc.metrics.loc.ploc(), 0);
/// ```
///
/// Walk the underlying `tree_sitter::Tree` and then run metrics on the
/// same parse:
///
/// ```
/// use big_code_analysis::{Ast, LANG, MetricsOptions, Source};
///
/// let ast = Ast::parse(Source::new(LANG::Rust, b"fn f() {}"))
///     .expect("rust feature enabled");
/// let root = ast.as_tree_sitter().root_node();
/// assert_eq!(root.kind(), "source_file");
/// let _ = ast.metrics(MetricsOptions::default()).expect("walker succeeds");
/// ```
pub struct Ast {
    inner: crate::langs::AstInner,
    name: Option<String>,
}

// `impl fmt::Debug for Ast` and `impl Ast` live in `spaces/ast.rs`.

/// Per-traversal options for [`analyze`] / [`Ast::metrics`].
///
/// Marked `#[non_exhaustive]` so future option fields can land
/// additively. Downstream callers must construct via the builder
/// methods rather than struct-literal syntax (rustc rejects external
/// struct literals on non-exhaustive types with E0639, including the
/// `..Default::default()` spread form). The defaults preserve every
/// metric value emitted by the pre-#182 [`analyze`] entry point.
///
/// ```
/// use big_code_analysis::MetricsOptions;
/// let opts = MetricsOptions::default().with_exclude_tests(true);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MetricsOptions {
    /// When true, the traversal asks the language module to skip
    /// test-only subtrees (e.g. Rust `#[test]` / `#[cfg(test)]`
    /// functions and modules). Only languages that override the
    /// internal `should_skip_subtree` hook honor this; others ignore
    /// the flag.
    pub(crate) exclude_tests: bool,
    /// Which metrics to compute. Defaults to [`MetricSet::all`] —
    /// every metric is enabled, matching the pre-#257 behaviour.
    /// Restrict via [`MetricsOptions::with_only`].
    pub(crate) metrics: MetricSet,
    /// When true (the default), Rust's `?` operator (the
    /// `try_expression` grammar node) contributes `+1` to both
    /// standard and modified cyclomatic complexity, matching upstream
    /// rust-code-analysis. Set to `false` (via
    /// [`MetricsOptions::with_count_cyclomatic_try`]) to treat `?` as
    /// linear error propagation rather than a branch — useful when
    /// cyclomatic is used as a maintainability gate that should not
    /// penalize fallible-but-linear code. Rust-only: no other
    /// language emits `try_expression`, so the flag is inert
    /// elsewhere. Defaulting to `true` keeps every published metric
    /// value unchanged (#409).
    pub(crate) count_cyclomatic_try: bool,
}

#[cfg(test)]
// The lossy-path / synthetic-Unit tests below drive the internal
// `metrics_inner` walk core directly (the `Ast`-seam-friendly
// counterpart of the retired path-positional entry points) so they
// keep regression coverage on the synthetic top-level Unit and the
// lossy-name handling.
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]
#[path = "spaces_tests.rs"]
mod tests;
