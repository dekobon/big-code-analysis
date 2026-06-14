// bca: suppress-file(halstead, loc, nargs, nexits, nom)
// FuncSpace construction helpers plus the `CodeMetrics` serde / `Display`
// impls; the offenders are mechanical-writer and many-fn aggregation
// artifacts, not per-function logic complexity (cognitive/cyclomatic enforced).

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
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;
use std::sync::Arc;

use crate::langs::LANG;
use crate::metric_set::{Metric, MetricSet};
use crate::preproc::PreprocResults;

use crate::checker::Checker;
use crate::error::MetricsError;
use crate::node::{Cursor, Node};
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

impl fmt::Display for SpaceKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = match self {
            SpaceKind::Unknown => "unknown",
            SpaceKind::Function => "function",
            SpaceKind::Class => "class",
            SpaceKind::Struct => "struct",
            SpaceKind::Trait => "trait",
            SpaceKind::Impl => "impl",
            SpaceKind::Unit => "unit",
            SpaceKind::Namespace => "namespace",
            SpaceKind::Interface => "interface",
        };
        write!(f, "{s}")
    }
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

impl CodeMetrics {
    /// Construct a `CodeMetrics` whose `selected` mask is the given
    /// [`MetricSet`]. All metric fields are at their `Default` value;
    /// the walker fills them in for whichever metrics the mask
    /// admits.
    #[inline]
    #[must_use]
    pub fn with_selected(selected: MetricSet) -> Self {
        Self {
            selected,
            ..Self::default()
        }
    }

    /// Returns the set of metrics that were computed for this space.
    #[inline]
    #[must_use]
    pub fn selected(&self) -> MetricSet {
        self.selected
    }
}

impl fmt::Display for CodeMetrics {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "{}", self.nargs)?;
        writeln!(f, "{}", self.nexits)?;
        writeln!(f, "{}", self.cognitive)?;
        writeln!(f, "{}", self.cyclomatic)?;
        writeln!(f, "{}", self.halstead)?;
        writeln!(f, "{}", self.loc)?;
        writeln!(f, "{}", self.nom)?;
        writeln!(f, "{}", self.tokens)?;
        write!(f, "{}", self.mi)
    }
}

impl CodeMetrics {
    /// Project these metrics into their [`crate::wire::CodeMetrics`] form,
    /// eliding metrics not in [`CodeMetrics::selected`] (and disabled
    /// class-only metrics) exactly as the serialized output does.
    #[must_use]
    pub fn to_wire(&self) -> crate::wire::CodeMetrics {
        crate::wire::CodeMetrics::from(self)
    }

    /// Sum each metric component from `other` into `self` in place. Used to
    /// roll nested function-space metrics into their parent space.
    pub fn merge(&mut self, other: &CodeMetrics) {
        self.cognitive.merge(&other.cognitive);
        self.cyclomatic.merge(&other.cyclomatic);
        self.halstead.merge(&other.halstead);
        self.loc.merge(&other.loc);
        self.nom.merge(&other.nom);
        self.tokens.merge(&other.tokens);
        self.mi.merge(&other.mi);
        self.nargs.merge(&other.nargs);
        self.nexits.merge(&other.nexits);
        self.abc.merge(&other.abc);
        self.wmc.merge(&other.wmc);
        self.npm.merge(&other.npm);
        self.npa.merge(&other.npa);
        // Union the selection masks so a parent space's emitted
        // fields are the union of every nested space's selection.
        // In practice every nested space shares the same mask (set
        // once from `MetricsOptions::metrics`), so this is the
        // identity operation; we union rather than assign to keep
        // `merge` correct under future callers that mix
        // independently-built `FuncSpace` values.
        self.selected = self.selected.union(other.selected);
    }
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

impl FuncSpace {
    /// Project this space into its [`crate::wire::FuncSpace`] form — the
    /// plain, `Deserialize`-capable record that defines the serialized
    /// shape. Serializing a `FuncSpace` produces exactly the same bytes as
    /// serializing `self.to_wire()`.
    #[must_use]
    pub fn to_wire(&self) -> crate::wire::FuncSpace {
        crate::wire::FuncSpace::from(self)
    }

    fn new<T: Getter>(node: &Node, code: &[u8], kind: SpaceKind, selected: MetricSet) -> Self {
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
                T::get_func_space_name(node, code)
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

#[inline]
fn compute_halstead_mi_and_wmc<T: ParserTrait>(state: &mut State, selected: MetricSet) {
    if selected.contains(Metric::Halstead) {
        state
            .halstead_maps
            .finalize(&mut state.space.metrics.halstead);
    }
    if selected.contains(Metric::Mi) {
        // `MetricsOptions::with_only` guarantees Mi's dependencies
        // (Loc + Cyclomatic + Halstead) are also selected, so the
        // Stats values feeding into the MI formula here are populated
        // — not the zero defaults that would silently produce a
        // garbage MI score.
        T::Mi::compute(
            &state.space.metrics.loc,
            &state.space.metrics.cyclomatic,
            &state.space.metrics.halstead,
            &mut state.space.metrics.mi,
        );
    }
    if selected.contains(Metric::Wmc) {
        T::Wmc::compute(
            state.space.kind,
            &state.space.metrics.cyclomatic,
            &mut state.space.metrics.wmc,
        );
    }
}

#[inline]
fn compute_averages(state: &mut State, selected: MetricSet) {
    // The per-function averages for Cognitive, Exit, and NArgs divide
    // by counts sourced from `Nom`. `Metric::dependencies` declares
    // `Nom` as a dependency of all three, so `with_only` pulls it into
    // any selection that includes them and these divisors reflect the
    // real function/closure counts. As defense-in-depth, each `average`
    // accessor additionally guards its divisor with `.max(1)`, so even
    // a zero divisor degrades to `sum / 1` rather than `inf`/`NaN`
    // (#428). Compute the divisors once and feed them into each gated
    // finalize.
    let nom_functions = state.space.metrics.nom.functions_sum() as usize;
    let nom_closures = state.space.metrics.nom.closures_sum() as usize;
    let nom_total = state.space.metrics.nom.total() as usize;
    // Cognitive average
    if selected.contains(Metric::Cognitive) {
        state.space.metrics.cognitive.finalize(nom_total);
    }
    // Nexit average
    if selected.contains(Metric::Nexits) {
        state.space.metrics.nexits.finalize(nom_total);
    }
    // Nargs average
    if selected.contains(Metric::Nargs) {
        state
            .space
            .metrics
            .nargs
            .finalize(nom_functions, nom_closures);
    }
}

#[inline]
fn compute_minmax(state: &mut State, selected: MetricSet) {
    if selected.contains(Metric::Cyclomatic) {
        state.space.metrics.cyclomatic.compute_minmax();
    }
    if selected.contains(Metric::Nexits) {
        state.space.metrics.nexits.compute_minmax();
    }
    if selected.contains(Metric::Cognitive) {
        state.space.metrics.cognitive.compute_minmax();
    }
    if selected.contains(Metric::Nargs) {
        state.space.metrics.nargs.compute_minmax();
    }
    if selected.contains(Metric::Nom) {
        state.space.metrics.nom.compute_minmax();
    }
    if selected.contains(Metric::Loc) {
        state.space.metrics.loc.compute_minmax();
    }
    if selected.contains(Metric::Abc) {
        state.space.metrics.abc.compute_minmax();
    }
    if selected.contains(Metric::Tokens) {
        state.space.metrics.tokens.compute_minmax();
    }
}

#[inline]
fn compute_sum(state: &mut State, selected: MetricSet) {
    if selected.contains(Metric::Wmc) {
        state.space.metrics.wmc.compute_sum();
    }
    if selected.contains(Metric::Npm) {
        state.space.metrics.npm.compute_sum();
    }
    if selected.contains(Metric::Npa) {
        state.space.metrics.npa.compute_sum();
    }
}

/// Runs the four per-space finalization passes (min/max, sum, Halstead +
/// MI + WMC, averages) on a single [`State`]. Shared by both the
/// single-element and pop arms of [`finalize`] so the call sequence stays
/// identical in both. The pop arm performs an *additional* Halstead
/// recompute on the parent after merging the child's maps — that extra
/// pass is intentionally left in [`finalize`], not folded in here.
fn finalize_state<T: ParserTrait>(state: &mut State, selected: MetricSet) {
    compute_minmax(state, selected);
    compute_sum(state, selected);
    compute_halstead_mi_and_wmc::<T>(state, selected);
    compute_averages(state, selected);
}

fn finalize<T: ParserTrait>(state_stack: &mut Vec<State>, diff_level: usize, selected: MetricSet) {
    if state_stack.is_empty() {
        return;
    }
    for _ in 0..diff_level {
        if state_stack.len() == 1 {
            let last_state = state_stack
                .last_mut()
                .expect("invariant: state_stack has exactly one element");
            finalize_state::<T>(last_state, selected);
            break;
        }
        let mut state = state_stack
            .pop()
            .expect("invariant: state_stack has more than one element");
        finalize_state::<T>(&mut state, selected);

        let last_state = state_stack
            .last_mut()
            .expect("invariant: state_stack has remaining elements after pop");
        last_state.halstead_maps.merge(&state.halstead_maps);
        compute_halstead_mi_and_wmc::<T>(last_state, selected);

        // Merge function spaces
        last_state.space.metrics.merge(&state.space.metrics);
        last_state.space.spaces.push(state.space);
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

impl<'a> Source<'a> {
    /// Build a `Source` for `lang` and `code` with no name and no
    /// preprocessor inputs. Chain `with_*` setters to attach a
    /// display name or preprocessor results.
    ///
    /// `Source` is `#[non_exhaustive]`, so external callers cannot
    /// use struct-literal syntax — this constructor plus the
    /// builder setters are the supported construction path.
    #[inline]
    #[must_use]
    pub fn new(lang: LANG, code: &'a [u8]) -> Self {
        Self {
            lang,
            code: Cow::Borrowed(code),
            name: None,
            preproc_path: None,
            preproc: None,
        }
    }

    /// Build a `Source` that owns `code`, so [`Ast::parse`] moves the
    /// buffer into the parser instead of copying it. Prefer this over
    /// [`Source::new`] when you already hold an owned `Vec<u8>` (e.g. a
    /// just-read file), which saves one full-buffer copy per parse.
    #[inline]
    #[must_use]
    pub fn from_bytes(lang: LANG, code: Vec<u8>) -> Self {
        Self {
            lang,
            code: Cow::Owned(code),
            name: None,
            preproc_path: None,
            preproc: None,
        }
    }

    /// Builder-style setter for `Source::name`.
    #[inline]
    #[must_use]
    pub fn with_name(mut self, name: Option<String>) -> Self {
        self.name = name;
        self
    }

    /// Builder-style setter for `Source::preproc_path`.
    #[inline]
    #[must_use]
    pub fn with_preproc_path(mut self, preproc_path: Option<&'a Path>) -> Self {
        self.preproc_path = preproc_path;
        self
    }

    /// Builder-style setter for `Source::preproc`.
    #[inline]
    #[must_use]
    pub fn with_preproc(mut self, preproc: Option<Arc<PreprocResults>>) -> Self {
        self.preproc = preproc;
        self
    }
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

impl fmt::Debug for Ast {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The held parser owns a `tree_sitter::Tree` and a `Vec<u8>`;
        // neither has a meaningful `Debug` projection (one is an opaque
        // C handle, the other is raw source). Reporting language + name
        // keeps the public `Ast: Debug` promise without forcing `Debug`
        // onto every per-language `*Code` tag.
        f.debug_struct("Ast")
            .field("language", &self.language())
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Ast {
    /// Parse `source` into a reusable [`Ast`]. Equivalent to the parse half
    /// of [`analyze`]: every [`Ast::metrics`] call on the returned handle
    /// produces the same [`FuncSpace`] as a freshly-issued
    /// `analyze(source, options)` would.
    ///
    /// # Errors
    ///
    /// Returns [`MetricsError::LanguageDisabled`] when the source language's
    /// per-language Cargo feature is not enabled in this build.
    pub fn parse(source: Source<'_>) -> Result<Self, MetricsError> {
        let Source {
            lang,
            code,
            name,
            preproc_path,
            preproc,
        } = source;
        let inner =
            crate::langs::ast_parse_dispatch(lang, code.into_owned(), preproc_path, preproc)?;
        Ok(Self { inner, name })
    }

    /// Adopt a caller-built [`tree_sitter::Tree`], reusing it instead of
    /// running the bundled parser, with `name: Option<String>` carried
    /// end-to-end.
    ///
    /// The supplied `tree` must have been produced from `code` with the
    /// [`tree_sitter::Language`] returned by
    /// [`LANG::tree_sitter_language`] for `lang`; a mismatch is not
    /// `unsafe` but yields nonsensical metric values.
    ///
    /// # Errors
    ///
    /// Returns [`MetricsError::LanguageDisabled`] when `lang`'s
    /// per-language Cargo feature is not enabled in this build.
    pub fn from_tree_sitter(
        lang: LANG,
        tree: tree_sitter::Tree,
        code: Vec<u8>,
        name: Option<String>,
    ) -> Result<Self, MetricsError> {
        let inner = crate::langs::ast_from_tree_dispatch(lang, tree, code)?;
        Ok(Self { inner, name })
    }

    /// Run the metric walker against the held parse. Safe to call
    /// repeatedly — the tree is reused.
    ///
    /// Two `metrics` calls with different [`MetricsOptions::with_only`]
    /// selections walk the tree twice; the savings versus [`analyze`] come
    /// from not re-parsing the source.
    ///
    /// # Errors
    ///
    /// The return type carries [`MetricsError::EmptyRoot`] for forward
    /// compatibility, but the walker always pushes a synthetic top-level
    /// [`SpaceKind::Unit`] [`FuncSpace`] before walking, so this method
    /// does not return `Err` in practice today.
    pub fn metrics(&self, options: MetricsOptions) -> Result<FuncSpace, MetricsError> {
        self.inner.run_metrics(self.name.clone(), options)
    }

    /// Return every operator and operand of each space in the held parse.
    ///
    /// The top-level [`crate::Ops::name`] is the `Source::name` supplied
    /// to [`Ast::parse`] / [`Ast::from_tree_sitter`] — carried explicitly
    /// rather than derived from a filesystem path via lossy UTF-8
    /// conversion, so [`crate::Ops::name_was_lossy`] is never set on this
    /// path. Safe to call repeatedly; the tree is reused.
    ///
    /// # Errors
    ///
    /// The return type carries [`MetricsError::EmptyRoot`] for forward
    /// compatibility, but the walker always pushes a synthetic top-level
    /// space before walking, so this method does not return `Err` in
    /// practice today (see the variant doc).
    ///
    /// # Examples
    ///
    /// ```
    /// use big_code_analysis::{Ast, LANG, Source};
    ///
    /// let ops = Ast::parse(
    ///     Source::new(LANG::Cpp, b"int a = 42;")
    ///         .with_name(Some("foo.c".to_owned())),
    /// )
    /// .expect("cpp feature enabled")
    /// .ops()
    /// .expect("walker succeeds");
    /// assert_eq!(ops.name.as_deref(), Some("foo.c"));
    /// assert!(!ops.name_was_lossy);
    /// ```
    pub fn ops(&self) -> Result<crate::ops::Ops, MetricsError> {
        self.inner.run_ops(self.name.clone())
    }

    /// Source language of the parsed tree.
    #[must_use]
    #[inline]
    pub fn language(&self) -> LANG {
        self.inner.language()
    }

    /// Source bytes the held tree was parsed from. For [`LANG::Cpp`] with
    /// preprocessor inputs supplied to [`Ast::parse`], these are the
    /// *expanded* bytes (see the type-level "C++ preprocessor" note).
    #[must_use]
    #[inline]
    pub fn source(&self) -> &[u8] {
        self.inner.code_bytes()
    }

    /// Display name carried through to [`FuncSpace::name`] by every
    /// [`Ast::metrics`] call.
    #[must_use]
    #[inline]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Borrow the underlying [`tree_sitter::Tree`] for callers that want
    /// to drive their own traversal alongside the metric walker.
    ///
    /// The returned reference is valid only while `self` lives; nodes
    /// obtained from it must be resolved against [`Ast::source`] (the
    /// `tree_sitter::Tree` is lazy and lifetime-bound to that byte
    /// buffer).
    #[must_use]
    #[inline]
    pub fn as_tree_sitter(&self) -> &tree_sitter::Tree {
        self.inner.ts_tree()
    }

    /// Strip non-doc comments from the held parse, returning the source
    /// with those byte ranges removed. `None` when there is nothing to
    /// strip. Safe to call repeatedly; the tree is reused.
    ///
    /// # Examples
    ///
    /// ```
    /// use big_code_analysis::{Ast, LANG, Source};
    ///
    /// let stripped = Ast::parse(Source::new(LANG::Rust, b"// gone\nfn f() {}\n"))
    ///     .expect("rust feature enabled")
    ///     .strip_comments()
    ///     .expect("a comment was present");
    /// assert!(!stripped.windows(2).any(|w| w == b"//"));
    /// ```
    #[must_use]
    pub fn strip_comments(&self) -> Option<Vec<u8>> {
        self.inner.run_strip_comments()
    }

    /// Detect the span of every function in the held parse. Safe to call
    /// repeatedly; the tree is reused.
    ///
    /// # Examples
    ///
    /// ```
    /// use big_code_analysis::{Ast, LANG, Source};
    ///
    /// let funcs = Ast::parse(Source::new(LANG::Rust, b"fn a() {}\nfn b() {}\n"))
    ///     .expect("rust feature enabled")
    ///     .functions();
    /// assert_eq!(funcs.len(), 2);
    /// ```
    #[must_use]
    pub fn functions(&self) -> Vec<crate::FunctionSpan> {
        self.inner.run_functions()
    }

    /// Build the [`AstResponse`](crate::AstResponse) node tree for the held
    /// parse under `cfg`. The `Source`-based counterpart of the deprecated
    /// `AstCallback` dispatch. Safe to call repeatedly; the tree is reused.
    ///
    /// # Examples
    ///
    /// ```
    /// use big_code_analysis::{Ast, AstCfg, LANG, Source};
    ///
    /// let resp = Ast::parse(Source::new(LANG::Rust, b"fn f() {}"))
    ///     .expect("rust feature enabled")
    ///     .dump(AstCfg {
    ///         id: String::new(),
    ///         language: "rust".to_owned(),
    ///         comment: false,
    ///         span: false,
    ///     });
    /// assert_eq!(resp.language, "rust");
    /// assert_eq!(resp.root.expect("root node").r#type, "source_file");
    /// ```
    #[must_use]
    pub fn dump(&self, cfg: crate::AstCfg) -> crate::AstResponse {
        self.inner.run_dump(cfg)
    }

    /// Count `(matching, total)` nodes in the held parse, where a node
    /// matches when its kind is named in `filters` (the same vocabulary
    /// the `bca count` CLI accepts — `all`, `call`, `comment`, `error`,
    /// `string`, `function`, a numeric `kind_id`, or an exact
    /// `node.kind()`). Safe to call repeatedly; the tree is reused.
    ///
    /// # Examples
    ///
    /// ```
    /// use big_code_analysis::{Ast, LANG, Source};
    ///
    /// let (matching, total) = Ast::parse(Source::new(LANG::Rust, b"fn f() {}"))
    ///     .expect("rust feature enabled")
    ///     .count(&["function_item".to_owned()]);
    /// assert_eq!(matching, 1);
    /// assert!(total > matching);
    /// ```
    #[must_use]
    pub fn count(&self, filters: &[String]) -> (usize, usize) {
        self.inner.run_count(filters)
    }

    /// Find every node in the held parse whose kind is named in
    /// `filters`. The returned [`Node`]s borrow the held tree, so they
    /// must be resolved against [`Ast::source`]. Safe to call
    /// repeatedly; the tree is reused.
    ///
    /// # Errors
    ///
    /// Currently infallible; the [`Result`] wrapper is reserved for a
    /// future strict-parsing mode (matching the other `Ast` walkers).
    pub fn find(&self, filters: &[String]) -> Result<Vec<Node<'_>>, MetricsError> {
        self.inner.run_find(filters)
    }

    /// Collect every in-source suppression marker (`// bca: suppress …`)
    /// in the held parse, sorted by line. Safe to call repeatedly; the
    /// tree is reused.
    #[must_use]
    pub fn suppressions(&self) -> Vec<crate::SuppressionMarker> {
        self.inner.run_suppressions()
    }

    /// Borrow the root [`Node`] of the held parse for callers that drive
    /// their own traversal (e.g. rendering an AST dump). Nodes obtained
    /// from it must be resolved against [`Ast::source`].
    #[must_use]
    #[inline]
    pub fn root_node(&self) -> Node<'_> {
        self.inner.root_node()
    }
}

/// Compute every metric for a [`Source`].
///
/// This is the recommended library entry point. It does not conflate
/// the top-level [`FuncSpace::name`] with a filesystem path: callers
/// supply an explicit `Source::name` and an optional
/// `Source::preproc_path` for C++ preprocessor lookup.
///
/// `options` controls per-traversal flags (e.g.
/// `MetricsOptions::default().with_exclude_tests(true)` to elide
/// Rust `#[test]` / `#[cfg(test)]` subtrees).
///
/// # Errors
///
/// The return type carries [`MetricsError::EmptyRoot`] for forward
/// compatibility, but the walker always pushes a synthetic top-level
/// [`SpaceKind::Unit`][crate::SpaceKind] `FuncSpace` before walking,
/// so this function does not return `Err` in practice today (see
/// the variant doc).
///
/// # Examples
///
/// Analysing an in-memory snippet without constructing a `Path`:
///
/// ```
/// use big_code_analysis::{analyze, MetricsOptions, Source, LANG};
///
/// let space = analyze(
///     Source::new(LANG::Rust, b"fn main() { let x = 1 + 2; }")
///         .with_name(Some("snippet.rs".to_owned())),
///     MetricsOptions::default(),
/// )
/// .expect("snippet has a top-level FuncSpace");
/// assert_eq!(space.name.as_deref(), Some("snippet.rs"));
/// ```
pub fn analyze(source: Source<'_>, options: MetricsOptions) -> Result<FuncSpace, MetricsError> {
    Ast::parse(source)?.metrics(options)
}

// Per-node metric dispatch. Each `compute` call is paired with a bit
// check against the caller's selection. The bit tests are cheap
// (single AND-and-compare on the `MetricSet` bitfield) and an
// unselected metric saves both the call overhead and any per-node
// text-slice / token-table work the metric does internally — Halstead
// in particular owns `HalsteadMaps` allocations and is the headline
// cost saving for `with_only(&[Metric::Loc])`. Extracted from
// `metrics_inner` so the walker stays under clippy's 100-line ceiling.
#[inline]
fn compute_per_node<'a, T: ParserTrait>(
    state: &mut State<'a>,
    node: &Node<'a>,
    code: &'a [u8],
    options: MetricsOptions,
    func_space: bool,
    unit: bool,
    nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
) {
    let selected = options.metrics;
    let last = &mut state.space;
    if selected.contains(Metric::Cognitive) {
        T::Cognitive::compute(node, code, &mut last.metrics.cognitive, nesting_map);
    }
    if selected.contains(Metric::Cyclomatic) {
        T::Cyclomatic::compute_with_options(
            node,
            code,
            &mut last.metrics.cyclomatic,
            options.count_cyclomatic_try,
        );
    }
    if selected.contains(Metric::Halstead) {
        T::Halstead::compute(node, code, &mut state.halstead_maps);
    }
    if selected.contains(Metric::Loc) {
        T::Loc::compute(node, &mut last.metrics.loc, func_space, unit);
    }
    if selected.contains(Metric::Nom) {
        T::Nom::compute(node, code, &mut last.metrics.nom);
    }
    if selected.contains(Metric::Tokens) {
        T::Tokens::compute(node, &mut last.metrics.tokens);
    }
    if selected.contains(Metric::Nargs) {
        T::NArgs::compute(node, &mut last.metrics.nargs);
    }
    if selected.contains(Metric::Nexits) {
        T::Exit::compute(node, code, &mut last.metrics.nexits);
    }
    if selected.contains(Metric::Abc) {
        T::Abc::compute(node, code, &mut last.metrics.abc);
    }
    if selected.contains(Metric::Npm) {
        T::Npm::compute(node, code, &mut last.metrics.npm);
    }
    if selected.contains(Metric::Npa) {
        T::Npa::compute(node, code, &mut last.metrics.npa);
    }
}

/// Pushes a synthetic `Unit` root onto the state stack when the grammar
/// hands us a non-`Unit` root.
///
/// Some grammars (e.g. tree-sitter-mozcpp on unparseable input) return a
/// non-Unit root. Wrapping with a synthetic Unit space spanning the whole
/// file keeps the top-level `FuncSpace` upholding the LOC invariant
/// `blank = sloc - ploc - only_comment_lines >= 0`. A `Unit` root needs
/// no wrapper, so nothing is pushed in that case.
fn push_synthetic_unit_root<T: ParserTrait>(
    state_stack: &mut Vec<State>,
    node: &Node,
    code: &[u8],
    selected: MetricSet,
) {
    if T::Getter::get_space_kind_with_code(node, code) != SpaceKind::Unit {
        let mut synthetic = FuncSpace::new::<T::Getter>(node, code, SpaceKind::Unit, selected);
        synthetic
            .metrics
            .loc
            .init_unit_span(node.start_row(), node.end_row());
        state_stack.push(State {
            space: synthetic,
            halstead_maps: HalsteadMaps::new(),
        });
    }
}

/// Scans a comment node for a suppression marker and applies it against
/// `state_stack` immediately.
///
/// Doing this inline during the walk (rather than queueing markers for a
/// post-walk pass keyed on line number) pins each marker to the
/// syntactically nearest enclosing function space — the only frame on the
/// stack that the grammar nested the comment inside. Line-only matching
/// was ambiguous when two sibling functions shared a source line and the
/// first-by-source-order won regardless of which body actually contained
/// the comment (issue #289).
///
/// A malformed marker is logged and dropped (no scope attached) rather
/// than aborting the walk: a typo in one file must not derail a
/// workspace-wide pass, and dropping is the conservative choice — a typo
/// should not accidentally silence anything.
fn apply_comment_suppression<T: ParserTrait>(
    state_stack: &mut Vec<State>,
    node: &Node,
    code: &[u8],
    diagnostic_path: &str,
) {
    if T::Checker::is_comment(node)
        && let Some(text) = node.utf8_text(code)
    {
        match parse_suppression_marker(text) {
            Ok(Some(s)) => apply_suppression(state_stack, &s),
            Ok(None) => {}
            Err(e) => {
                // The `+ 1` converts tree-sitter's 0-based rows to the
                // 1-based line numbers `FuncSpace::start_line` and the
                // rest of this module report.
                eprintln!("warning: {}:{}: {e}", diagnostic_path, node.start_row() + 1);
            }
        }
    }
}

/// Pushes `node`'s direct children onto the traversal `stack`, each tagged
/// with `new_level`.
///
/// The `children.drain(..).rev()` ordering is load-bearing: it makes the
/// LIFO `stack` yield children in source order, which in turn governs
/// line-shared suppression attribution (issue #289). The `children`
/// scratch buffer is drained empty here so callers can reuse its
/// allocation across iterations.
fn push_children<'a>(
    cursor: &mut Cursor<'a>,
    node: &Node<'a>,
    new_level: usize,
    children: &mut Vec<(Node<'a>, usize)>,
    stack: &mut Vec<(Node<'a>, usize)>,
) {
    cursor.reset(node);
    if cursor.goto_first_child() {
        loop {
            children.push((cursor.node(), new_level));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        for child in children.drain(..).rev() {
            stack.push(child);
        }
    }
}

pub(crate) fn metrics_inner<T: ParserTrait>(
    parser: &T,
    name: Option<String>,
    options: MetricsOptions,
) -> Result<FuncSpace, MetricsError> {
    // The suppression-warning diagnostic uses the caller-supplied
    // name when present; otherwise we fall back to a placeholder so
    // the warning still locates the offending line. All path-based
    // shims pass a lossy-stringified path here, matching pre-#254
    // behaviour byte-for-byte.
    let diagnostic_path = name.as_deref().unwrap_or("<input>");
    let selected = options.metrics;
    let code = parser.code();
    let node = parser.root();
    let mut cursor = node.cursor();
    let mut stack = Vec::new();
    let mut children = Vec::new();
    let mut state_stack: Vec<State> = Vec::new();
    let mut last_level = 0;
    // Initialize nesting_map used for storing nesting information for cognitive
    // Three type of nesting info: conditionals, functions and lambdas
    let mut nesting_map = HashMap::<usize, (usize, usize, usize)>::default();
    nesting_map.insert(node.id(), (0, 0, 0));

    // Suppression markers are resolved inline during the walk rather
    // than queued for a post-finalize pass. When we visit a comment
    // node, the active `state_stack` already encodes the comment's
    // syntactic context: the topmost `SpaceKind::Function` entry is
    // the *innermost enclosing function* by construction, with no
    // ambiguity when sibling functions share a source line (issue
    // #289). The root `Unit` state — always at index 0 once the walk
    // has visited the AST root — owns file-scoped markers.

    push_synthetic_unit_root::<T>(&mut state_stack, &node, code, selected);

    stack.push((node, 0));

    while let Some((node, level)) = stack.pop() {
        // Close any spaces left open by a deeper, already-walked subtree
        // before doing anything else with this node. This must run before
        // the test-subtree prune below so that, when we skip a pruned
        // node, `state_stack.last_mut()` is the node's true enclosing
        // space (#722) — not a sibling's still-open function/impl space.
        if level < last_level {
            finalize::<T>(&mut state_stack, last_level - level, selected);
            last_level = level;
        }

        // Prune test-only subtrees before any per-metric work runs.
        // The hook is gated on `exclude_tests` so the default
        // `metrics()` entry point keeps emitting the pre-#182
        // numbers byte-for-byte.
        if options.exclude_tests && T::Checker::should_skip_subtree(&node, code) {
            // `sloc` is span-based, not node-accumulated, so unlike every
            // other loc sub-metric it does not shrink just because we
            // skip the subtree. Record the pruned node's row span on the
            // innermost enclosing func-space so its `sloc` drops in step
            // (#722); `Sloc::merge` then folds that count upward so every
            // enclosing space — including the unit, which feeds MI's SLOC
            // term — drops too, even when the test item is nested in a
            // retained `impl`/`trait`/closure (#741). Gated on the `Loc`
            // selection so deselecting loc keeps the walk's work identical.
            if selected.contains(Metric::Loc)
                && let Some(state) = state_stack.last_mut()
            {
                state
                    .space
                    .metrics
                    .loc
                    .exclude_test_span(node.start_row(), node.end_row());
            }
            continue;
        }

        let func_space = T::Checker::promotes_to_func_space_with_code(&node, code);

        // `kind` is consumed in exactly two places: `FuncSpace::new`
        // (only when `func_space` is true) and the `unit` flag, which
        // flows solely into `Loc::compute` (only when `Loc` is
        // selected). For some languages — notably Elixir, whose
        // `get_space_kind_with_code` runs a per-`Call` source-text
        // keyword scan — this lookup is far from a cheap enum compare,
        // so we skip it entirely when neither consumer is active.
        // When it IS computed it returns the same value as before, so
        // both consumers observe byte-identical results (issue #522).
        let kind = if func_space || selected.contains(Metric::Loc) {
            T::Getter::get_space_kind_with_code(&node, code)
        } else {
            // Unused on this path: `func_space` is false (so
            // `FuncSpace::new` is not called) and `Loc` is deselected
            // (so the `unit` flag below is never read by `Loc::compute`).
            SpaceKind::Unknown
        };
        let unit = kind == SpaceKind::Unit;

        let new_level = if func_space {
            let state = State {
                space: FuncSpace::new::<T::Getter>(&node, code, kind, selected),
                halstead_maps: HalsteadMaps::new(),
            };
            state_stack.push(state);
            last_level = level + 1;
            last_level
        } else {
            level
        };

        // Pin each suppression marker to its innermost enclosing
        // function space (issue #289); see `apply_comment_suppression`.
        apply_comment_suppression::<T>(&mut state_stack, &node, code, diagnostic_path);

        if let Some(state) = state_stack.last_mut() {
            compute_per_node::<T>(
                state,
                &node,
                code,
                options,
                func_space,
                unit,
                &mut nesting_map,
            );
        }

        push_children(&mut cursor, &node, new_level, &mut children, &mut stack);
    }

    finalize::<T>(&mut state_stack, usize::MAX, selected);

    // Reserved error path: `MetricsError::EmptyRoot` is unreachable
    // today because the synthetic Unit push above (and every
    // language's translation_unit / module / source_file being a
    // `func_space`) keeps the state stack non-empty for every input,
    // including empty / whitespace-only / comment-only sources. The
    // `ok_or` is retained so a future walker change that legitimately
    // drains the stack surfaces a distinct error variant rather than
    // panicking or returning a bare `None`. See `MetricsError::EmptyRoot`
    // for the matching variant doc.
    let mut state = state_stack.pop().ok_or(MetricsError::EmptyRoot)?;
    state.space.name = name;
    Ok(state.space)
}

fn apply_suppression(state_stack: &mut [State], suppression: &Suppression) {
    // Both arms ultimately call `merge` on a `FuncSpace::suppressed`;
    // they differ only in *which* frame on the stack to target.
    //
    // - `File`: the topmost `Unit` frame — by construction the root
    //   `state_stack[0]`, but we match on `SpaceKind::Unit` rather
    //   than index 0 so the invariant is runtime-checked. The
    //   synthetic Unit pushed by `metrics_inner` for non-Unit-root
    //   grammars and every translation-unit/module/source-file being
    //   a `func_space` keep `state_stack[0]` populated for every
    //   input; a marker with no Unit frame on the stack would be a
    //   bug elsewhere and is silently dropped rather than landing on
    //   an arbitrary frame.
    // - `Function`: the topmost `SpaceKind::Function` frame — the
    //   syntactically nearest enclosing function body. Class / struct
    //   / trait spaces are skipped so a marker at class scope but
    //   outside any method does not silence thresholds on the entire
    //   class; authors who want class-wide suppression use `bca:
    //   suppress-file` or repeat the marker on each method. A marker
    //   outside every function body finds no `Function` frame and is
    //   silently dropped — the issue's "no enclosing function" rule.
    let target = match suppression.kind {
        SuppressionKind::File => state_stack
            .iter_mut()
            .find(|s| matches!(s.space.kind, SpaceKind::Unit)),
        SuppressionKind::Function => state_stack
            .iter_mut()
            .rev()
            .find(|s| matches!(s.space.kind, SpaceKind::Function)),
    };
    if let Some(state) = target {
        state.space.suppressed.merge(&suppression.scope);
    }
}

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

impl Default for MetricsOptions {
    /// Defaults preserve every metric value emitted by the pre-#182
    /// [`analyze`] entry point: every metric selected, tests
    /// included, and Rust `?` counted toward cyclomatic (#409).
    fn default() -> Self {
        Self {
            exclude_tests: false,
            metrics: MetricSet::default(),
            count_cyclomatic_try: true,
        }
    }
}

impl MetricsOptions {
    /// Builder-style setter for `MetricsOptions::exclude_tests`.
    ///
    /// Provided because `MetricsOptions` is `#[non_exhaustive]` — the
    /// struct-literal form is unavailable to downstream crates, so
    /// external callers chain `MetricsOptions::default()
    /// .with_exclude_tests(true)` instead.
    #[inline]
    #[must_use]
    pub fn with_exclude_tests(mut self, exclude_tests: bool) -> Self {
        self.exclude_tests = exclude_tests;
        self
    }

    /// Builder-style setter for `MetricsOptions::count_cyclomatic_try`.
    ///
    /// Pass `false` to stop Rust's `?` operator from contributing to
    /// cyclomatic complexity (standard and modified). The default is
    /// `true`, which keeps every published metric value unchanged
    /// (#409). Inert for non-Rust languages, none of which emit the
    /// `try_expression` grammar node.
    #[inline]
    #[must_use]
    pub fn with_count_cyclomatic_try(mut self, count: bool) -> Self {
        self.count_cyclomatic_try = count;
        self
    }

    /// Restrict computation to the given metrics. Metrics outside
    /// this set are skipped during the walk; their `Stats` fields on
    /// [`CodeMetrics`] remain at their `Default` value and are
    /// elided from the [`Serialize`] output. Pass an empty slice to
    /// disable every metric (the walker still runs and produces the
    /// space tree, but no metric values are populated).
    ///
    /// # Dependencies
    ///
    /// Derived metrics implicitly pull in the inputs they require:
    ///
    /// - [`Metric::Mi`] adds [`Metric::Loc`], [`Metric::Cyclomatic`],
    ///   [`Metric::Halstead`].
    /// - [`Metric::Wmc`] adds [`Metric::Cyclomatic`] and
    ///   [`Metric::Nom`].
    ///
    /// This auto-resolution is silent: a caller asking for `Mi`
    /// alone gets a populated `Mi` value, not a zero. See
    /// [`Metric::dependencies`] for the source of truth.
    ///
    /// # Examples
    ///
    /// ```
    /// use big_code_analysis::{Metric, MetricsOptions};
    ///
    /// // Compute LoC only.
    /// let _opts = MetricsOptions::default().with_only(&[Metric::Loc]);
    ///
    /// // Compute Mi: Loc + Cyclomatic + Halstead are auto-added.
    /// let _opts = MetricsOptions::default().with_only(&[Metric::Mi]);
    /// ```
    #[inline]
    #[must_use]
    pub fn with_only(mut self, metrics: &[Metric]) -> Self {
        self.metrics = MetricSet::from_slice_with_deps(metrics);
        self
    }

    /// Restrict computation to the metrics in `metrics`, closing the
    /// set under [`Metric::dependencies`] before storing it.
    ///
    /// Like [`MetricsOptions::with_only`], a derived metric pulls in
    /// the inputs it needs: passing `MetricSet::empty().with(Metric::Mi)`
    /// also selects [`Metric::Loc`], [`Metric::Cyclomatic`], and
    /// [`Metric::Halstead`], so the maintainability index is computed
    /// from real inputs rather than zero-valued defaults (#743). The
    /// resolution is idempotent: an already-closed set is stored
    /// unchanged.
    ///
    /// Use this builder when you already hold a [`MetricSet`]; reach
    /// for [`MetricsOptions::with_only`] when you have a `&[Metric]`.
    ///
    /// # Examples
    ///
    /// ```
    /// use big_code_analysis::{Metric, MetricSet, MetricsOptions};
    ///
    /// // `Mi` alone — Loc + Cyclomatic + Halstead are auto-added so the
    /// // resulting MI value is meaningful.
    /// let set = MetricSet::empty().with(Metric::Mi);
    /// let _opts = MetricsOptions::default().with_metric_set(set);
    /// ```
    #[inline]
    #[must_use]
    pub fn with_metric_set(mut self, metrics: MetricSet) -> Self {
        self.metrics = metrics.resolved();
        self
    }
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
mod tests {
    use crate::MetricsOptions;
    use crate::spaces::metrics_inner;
    use crate::{CppParser, ParserTrait, SpaceKind, check_func_space};

    /// `SpaceKind` is `#[non_exhaustive]` (#551); the attribute is a
    /// compile-time forward-compat contract and must not change the
    /// serialized form. Every variant still round-trips through its
    /// lowercase token, and `Display` agrees with serde.
    #[test]
    fn space_kind_non_exhaustive_serde_roundtrip_unchanged() {
        for kind in [
            SpaceKind::Unknown,
            SpaceKind::Function,
            SpaceKind::Class,
            SpaceKind::Struct,
            SpaceKind::Trait,
            SpaceKind::Impl,
            SpaceKind::Unit,
            SpaceKind::Namespace,
            SpaceKind::Interface,
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{kind}\""));
            let back: SpaceKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, kind);
        }
    }

    /// Positive coverage for the C++ function-space predicates on the
    /// only `function_definition` `kind_id` (343) that
    /// `tree-sitter-mozcpp` currently emits. The structural
    /// `FunctionDefinition*` contract for the aliased kind_ids
    /// (489/491/494) that no observed input parses to is documented
    /// at the predicate call sites in `src/checker.rs` and
    /// `src/getter.rs` — see issue #285.
    #[test]
    fn cpp_function_definition_is_classified_as_function() {
        use crate::Cpp;
        use crate::checker::Checker;
        use crate::getter::Getter;
        use crate::langs::CppCode;
        use crate::traits::Search;

        let source = "int the_func(int x) { return x; }\n";
        let path = std::path::PathBuf::from("fd.cc");
        let parser = CppParser::new(source.as_bytes().to_vec(), &path, None);
        let root = parser.root();

        // Walk for any `FunctionDefinition*` variant (FD/FD2/FD3/FD4)
        // so the test stays valid if a future grammar bump starts
        // emitting one of the higher-numbered aliases.
        let fn_node = root
            .first_occurrence(|id| {
                Cpp::FunctionDefinition == id
                    || Cpp::FunctionDefinition2 == id
                    || Cpp::FunctionDefinition3 == id
                    || Cpp::FunctionDefinition4 == id
            })
            .expect("parse must produce a function_definition node");

        assert!(
            CppCode::is_func(&fn_node),
            "is_func must return true for a function_definition"
        );
        assert!(
            CppCode::is_func_space(&fn_node),
            "is_func_space must return true for a function_definition"
        );
        assert_eq!(
            CppCode::get_space_kind(&fn_node),
            SpaceKind::Function,
            "get_space_kind must classify function_definition as Function"
        );
        assert_eq!(
            CppCode::get_func_space_name(&fn_node, source.as_bytes()),
            Some("the_func"),
            "get_func_space_name must extract the declarator identifier"
        );
    }

    #[test]
    fn cpp_scope_resolution_operator() {
        check_func_space::<CppParser, _>(
            "void Foo::bar(){
                return;
            }",
            "foo.cpp",
            |func_space| {
                insta::assert_json_snapshot!(
                    func_space.spaces[0].name,
                    @r###""Foo::bar""###
                );
            },
        );
    }

    /// Regression for issue #80 — when tree-sitter-mozcpp returns a non-Unit
    /// root (e.g. an `ERROR` root for code it cannot fully parse, as
    /// happens for parts of DeepSpeech's KenLM and OpenFst sources), the
    /// top-level `FuncSpace` must still be a `Unit` spanning the whole
    /// file, with `blank >= 0` and `sloc >= ploc`.
    #[test]
    fn cpp_error_root_yields_unit_top_level_space() {
        // This snippet (a chunk of kenlm/lm/model.hh shape) is rejected by
        // tree-sitter-mozcpp as a clean translation_unit and surfaces as an
        // ERROR root node in the parse tree. Verified at the time of writing
        // against tree-sitter-mozcpp 0.20.4.
        let source = "#ifndef A\n\
                      namespace a { namespace b { namespace c {\n\
                      template <class S, class V> class C : publi\n";

        let path = std::path::PathBuf::from("error_root.cc");
        let parser = CppParser::new(source.as_bytes().to_vec(), &path, None);
        // Sanity: the grammar really does fall back to a non-Unit root for
        // this snippet — otherwise the synthetic-Unit code path is not
        // exercised by this test.
        assert!(
            parser.root().as_tree_sitter().is_error(),
            "test premise broken: grammar must yield ERROR root for this snippet"
        );

        let space = metrics_inner(
            &parser,
            path.to_str().map(str::to_owned),
            MetricsOptions::default(),
        )
        .unwrap();

        assert_eq!(
            space.kind,
            SpaceKind::Unit,
            "top-level FuncSpace must be Unit, not {:?}",
            space.kind
        );

        let loc = &space.metrics.loc;
        let sloc = loc.sloc();
        let ploc = loc.ploc();
        let blank = loc.blank();
        let line_count = source.lines().count();

        assert!(
            sloc >= ploc,
            "sloc ({sloc}) must be >= ploc ({ploc}) for the file-level space"
        );
        // `blank` is `u64`, so non-negativity is type-guaranteed; assert the
        // real invariant instead — blank lines cannot exceed source lines, so
        // a saturating-subtraction underflow (#437) cannot inflate the count.
        assert!(blank <= sloc, "blank ({blank}) must be <= sloc ({sloc})");
        assert_eq!(
            sloc as usize, line_count,
            "sloc ({sloc}) should match the file's line count ({line_count})"
        );
    }

    /// Lesson-9 contract (`docs/development/lessons_learned.md` §9,
    /// issue #193): for every supported language, parsing any input —
    /// including malformed or truncated — must yield a file-level
    /// `FuncSpace` whose `kind == SpaceKind::Unit` with `sloc >= ploc`
    /// and `blank >= 0`.
    ///
    /// This helper pins the **contract** at the public API surface
    /// (`metrics()` always returns a `Unit` top-level space). For most
    /// grammars the parse root is already the canonical translation-
    /// unit kind regardless of input, so the synthetic-Unit wrapper
    /// (`src/spaces.rs:~385`) is not actually exercised by tests
    /// using this helper alone. They serve as future-proofing: a
    /// grammar bump that starts promoting an inner kind to root on
    /// partial input would fail here before shipping a non-`Unit`
    /// top-level space to downstream consumers.
    ///
    /// Tests that need to exercise the synthetic-Unit wrapper itself
    /// (i.e., the path triggered by an `ERROR`-root parse) must also
    /// assert `parser.root().as_tree_sitter().is_error()` before calling this
    /// helper. See `cpp_error_root_yields_unit_top_level_space` and
    /// `lua_partial_input_yields_synthetic_unit_wrapper` — those two
    /// are the only tests in the corpus that today exercise the
    /// wrapper path. Issue #220 tracks finding additional per-grammar
    /// fixtures that surface ERROR roots so each language can have
    /// both a contract test and a wrapper-exercising test.
    fn assert_top_level_space_is_unit_contract<P: ParserTrait>(source: &str, filename: &str) {
        let path = std::path::PathBuf::from(filename);
        let parser = P::new(source.as_bytes().to_vec(), &path, None);
        let space = metrics_inner(
            &parser,
            path.to_str().map(str::to_owned),
            MetricsOptions::default(),
        )
        .expect("metrics must yield a top-level space");
        assert_eq!(
            space.kind,
            SpaceKind::Unit,
            "top-level FuncSpace for {filename:?} must be Unit, not {:?}",
            space.kind
        );
        let loc = &space.metrics.loc;
        let sloc = loc.sloc();
        let ploc = loc.ploc();
        let blank = loc.blank();
        assert!(
            sloc >= ploc,
            "sloc ({sloc}) must be >= ploc ({ploc}) for the file-level space of {filename:?}",
        );
        // `blank` is `u64`; non-negativity is type-guaranteed. Assert the
        // real invariant — blank lines cannot exceed source lines (#437).
        assert!(
            blank <= sloc,
            "blank ({blank}) must be <= sloc ({sloc}) for the file-level space of {filename:?}",
        );
    }

    /// Like [`assert_top_level_space_is_unit_contract`] but additionally
    /// asserts the parse root is an `ERROR` node, so the test actually
    /// exercises the synthetic-Unit wrapper in `metrics()` rather than
    /// the contract-only path. Use this for languages where a fixture
    /// is known to make the grammar return ERROR (currently: Lua, C++
    /// via mozcpp).
    fn assert_partial_input_yields_synthetic_unit_wrapper<P: ParserTrait>(
        source: &str,
        filename: &str,
    ) {
        let path = std::path::PathBuf::from(filename);
        let parser = P::new(source.as_bytes().to_vec(), &path, None);
        assert!(
            parser.root().as_tree_sitter().is_error(),
            "test premise broken: grammar must yield ERROR root for {filename:?}",
        );
        assert_top_level_space_is_unit_contract::<P>(source, filename);
    }

    #[test]
    fn python_top_level_space_is_unit_contract() {
        assert_top_level_space_is_unit_contract::<crate::PythonParser>(
            "def foo(x):\n    return x +\n",
            "partial.py",
        );
    }

    #[test]
    fn javascript_top_level_space_is_unit_contract() {
        assert_top_level_space_is_unit_contract::<crate::JavascriptParser>(
            "function foo(x) {\n  return x +\n",
            "partial.js",
        );
    }

    #[test]
    fn mozjs_top_level_space_is_unit_contract() {
        assert_top_level_space_is_unit_contract::<crate::MozjsParser>(
            "function foo(x) {\n  return x +\n",
            "partial.js",
        );
    }

    #[test]
    fn typescript_top_level_space_is_unit_contract() {
        assert_top_level_space_is_unit_contract::<crate::TypescriptParser>(
            "function foo(x: number): number {\n  return x +\n",
            "partial.ts",
        );
    }

    #[test]
    fn tsx_top_level_space_is_unit_contract() {
        assert_top_level_space_is_unit_contract::<crate::TsxParser>(
            "function Foo(x: number): JSX.Element {\n  return <div>{x +\n",
            "partial.tsx",
        );
    }

    #[test]
    fn java_top_level_space_is_unit_contract() {
        assert_top_level_space_is_unit_contract::<crate::JavaParser>(
            "class Foo {\n  void bar(int x) {\n    return x +\n",
            "Partial.java",
        );
    }

    #[test]
    fn kotlin_top_level_space_is_unit_contract() {
        assert_top_level_space_is_unit_contract::<crate::KotlinParser>(
            "class Foo {\n  fun bar(x: Int): Int {\n    return x +\n",
            "Partial.kt",
        );
    }

    #[test]
    fn go_top_level_space_is_unit_contract() {
        assert_top_level_space_is_unit_contract::<crate::GoParser>(
            "package main\nfunc foo(x int) int {\n  return x +\n",
            "partial.go",
        );
    }

    #[test]
    fn rust_top_level_space_is_unit_contract() {
        assert_top_level_space_is_unit_contract::<crate::RustParser>(
            "fn foo(x: i32) -> i32 {\n    return x +\n",
            "partial.rs",
        );
    }

    #[test]
    fn csharp_top_level_space_is_unit_contract() {
        assert_top_level_space_is_unit_contract::<crate::CsharpParser>(
            "class Foo {\n  void Bar(int x) {\n    return x +\n",
            "Partial.cs",
        );
    }

    /// Regression for #429: a C# `EnumDeclaration` opens a FuncSpace
    /// via `is_func_space`, so `get_space_kind` must classify it as
    /// `Class` (matching Java/PHP/Groovy) rather than letting it fall
    /// through to `SpaceKind::Unknown`. The enum is the only declared
    /// space, so it appears as a direct child of the top-level Unit.
    #[test]
    fn csharp_enum_space_kind_is_class() {
        let src = "enum Color { Red, Green, Blue }\n";
        let path = std::path::PathBuf::from("Color.cs");
        let parser = crate::CsharpParser::new(src.as_bytes().to_vec(), &path, None);
        let space = metrics_inner(
            &parser,
            path.to_str().map(str::to_owned),
            MetricsOptions::default(),
        )
        .expect("metrics must yield a top-level space");

        let enum_space = space.spaces.first().expect("enum must open a child space");
        assert_eq!(enum_space.kind, SpaceKind::Class);
        assert_eq!(enum_space.name.as_deref(), Some("Color"));
    }

    #[test]
    fn bash_top_level_space_is_unit_contract() {
        assert_top_level_space_is_unit_contract::<crate::BashParser>(
            "function foo() {\n  echo \"x +\n",
            "partial.sh",
        );
    }

    /// Lua's grammar surfaces an `ERROR` root for this fixture
    /// (tree-sitter-lua 0.4.x), so this test exercises the
    /// synthetic-Unit wrapper directly, on par with the C++
    /// regression in `cpp_error_root_yields_unit_top_level_space`.
    /// The 16 sibling `*_top_level_space_is_unit_contract` tests
    /// only pin the public-API contract; only this and the C++ test
    /// actually trigger the wrapper code path. See #220.
    #[test]
    fn lua_partial_input_yields_synthetic_unit_wrapper() {
        assert_partial_input_yields_synthetic_unit_wrapper::<crate::LuaParser>(
            "function foo(x)\n  return x +\n",
            "partial.lua",
        );
    }

    #[test]
    fn tcl_top_level_space_is_unit_contract() {
        assert_top_level_space_is_unit_contract::<crate::TclParser>(
            "proc foo {x} {\n  return [expr {$x +\n",
            "partial.tcl",
        );
    }

    /// Lesson-9 contract for iRules: an unclosed `when` handler (truncated
    /// mid-body) must still yield a `Unit` top-level space. Like Tcl, the
    /// grammar keeps `source_file` as the root with an inner `ERROR`, so
    /// this pins the contract rather than the synthetic-Unit wrapper path.
    #[test]
    fn irules_top_level_space_is_unit_contract() {
        assert_top_level_space_is_unit_contract::<crate::IrulesParser>(
            "when HTTP_REQUEST { if { $a } {\n",
            "partial.irule",
        );
    }

    #[test]
    fn perl_top_level_space_is_unit_contract() {
        assert_top_level_space_is_unit_contract::<crate::PerlParser>(
            "sub foo {\n  my $x = shift;\n  return $x +\n",
            "partial.pl",
        );
    }

    #[test]
    fn php_top_level_space_is_unit_contract() {
        assert_top_level_space_is_unit_contract::<crate::PhpParser>(
            "<?php\nfunction foo($x) {\n  return $x +\n",
            "partial.php",
        );
    }

    #[test]
    fn elixir_top_level_space_is_unit_contract() {
        assert_top_level_space_is_unit_contract::<crate::ElixirParser>(
            "defmodule Foo do\n  def bar(x) do\n    x +\n",
            "partial.ex",
        );
    }

    // Regression for #275: the source-aware Getter must extract the
    // human-readable head name from each macro-shaped declaration.
    // The wave 2 implementation initially looked for an `Identifier` /
    // `Alias` / `Call` as a *direct* child of the outer Call, but the
    // tree-sitter-elixir grammar wraps the head in an `Arguments`
    // node, so every promoted Class / Function space was labelled
    // `<anonymous>` despite the source carrying a name.
    #[test]
    fn elixir_func_space_names_resolve_through_arguments_wrapper() {
        let src = "defmodule Foo.Bar do\n  def hello(x), do: x\n  defp helper, do: :ok\n  defmodule Inner do\n    def i, do: 1\n  end\nend\n";
        let path = std::path::PathBuf::from("foo.ex");
        let parser = crate::ElixirParser::new(src.as_bytes().to_vec(), &path, None);
        let space = metrics_inner(
            &parser,
            path.to_str().map(str::to_owned),
            MetricsOptions::default(),
        )
        .expect("metrics must yield a top-level space");

        // Top-level Unit -> file name.
        assert_eq!(space.name.as_deref(), Some("foo.ex"));

        // Outer defmodule Class is named `Foo.Bar`.
        let outer = space.spaces.first().expect("outer class space");
        assert_eq!(outer.kind, SpaceKind::Class);
        assert_eq!(outer.name.as_deref(), Some("Foo.Bar"));

        // Direct child names: `hello`, `helper`, `Inner`.
        let names: Vec<&str> = outer
            .spaces
            .iter()
            .map(|s| s.name.as_deref().unwrap_or("?"))
            .collect();
        assert_eq!(names, vec!["hello", "helper", "Inner"]);

        // Nested defmodule's child def resolves too.
        let inner = outer
            .spaces
            .iter()
            .find(|s| s.kind == SpaceKind::Class)
            .expect("nested class");
        let inner_names: Vec<&str> = inner
            .spaces
            .iter()
            .map(|s| s.name.as_deref().unwrap_or("?"))
            .collect();
        assert_eq!(inner_names, vec!["i"]);
    }

    /// `Preproc` and `Ccomment` are auxiliary grammars (preprocessor
    /// directives and comments respectively). They expose the same
    /// `ParserTrait` API, so the lesson-9 contract must hold for them
    /// too — a grammar bump promoting an inner construct to root would
    /// otherwise produce a non-`Unit` file-level space.
    #[test]
    fn preproc_top_level_space_is_unit_contract() {
        assert_top_level_space_is_unit_contract::<crate::PreprocParser>(
            "#ifdef FOO\n#define BAR(x) (x +\n",
            "partial.h",
        );
    }

    #[test]
    fn ccomment_top_level_space_is_unit_contract() {
        assert_top_level_space_is_unit_contract::<crate::CcommentParser>(
            "/* unterminated comment\n  spanning several\n",
            "partial.c",
        );
    }

    /// Ruby uses tree-sitter-ruby which always returns a `program`
    /// (Unit) root regardless of input — the synthetic-Unit fallback
    /// path is unreachable today. The test pins the contract so a
    /// future grammar bump that starts promoting an inner kind to
    /// root would fail here.
    #[test]
    fn ruby_top_level_space_is_unit_contract() {
        // Truncated method definition (missing `end`) plus an
        // incomplete parameter list — tree-sitter-ruby treats both as
        // ERROR children of `program`.
        assert_top_level_space_is_unit_contract::<crate::RubyParser>(
            "class Foo\n  def bar(\n    x\n  ",
            "partial.rb",
        );
    }

    // The former `non_utf8_path_yields_lossy_top_level_name` test
    // (issue #128) pinned the lossy-UTF-8 name derivation performed by
    // the path-positional `metrics_with_options` shim, retired with the
    // rest of the path-positional surface in #570. The `Ast` /
    // `metrics_inner` seam carries an explicit `Option<String>` name
    // end-to-end and never derives one from a `&Path`, so there is no
    // production code path left to regress; callers own any lossy
    // path-to-string conversion. The explicit-name contract is covered
    // by the `analyze_in_memory_snippet_carries_caller_supplied_name`
    // test below.

    /// `analyze` with a caller-supplied `Source::name` skips the
    /// lossy round-trip entirely — the top-level name is whatever
    /// string the caller passed, byte-for-byte. This is the
    /// post-#254 contract: callers analysing in-memory snippets no
    /// longer need a `Path` to identify the resulting `FuncSpace`.
    #[test]
    fn analyze_in_memory_snippet_carries_caller_supplied_name() {
        use crate::{Source, analyze};

        let source = Source::new(crate::LANG::Cpp, b"int a = 42;")
            .with_name(Some("in-memory.cpp".to_owned()));
        let space = analyze(source, MetricsOptions::default())
            .expect("analyze must yield a top-level space");
        assert_eq!(
            space.name.as_deref(),
            Some("in-memory.cpp"),
            "top-level name must be the caller-supplied string, byte-for-byte"
        );
    }

    /// `analyze` with `Source::name = None` leaves the top-level
    /// `FuncSpace::name` as `None`. The pre-#254 entry points always
    /// forced a `Some(...)`; the new API lets callers opt out.
    #[test]
    fn analyze_without_name_leaves_top_level_name_none() {
        use crate::{Source, analyze};

        let space = analyze(
            Source::new(crate::LANG::Cpp, b"int a = 42;"),
            MetricsOptions::default(),
        )
        .expect("analyze must yield a top-level space");
        assert!(
            space.name.is_none(),
            "top-level name must be None when Source::name is None, got {:?}",
            space.name
        );
    }

    // --- #306: file-scope suppression requires a Unit target ------
    //
    // `apply_suppression` historically picked `state_stack.first_mut()`
    // for the `File` arm, relying on the convention that the root
    // frame is always `SpaceKind::Unit`. The fix tightens that to an
    // explicit `SpaceKind::Unit` predicate so an accidentally
    // non-Unit root cannot silently swallow a file marker. These
    // tests pin the new behaviour: they construct a `State` slice by
    // hand (bypassing the parser) so the invariant violation is
    // observable in isolation.

    fn make_state<'a>(kind: SpaceKind) -> super::State<'a> {
        // Synthetic State constructor for `apply_suppression` tests.
        // Line spans are zeroed because these tests only inspect
        // `space.kind` and `space.suppressed`; do not reuse this helper
        // for tests that depend on `start_line` / `end_line` /
        // `metrics`.
        super::State {
            space: super::FuncSpace {
                name: None,
                start_line: 0,
                end_line: 0,
                kind,
                spaces: Vec::new(),
                metrics: super::CodeMetrics::default(),
                suppressed: super::SuppressionScope::default(),
            },
            halstead_maps: crate::metrics::halstead::HalsteadMaps::new(),
        }
    }

    fn file_suppression_all() -> crate::suppression::Suppression {
        crate::suppression::Suppression {
            kind: crate::suppression::SuppressionKind::File,
            scope: crate::suppression::SuppressionScope::All,
            source: crate::suppression::SuppressionSource::Native,
        }
    }

    #[test]
    fn file_suppression_attaches_to_unit_frame() {
        let mut stack = vec![make_state(SpaceKind::Unit), make_state(SpaceKind::Function)];
        super::apply_suppression(&mut stack, &file_suppression_all());
        assert!(
            stack[0].space.suppressed.is_all(),
            "file marker (scope=All) must attach to the Unit root frame"
        );
        assert!(
            stack[1].space.suppressed.is_empty(),
            "file marker must not attach to a non-Unit frame"
        );
    }

    #[test]
    fn file_suppression_skips_non_unit_root_frame() {
        // Synthetic stack where index 0 is *not* `Unit` — simulates
        // the broken-invariant case the explicit predicate guards
        // against. With the old `first_mut()` code this would
        // erroneously attach the file marker to a Function frame.
        let mut stack = vec![
            make_state(SpaceKind::Function),
            make_state(SpaceKind::Class),
        ];
        super::apply_suppression(&mut stack, &file_suppression_all());
        assert!(
            stack.iter().all(|s| s.space.suppressed.is_empty()),
            "file marker must be silently dropped when no Unit frame exists"
        );
    }

    #[test]
    fn file_suppression_finds_unit_deeper_in_stack() {
        // The new predicate is "first frame whose kind is Unit",
        // not "first frame". If the root invariant is violated and
        // a Unit frame sits below a non-Unit frame, the marker must
        // still land on the Unit frame rather than being dropped.
        // Under the old `first_mut()` code, the Function root would
        // have absorbed the marker; this test pins the new search
        // semantics.
        let mut stack = vec![make_state(SpaceKind::Function), make_state(SpaceKind::Unit)];
        super::apply_suppression(&mut stack, &file_suppression_all());
        assert!(
            stack[0].space.suppressed.is_empty(),
            "non-Unit frame above the Unit must not absorb the file marker"
        );
        assert!(
            stack[1].space.suppressed.is_all(),
            "file marker must land on the Unit frame even when not at index 0"
        );
    }

    #[test]
    fn file_suppression_empty_stack_is_silent_noop() {
        // No frames on the stack — `apply_suppression` must not
        // panic and must remain a silent no-op. Reaching the end of
        // this body proves no-panic; the stack cannot grow through
        // `&mut [State]`, so an explicit `is_empty()` check would
        // be a dead assertion.
        let mut stack: Vec<super::State<'_>> = Vec::new();
        super::apply_suppression(&mut stack, &file_suppression_all());
    }

    // --- #182: exclude_tests for Rust -----------------------------
    //
    // These exercise both flag values (`exclude_tests = false` is
    // the documented backward-compatible default; `true` opts in to
    // the new pruning). They are anchored on integer-valued
    // accessors (`nom_functions_sum`, `cyclomatic_sum`,
    // `cognitive_sum`, `n_operators`) rather than float magnitudes,
    // because Halstead floats are bit-brittle (lessons_learned.md).

    mod exclude_tests_rust {
        use crate::spaces::metrics_inner;
        use crate::{MetricsOptions, ParserTrait, RustParser};
        use std::path::PathBuf;

        fn analyse(source: &str, exclude_tests: bool) -> crate::FuncSpace {
            let path = PathBuf::from("lib.rs");
            let parser = RustParser::new(source.as_bytes().to_vec(), &path, None);
            metrics_inner(
                &parser,
                path.to_str().map(str::to_owned),
                MetricsOptions::default().with_exclude_tests(exclude_tests),
            )
            .expect("metrics must yield a top-level space")
        }

        // Production function plus an outer-attribute `#[test]`
        // function. With pruning on, the unit-level counts must
        // drop to the production function alone.
        #[test]
        fn outer_test_attribute_elides_function() {
            let source = "\
fn prod() -> i32 { 1 + 2 }

#[test]
fn t() { assert_eq!(1 + 1, 2); }
";
            let baseline = analyse(source, false);
            let pruned = analyse(source, true);

            // Baseline: both functions counted (2 functions).
            assert_eq!(baseline.metrics.nom.functions_sum() as usize, 2);
            // Pruned: only the production function (1 function).
            assert_eq!(pruned.metrics.nom.functions_sum() as usize, 1);
            // Cyclomatic should also drop: prod has 1, test fn body
            // adds its own branches via assert_eq!. We use
            // non-strict inequality (`pruned <= baseline`) here so
            // grammar tweaks that flatten `assert_eq!` expansion to
            // zero cyclomatic branches don't make this test brittle;
            // the load-bearing pruning check is `functions_sum`
            // above.
            assert!(
                pruned.metrics.cyclomatic.cyclomatic_sum()
                    <= baseline.metrics.cyclomatic.cyclomatic_sum()
            );
        }

        // `#[cfg(test)] mod tests { fn helper() {} #[test] fn t() {}
        // }` — every function inside the gated module disappears.
        #[test]
        fn cfg_test_mod_elides_entire_module() {
            let source = "\
fn prod() -> i32 { 1 }

#[cfg(test)]
mod tests {
    fn helper() -> i32 { 2 }
    fn another_helper() -> i32 { 3 }
    #[test] fn t() { assert_eq!(1, 1); }
}
";
            let baseline = analyse(source, false);
            let pruned = analyse(source, true);

            // Baseline: prod + helper + another_helper + t = 4 functions.
            assert_eq!(baseline.metrics.nom.functions_sum() as usize, 4);
            // Pruned: only prod survives.
            assert_eq!(pruned.metrics.nom.functions_sum() as usize, 1);
        }

        // `#[tokio::test]` is the most common async-runtime variant
        // and must be elided too. Baseline anchored at 2 so a grammar
        // regression that stops counting `async fn` cannot make this
        // test pass without pruning actually doing work.
        #[test]
        fn tokio_test_attribute_is_elided() {
            let source = "\
fn prod() -> i32 { 1 }

#[tokio::test]
async fn async_t() { let _x = 1; }
";
            let baseline = analyse(source, false);
            let pruned = analyse(source, true);
            assert_eq!(baseline.metrics.nom.functions_sum() as usize, 2);
            assert_eq!(pruned.metrics.nom.functions_sum() as usize, 1);
        }

        // `#[cfg(all(test, target_arch = \"x86_64\"))]` — the
        // attribute parser must accept commas inside `all(...)`.
        // Baseline anchored at 2 to guard against silent grammar
        // regressions (see `tokio_test_attribute_is_elided`).
        #[test]
        fn cfg_all_test_with_extras_is_elided() {
            let source = "\
fn prod() -> i32 { 1 }

#[cfg(all(test, target_arch = \"x86_64\"))]
fn arch_specific_test() { let _x = 1; }
";
            let baseline = analyse(source, false);
            let pruned = analyse(source, true);
            assert_eq!(baseline.metrics.nom.functions_sum() as usize, 2);
            assert_eq!(pruned.metrics.nom.functions_sum() as usize, 1);
        }

        // Plain prod-only file must be unchanged by either flag
        // value — i.e. the flag is genuinely a no-op when there's
        // no test code. Anchor the absolute count (2) so the
        // "they're equal" assertion can't be satisfied by both
        // values being 0.
        #[test]
        fn pure_production_unaffected_by_flag() {
            let source = "\
fn prod() -> i32 { 1 + 2 }
fn helper(x: i32) -> i32 { x * 2 }
";
            let baseline = analyse(source, false);
            let pruned = analyse(source, true);
            assert_eq!(baseline.metrics.nom.functions_sum() as usize, 2);
            assert_eq!(pruned.metrics.nom.functions_sum() as usize, 2);
            assert_eq!(
                baseline.metrics.cyclomatic.cyclomatic_sum(),
                pruned.metrics.cyclomatic.cyclomatic_sum(),
            );
        }

        // Backward compat: with the flag off (the default), every
        // node is still counted even when the source contains
        // test items.
        #[test]
        fn default_flag_off_preserves_baseline() {
            let source = "\
fn prod() -> i32 { 1 }

#[test]
fn t() { assert_eq!(1, 1); }
";
            let baseline_default = analyse(source, false);
            assert_eq!(baseline_default.metrics.nom.functions_sum() as usize, 2);
        }

        // Stacked attributes: tree-sitter exposes multiple
        // `#[...]` decorations as a chain of `AttributeItem`
        // siblings before the decorated item. The matcher must
        // walk all of them, not just the immediately-preceding
        // one, so a `#[cfg(target_arch = "x86_64")]` on top of
        // `#[cfg(test)]` still prunes.
        #[test]
        fn stacked_attributes_walk_all_siblings() {
            let source = "\
fn prod() -> i32 { 1 }

#[cfg(target_arch = \"x86_64\")]
#[cfg(test)]
fn t() { let _x = 1; }
";
            let baseline = analyse(source, false);
            let pruned = analyse(source, true);
            assert_eq!(baseline.metrics.nom.functions_sum() as usize, 2);
            assert_eq!(pruned.metrics.nom.functions_sum() as usize, 1);
        }

        // Regression for #278. `test` was previously required to be
        // the first operand of `all(...)` / `any(...)`; forms like
        // `cfg(all(unix, test))` and `cfg(any(feature = "x", test))`
        // were silently kept. Baseline anchored at 3 (prod + two
        // gated fns) so a grammar regression cannot satisfy the test
        // without pruning doing real work.
        #[test]
        fn cfg_with_test_not_first_is_elided() {
            let source = "\
fn prod() -> i32 { 1 }

#[cfg(all(unix, test))]
fn unix_only_test() { let _x = 1; }

#[cfg(any(feature = \"slow\", test))]
fn slow_or_test() { let _x = 2; }
";
            let baseline = analyse(source, false);
            let pruned = analyse(source, true);
            assert_eq!(baseline.metrics.nom.functions_sum() as usize, 3);
            assert_eq!(pruned.metrics.nom.functions_sum() as usize, 1);
        }

        // Negative coverage: attribute shapes that look like "test"
        // but must NOT trigger pruning. Production code marked with
        // `#[cfg(not(test))]`, a feature flag named "test", or a
        // user macro whose path contains "test" must survive
        // pruning intact.
        #[test]
        fn lookalike_attributes_are_not_pruned() {
            let source = "\
#[cfg(not(test))]
fn only_outside_tests() -> i32 { 1 }

#[cfg(feature = \"test\")]
fn behind_test_feature() -> i32 { 2 }

#[my_crate::test_helper]
fn decorated_helper() -> i32 { 3 }

#[cfg(all(unix, not(test)))]
fn unix_prod_only() -> i32 { 4 }
";
            let pruned = analyse(source, true);
            // None of the four attributes mark test-only code.
            // All four functions must survive — particularly the
            // last one, which combines `not(test)` with another
            // operand (regression sibling to #278).
            assert_eq!(pruned.metrics.nom.functions_sum() as usize, 4);
        }

        // Inner attribute on a module: `mod tests { #![cfg(test)] ... }`
        // is the idiomatic form when you want to put the gate inside
        // the module body rather than on the declaration. Baseline
        // anchored at 3 (prod + helper + t) so a grammar regression
        // that drops the module body cannot satisfy this test with
        // pruning disabled.
        #[test]
        fn inner_cfg_test_attribute_elides_module() {
            let source = "\
fn prod() -> i32 { 1 }

mod tests {
    #![cfg(test)]
    fn helper() -> i32 { 2 }
    #[test] fn t() { assert_eq!(1, 1); }
}
";
            let baseline = analyse(source, false);
            let pruned = analyse(source, true);
            assert_eq!(baseline.metrics.nom.functions_sum() as usize, 3);
            assert_eq!(pruned.metrics.nom.functions_sum() as usize, 1);
        }

        // #722: `sloc` is the lone loc sub-metric computed by span
        // subtraction rather than node accumulation, so before the fix
        // it stayed pinned at the full-file extent while `ploc`/`cloc`/
        // `lloc` correctly dropped — leaving an internally inconsistent
        // loc block (and a `blank` derived from the stale `sloc` that
        // over-counted). Under `exclude_tests`, unit `sloc` must drop in
        // step with the pruned test module.
        //
        // Layout (0-based rows): prod body rows 0..=3, blank row 4,
        // the `#[cfg(test)]` attribute (a sibling of `mod_item`, NOT
        // pruned) row 5, and the pruned `mod tests { … }` rows 6..=11.
        // Baseline `sloc` is the full 12-row span; pruned drops the six
        // module rows to 6, which equals the retained `ploc 5` (prod's
        // four lines + the surviving attribute line) plus the single
        // real `blank` line. Pre-fix, pruned `sloc` stayed 12 and
        // `blank` reported 7 — six phantom blanks from the elided module.
        #[test]
        fn sloc_drops_with_pruned_cfg_test_mod() {
            let source = "\
fn prod() {
    let x = 1;
    println!(\"{x}\");
}

#[cfg(test)]
mod tests {
    #[test]
    fn a() {
        assert_eq!(1, 1);
    }
}
";
            let baseline = analyse(source, false);
            let pruned = analyse(source, true);

            assert_eq!(baseline.metrics.loc.sloc(), 12);
            assert_eq!(baseline.metrics.loc.ploc(), 11);

            // The headline fix: `sloc` falls in step with `ploc`.
            assert_eq!(pruned.metrics.loc.sloc(), 6);
            assert_eq!(pruned.metrics.loc.ploc(), 5);
            // Internal consistency restored: one real blank line, not the
            // pre-fix seven.
            assert_eq!(pruned.metrics.loc.blank(), 1);
        }

        // Adjacent pruned modules: two top-level `#[cfg(test)] mod`
        // blocks. Their spans are disjoint, so the excluded line counts
        // simply add (no interval merge). Rows (0-based): prod row 0,
        // attr row 1, `mod a` rows 2..=5, attr row 6, `mod b` rows
        // 7..=10 — a 11-row span. Pruning removes both four-row modules,
        // leaving rows 0/1/6 (prod + the two surviving sibling
        // attributes) → `sloc 3`, matching `ploc 3` with zero blanks.
        #[test]
        fn sloc_drops_for_adjacent_test_modules() {
            let source = "\
fn prod() {}
#[cfg(test)]
mod a {
    #[test]
    fn x() {}
}
#[cfg(test)]
mod b {
    #[test]
    fn y() {}
}
";
            let baseline = analyse(source, false);
            let pruned = analyse(source, true);

            assert_eq!(baseline.metrics.loc.sloc(), 11);
            assert_eq!(pruned.metrics.loc.sloc(), 3);
            assert_eq!(pruned.metrics.loc.ploc(), 3);
            assert_eq!(pruned.metrics.loc.blank(), 0);
        }

        // Nested pruned modules: a non-test `mod inner` lives inside a
        // `#[cfg(test)] mod outer`. The walk `continue`s on `outer` and
        // never descends, so `inner`'s span is folded into `outer`'s and
        // never double-counted. Rows (0-based): prod row 0, attr row 1,
        // `mod outer` rows 2..=7 (an 8-row span). Pruning removes the
        // six outer rows, leaving rows 0/1 → `sloc 2`, matching `ploc 2`.
        #[test]
        fn sloc_drops_for_nested_test_modules() {
            let source = "\
fn prod() {}
#[cfg(test)]
mod outer {
    mod inner {
        #[test]
        fn t() {}
    }
}
";
            let baseline = analyse(source, false);
            let pruned = analyse(source, true);

            assert_eq!(baseline.metrics.loc.sloc(), 8);
            assert_eq!(pruned.metrics.loc.sloc(), 2);
            assert_eq!(pruned.metrics.loc.ploc(), 2);
            assert_eq!(pruned.metrics.loc.blank(), 0);
        }

        // A pruned test function nested inside a *retained* `impl` block
        // must shrink BOTH that impl space's `sloc` AND every enclosing
        // space up to the unit root. The prune hook records the span on the
        // walker's current enclosing func-space (after `finalize`), and
        // `Sloc::merge` folds the pruned line count upward so the unit's
        // span-based `sloc` drops in step — mirroring how `Ploc` unions its
        // line-set upward (issue #741, a #722 follow-up). Rows (0-based):
        // `impl Foo {` row 0, `fn prod` row 1, the `#[test]` attribute (a
        // sibling, retained) row 2, the pruned single-line `fn t() {}` row
        // 3, `}` row 4 — a five-row impl span inside a six-row unit span.
        // Pruning removes the one test-fn row, so both the impl-level and
        // the unit-level `sloc` drop by exactly one.
        #[test]
        fn sloc_drops_for_test_fn_nested_in_impl() {
            let source = "\
impl Foo {
    fn prod(&self) {}
    #[test]
    fn t() {}
}
";
            let baseline = analyse(source, false);
            let pruned = analyse(source, true);

            // The unit wraps exactly one `impl` space; the length guard
            // makes the single-child assumption explicit rather than
            // relying on `[0]` ordering.
            assert_eq!(baseline.spaces.len(), 1);
            assert_eq!(pruned.spaces.len(), 1);

            // Enclosing-space attribution: the impl space shrinks by the
            // one pruned test-fn row.
            let baseline_impl = &baseline.spaces[0];
            let pruned_impl = &pruned.spaces[0];
            assert_eq!(baseline_impl.metrics.loc.sloc(), 5);
            assert_eq!(pruned_impl.metrics.loc.sloc(), 4);

            // Unit-root propagation (the #741 fix): the pruned line count
            // folds upward through `Sloc::merge`, so the unit's `sloc` drops
            // by the same one row. Before the fix this stayed at the
            // baseline value because only the impl's `excluded_lines` grew.
            assert_eq!(baseline.metrics.loc.sloc(), 5);
            assert_eq!(pruned.metrics.loc.sloc(), 4);
        }

        // A `#[test] fn` directly inside a production `impl` (no separate
        // `#[cfg(test)] mod`): the unit's span-based `sloc` must still drop
        // by the pruned test-fn rows. Rows (0-based): `impl Calc {` row 0,
        // `fn add` rows 1..=3 (a retained production method), `#[test]` row
        // 4, `fn t` rows 5..=7 (pruned), `}` row 8 — a nine-row unit span.
        // Pruning removes the three test-fn rows (5..=7), so the unit `sloc`
        // drops from 9 to 6, matching `ploc`.
        #[test]
        fn sloc_drops_for_test_fn_in_production_impl() {
            let source = "\
impl Calc {
    fn add(&self, x: i32) -> i32 {
        x + 1
    }
    #[test]
    fn t() {
        assert_eq!(1 + 1, 2);
    }
}
";
            let baseline = analyse(source, false);
            let pruned = analyse(source, true);

            assert_eq!(baseline.metrics.loc.sloc(), 9);
            assert_eq!(pruned.metrics.loc.sloc(), 6);
            assert_eq!(pruned.metrics.loc.ploc(), pruned.metrics.loc.sloc());
        }

        // A pruned test item nested inside a *retained* closure: the closure
        // body opens its own func-space (Rust `ClosureExpression`), so the
        // prune hook records the span on the closure, not the unit. The fix
        // must still propagate the count up to the unit. Rows (0-based):
        // `fn make() {` row 0, `let f = || {` row 1, `#[test]` row 2,
        // `fn t() {}` row 3 (pruned), `};` row 4, `}` row 5 — a six-row unit
        // span. Pruning removes the one test-fn row, so the unit `sloc`
        // drops from 6 to 5.
        #[test]
        fn sloc_drops_for_test_fn_nested_in_closure() {
            let source = "\
fn make() {
    let f = || {
        #[test]
        fn t() {}
    };
}
";
            let baseline = analyse(source, false);
            let pruned = analyse(source, true);

            assert_eq!(baseline.metrics.loc.sloc(), 6);
            assert_eq!(pruned.metrics.loc.sloc(), 5);
        }

        // A non-test `impl` with no test items must be unaffected by the
        // upward-propagation fold: with nothing pruned, `excluded_lines`
        // stays zero at every level, so pruned and baseline `sloc` agree.
        #[test]
        fn non_test_impl_sloc_unaffected_by_pruning() {
            let source = "\
impl Calc {
    fn add(&self, x: i32) -> i32 {
        x + 1
    }
}
";
            let baseline = analyse(source, false);
            let pruned = analyse(source, true);

            assert_eq!(baseline.metrics.loc.sloc(), pruned.metrics.loc.sloc());
            assert_eq!(pruned.spaces.len(), 1);
            assert_eq!(
                baseline.spaces[0].metrics.loc.sloc(),
                pruned.spaces[0].metrics.loc.sloc()
            );
        }
    }

    // Non-Rust languages must ignore `exclude_tests = true` because
    // they don't override `should_skip_subtree`. This is the
    // "spot-check non-Rust" check from issue #182.
    mod exclude_tests_non_rust {
        use crate::spaces::metrics_inner;
        use crate::{CppParser, MetricsOptions, ParserTrait};
        use std::path::PathBuf;

        #[test]
        fn cpp_ignores_exclude_tests_flag() {
            let source = "\
int prod() { return 1; }
int helper() { return 2; }
";
            let path = PathBuf::from("foo.cpp");
            let parser = CppParser::new(source.as_bytes().to_vec(), &path, None);
            let baseline = metrics_inner(
                &parser,
                path.to_str().map(str::to_owned),
                MetricsOptions::default().with_exclude_tests(false),
            )
            .expect("baseline must yield a top-level space");
            let parser = CppParser::new(source.as_bytes().to_vec(), &path, None);
            let pruned = metrics_inner(
                &parser,
                path.to_str().map(str::to_owned),
                MetricsOptions::default().with_exclude_tests(true),
            )
            .expect("pruned must yield a top-level space");
            // Anchor on the absolute count (2) so a regression that
            // dropped all C++ functions wouldn't satisfy a bare
            // `baseline == pruned` check.
            assert_eq!(baseline.metrics.nom.functions_sum() as usize, 2);
            assert_eq!(pruned.metrics.nom.functions_sum() as usize, 2);
        }
    }

    // --- #257: per-metric selection via with_only --------------------
    //
    // Exercise the gating bitfield through the recommended public
    // entry point (`analyze` + `Source`) rather than the deprecated
    // path-positional shims, so the tests pin the surface library
    // consumers actually use.

    mod with_only {
        use crate::{LANG, Metric, MetricSet, MetricsOptions, Source, analyze};

        const SOURCE: &str = "\
fn prod(x: i32) -> i32 {
    if x > 0 { x + 1 } else { x - 1 }
}
";

        fn analyse(metrics: &[Metric]) -> crate::FuncSpace {
            let opts = MetricsOptions::default().with_only(metrics);
            analyze(
                Source::new(LANG::Rust, SOURCE.as_bytes()).with_name(Some("lib.rs".to_owned())),
                opts,
            )
            .expect("analyze must yield a top-level space")
        }

        // `with_only(&[Metric::Loc])` records exactly that bit on
        // `CodeMetrics.selected` and leaves the dependent metrics
        // (cognitive / cyclomatic / halstead / ...) at their default
        // values. The dependent-metric anchors guard against the
        // walker silently running them anyway.
        #[test]
        fn loc_only_skips_other_metrics() {
            let full = analyze(
                Source::new(LANG::Rust, SOURCE.as_bytes()).with_name(Some("lib.rs".to_owned())),
                MetricsOptions::default(),
            )
            .expect("full analyze must yield a top-level space");
            let pruned = analyse(&[Metric::Loc]);

            assert_eq!(
                pruned.metrics.selected(),
                MetricSet::empty().with(Metric::Loc),
                "with_only(&[Loc]) must record exactly the Loc bit"
            );
            // LoC populated: the production function span is >= 1 ploc.
            assert!(pruned.metrics.loc.ploc() >= 1);
            // Full run has > 0 cognitive/cyclomatic; pruned must be
            // exactly zero because the compute call is gated off.
            assert!(full.metrics.cognitive.cognitive_sum() > 0);
            assert_eq!(pruned.metrics.cognitive.cognitive_sum(), 0);
            assert!(full.metrics.cyclomatic.cyclomatic_sum() > 0);
            assert_eq!(pruned.metrics.cyclomatic.cyclomatic_sum(), 0);
            // Halstead operators count is at the default (0) — no
            // per-node token text was hashed.
            assert_eq!(pruned.metrics.halstead.unique_operators(), 0);
        }

        // Selecting `Mi` alone must auto-add its dependencies
        // (Loc + Cyclomatic + Halstead) — otherwise the MI formula
        // would compute against zero inputs and return a meaningless
        // score.
        #[test]
        fn mi_auto_pulls_dependencies() {
            let pruned = analyse(&[Metric::Mi]);
            let sel = pruned.metrics.selected();
            assert!(sel.contains(Metric::Mi));
            assert!(sel.contains(Metric::Loc), "Mi depends on Loc");
            assert!(sel.contains(Metric::Cyclomatic), "Mi depends on Cyclomatic");
            assert!(sel.contains(Metric::Halstead), "Mi depends on Halstead");
            // Unrelated metrics must NOT be selected.
            assert!(!sel.contains(Metric::Abc));
            assert!(!sel.contains(Metric::Tokens));
            // The dependencies must actually be populated — not just
            // selected. Otherwise the MI formula receives zero inputs
            // and `mi_original`'s `inputs_are_empty` short-circuit
            // returns 0.0, which would also be `is_finite`. We anchor
            // on the dependency values themselves (Loc ploc > 0,
            // Cyclomatic sum > 0) so the test would fail if the
            // walker silently skipped the dependency compute.
            assert!(
                pruned.metrics.loc.ploc() > 0,
                "Loc must have run (Mi dependency); got ploc=0"
            );
            assert!(
                pruned.metrics.cyclomatic.cyclomatic_sum() > 0,
                "Cyclomatic must have run (Mi dependency); got sum=0"
            );
            // With non-zero inputs feeding the MI formula, the result
            // is a finite non-zero number (the MI for this snippet is
            // around 150 — a positive value well above the 0.0 that
            // `inputs_are_empty` would short-circuit to).
            let mi_value = pruned.metrics.mi.original();
            assert!(
                mi_value.is_finite() && mi_value != 0.0,
                "MI must be finite and non-default when its dependencies were computed; got {mi_value}"
            );
        }

        // `with_only(&[Metric::Wmc])` auto-adds Cyclomatic + Nom.
        #[test]
        fn wmc_auto_pulls_dependencies() {
            let pruned = analyse(&[Metric::Wmc]);
            let sel = pruned.metrics.selected();
            assert!(sel.contains(Metric::Wmc));
            assert!(
                sel.contains(Metric::Cyclomatic),
                "Wmc depends on Cyclomatic"
            );
            assert!(sel.contains(Metric::Nom), "Wmc depends on Nom");
            assert!(!sel.contains(Metric::Halstead));
            // Dependency must actually be computed, not just bit-set:
            // selecting Wmc alone must populate Cyclomatic & Nom.
            assert!(
                pruned.metrics.cyclomatic.cyclomatic_sum() > 0,
                "Cyclomatic must have run (Wmc dependency); got sum=0"
            );
            assert!(
                pruned.metrics.nom.functions_sum() > 0,
                "Nom must have run (Wmc dependency); got functions_sum=0"
            );
        }

        // #428: selecting Cognitive/Exit/NArgs alone must auto-pull
        // Nom so their per-function averages divide by the real
        // function count instead of the `Stats` default (0), which
        // would otherwise yield inf/NaN. `SOURCE` is a single
        // function with one `if` branch and one argument, so the
        // function count is exactly 1 and each average equals its
        // own sum.
        #[test]
        fn cognitive_only_pulls_nom_and_average_is_finite() {
            let pruned = analyse(&[Metric::Cognitive]);
            let sel = pruned.metrics.selected();
            assert!(sel.contains(Metric::Cognitive));
            assert!(sel.contains(Metric::Nom), "Cognitive depends on Nom (#428)");
            // Nom must actually run, supplying the divisor.
            assert!(
                pruned.metrics.nom.total() > 0,
                "Nom must have run (Cognitive dependency); got total=0"
            );
            let avg = pruned.metrics.cognitive.cognitive_average();
            assert!(
                avg.is_finite(),
                "cognitive_average must be finite when Nom is pulled in; got {avg}"
            );
            // The `if`/`else` over one function => cognitive sum == 2
            // => average == 2 (one increment for the `if`, one for the
            // `else` branch).
            assert_eq!(pruned.metrics.cognitive.cognitive_sum(), 2);
            assert_eq!(avg, 2.0);
        }

        #[test]
        fn exit_only_pulls_nom_and_average_is_finite() {
            let pruned = analyse(&[Metric::Nexits]);
            let sel = pruned.metrics.selected();
            assert!(sel.contains(Metric::Nexits));
            assert!(sel.contains(Metric::Nom), "Exit depends on Nom (#428)");
            assert!(
                pruned.metrics.nom.total() > 0,
                "Nom must have run (Exit dependency); got total=0"
            );
            let avg = pruned.metrics.nexits.nexits_average();
            assert!(
                avg.is_finite(),
                "nexits_average must be finite when Nom is pulled in; got {avg}"
            );
            // `prod` has no explicit `return`, so nexits_sum == 0 and the
            // guarded divisor (1) keeps the average a finite 0.0.
            assert_eq!(pruned.metrics.nexits.nexits_sum(), 0);
            assert_eq!(avg, 0.0);
        }

        #[test]
        fn nargs_only_pulls_nom_and_average_is_finite() {
            let pruned = analyse(&[Metric::Nargs]);
            let sel = pruned.metrics.selected();
            assert!(sel.contains(Metric::Nargs));
            assert!(sel.contains(Metric::Nom), "NArgs depends on Nom (#428)");
            assert!(
                pruned.metrics.nom.total() > 0,
                "Nom must have run (NArgs dependency); got total=0"
            );
            let avg = pruned.metrics.nargs.average();
            assert!(
                avg.is_finite(),
                "average must be finite when Nom is pulled in; got {avg}"
            );
            // One argument over one function => average == 1.
            assert_eq!(avg, 1.0);
        }

        // `MetricsOptions::default()` selects every metric (#257's
        // default-preservation contract).
        #[test]
        fn default_options_select_every_metric() {
            let full = analyze(
                Source::new(LANG::Rust, SOURCE.as_bytes()).with_name(Some("lib.rs".to_owned())),
                MetricsOptions::default(),
            )
            .expect("analyze must yield a top-level space");
            assert_eq!(full.metrics.selected(), MetricSet::all());
        }

        // JSON serialization elides unselected metrics. Anchored on
        // the field names emitted at the top level of the
        // `metrics` object rather than the full payload so a future
        // additive change (new metric, new sub-field) doesn't shift
        // unrelated tests.
        #[test]
        fn unselected_metrics_are_skipped_in_json() {
            let pruned = analyse(&[Metric::Loc]);
            let json =
                serde_json::to_value(&pruned.metrics).expect("CodeMetrics must serialize cleanly");
            let metrics = json.as_object().expect("CodeMetrics serializes as object");

            assert!(
                metrics.contains_key("loc"),
                "loc must be serialized when selected"
            );
            for skipped in [
                "cognitive",
                "cyclomatic",
                "halstead",
                "nom",
                "tokens",
                "nargs",
                "nexits",
                "abc",
                "mi",
                "wmc",
                "npm",
                "npa",
            ] {
                assert!(
                    !metrics.contains_key(skipped),
                    "{skipped} must be elided when not selected"
                );
            }
        }

        // #522: `kind` (via `get_space_kind_with_code`) is computed
        // lazily — skipped when a node is neither a func space nor a
        // Loc consumer. The lazy path must be byte-equivalent for
        // every consumer:
        //   - promoted (func_space) nodes still compute `kind`, so the
        //     space tree's `SpaceKind`s are unchanged;
        //   - non-Loc metrics never read `unit`, so their values are
        //     unchanged when Loc is deselected.
        // Elixir is the canonical regression target: its
        // `get_space_kind_with_code` runs a per-`Call` source-text
        // keyword scan, and `defmodule` / `def` promote to Class /
        // Function spaces whose kind would be lost if the lazy gate
        // skipped a node it shouldn't.
        #[test]
        fn elixir_loc_deselected_preserves_kinds_and_metrics() {
            use crate::SpaceKind;

            const ELIXIR_SOURCE: &str = "\
defmodule Greeter do
  def hello(name) do
    if name == \"\" do
      :anon
    else
      name
    end
  end

  def bye() do
    :ok
  end
end
";

            fn collect_kinds(space: &crate::FuncSpace, out: &mut Vec<SpaceKind>) {
                out.push(space.kind);
                for sub in &space.spaces {
                    collect_kinds(sub, out);
                }
            }

            fn analyse_elixir(metrics: Option<&[Metric]>) -> crate::FuncSpace {
                let opts = match metrics {
                    Some(m) => MetricsOptions::default().with_only(m),
                    None => MetricsOptions::default(),
                };
                analyze(
                    Source::new(LANG::Elixir, ELIXIR_SOURCE.as_bytes())
                        .with_name(Some("greeter.ex".to_owned())),
                    opts,
                )
                .expect("analyze must yield a top-level space")
            }

            let full = analyse_elixir(None);
            // Cognitive is a non-Loc metric, exercised without Loc so
            // the lazy gate takes the skip branch on non-promoted
            // nodes. Cognitive auto-pulls Nom; neither selects Loc.
            let pruned = analyse_elixir(Some(&[Metric::Cognitive]));

            assert!(
                !pruned.metrics.selected().contains(Metric::Loc),
                "test premise: Loc must be deselected on the pruned run"
            );

            // The promoted-space kinds must be identical: defmodule =>
            // Class, the two def macros => Function, the module file
            // root => Unit. If the lazy gate wrongly skipped a promoted
            // node, FuncSpace::new would have seen SpaceKind::Unknown.
            let mut full_kinds = Vec::new();
            let mut pruned_kinds = Vec::new();
            collect_kinds(&full, &mut full_kinds);
            collect_kinds(&pruned, &mut pruned_kinds);
            assert_eq!(
                full_kinds, pruned_kinds,
                "lazy `kind` computation must not change the space-tree SpaceKinds"
            );
            assert!(
                full_kinds.contains(&SpaceKind::Class),
                "test premise: defmodule must promote to a Class space"
            );
            assert!(
                full_kinds.contains(&SpaceKind::Function),
                "test premise: def must promote to a Function space"
            );

            // The non-Loc metric value must be byte-identical between
            // the full and Loc-deselected runs (the `unit` flag only
            // feeds Loc, so deselecting Loc cannot move it).
            assert!(
                full.metrics.cognitive.cognitive_sum() > 0,
                "test premise: the source has cognitive complexity (the `if`/`else`)"
            );
            assert_eq!(
                full.metrics.cognitive.cognitive_sum(),
                pruned.metrics.cognitive.cognitive_sum(),
                "deselecting Loc must not change cognitive complexity"
            );
        }

        // Empty slice = nothing selected. Every metric must be
        // elided from JSON output; the space tree is still
        // produced.
        #[test]
        fn empty_slice_selects_nothing() {
            let pruned = analyse(&[]);
            assert_eq!(pruned.metrics.selected(), MetricSet::empty());
            let json =
                serde_json::to_value(&pruned.metrics).expect("CodeMetrics must serialize cleanly");
            let metrics = json.as_object().expect("CodeMetrics serializes as object");
            assert!(
                metrics.is_empty(),
                "with_only(&[]) must elide every metric, got keys {:?}",
                metrics.keys().collect::<Vec<_>>()
            );
        }

        // #743: `with_metric_set` must close the caller-supplied set
        // under its dependencies, exactly like `with_only`. A verbatim
        // `empty().with(Mi)` would otherwise compute the MI formula
        // against zero-valued Loc / Cyclomatic / Halstead inputs and
        // emit a meaningless score with no error.
        #[test]
        fn with_metric_set_resolves_dependency_closure() {
            let unresolved = MetricSet::empty().with(Metric::Mi);
            assert!(
                !unresolved.contains(Metric::Loc),
                "test premise: the verbatim set must omit Mi's deps"
            );

            let opts = MetricsOptions::default().with_metric_set(unresolved);
            let space = analyze(
                Source::new(LANG::Rust, SOURCE.as_bytes()).with_name(Some("lib.rs".to_owned())),
                opts,
            )
            .expect("analyze must yield a top-level space");

            let sel = space.metrics.selected();
            assert!(sel.contains(Metric::Mi));
            assert!(sel.contains(Metric::Loc), "Mi depends on Loc");
            assert!(sel.contains(Metric::Cyclomatic), "Mi depends on Cyclomatic");
            assert!(sel.contains(Metric::Halstead), "Mi depends on Halstead");

            // The dependencies must actually be computed, not just
            // bit-set: Loc ploc > 0 and Cyclomatic sum > 0 prove the
            // walker ran them.
            assert!(
                space.metrics.loc.ploc() > 0,
                "Loc must have run (Mi dependency); got ploc=0"
            );
            assert!(
                space.metrics.cyclomatic.cyclomatic_sum() > 0,
                "Cyclomatic must have run (Mi dependency); got sum=0"
            );

            // The resolved `with_metric_set` form must match the
            // `with_only(&[Mi])` result bit-for-bit, both in the
            // selected set and the computed MI value.
            let via_only = analyze(
                Source::new(LANG::Rust, SOURCE.as_bytes()).with_name(Some("lib.rs".to_owned())),
                MetricsOptions::default().with_only(&[Metric::Mi]),
            )
            .expect("analyze must yield a top-level space");
            assert_eq!(sel, via_only.metrics.selected());
            let mi_value = space.metrics.mi.original();
            assert!(
                mi_value.is_finite() && mi_value != 0.0,
                "MI must be finite and non-default once its deps are computed; got {mi_value}"
            );
            assert_eq!(mi_value, via_only.metrics.mi.original());
        }

        // An already-resolved set passes through `with_metric_set`
        // unchanged (idempotence at the builder level).
        #[test]
        fn with_metric_set_passes_resolved_set_unchanged() {
            let resolved = MetricSet::from_slice_with_deps(&[Metric::Mi]);
            let opts = MetricsOptions::default().with_metric_set(resolved);
            let space = analyze(
                Source::new(LANG::Rust, SOURCE.as_bytes()).with_name(Some("lib.rs".to_owned())),
                opts,
            )
            .expect("analyze must yield a top-level space");
            assert_eq!(space.metrics.selected(), resolved);
        }
    }
}
