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

use std::collections::HashMap;

use serde::Serialize;
use serde::ser::SerializeStruct;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::langs::LANG;
use crate::metric_set::{Metric, MetricSet};
use crate::preproc::PreprocResults;

use crate::checker::Checker;
use crate::error::MetricsError;
use crate::node::Node;
use crate::suppression::{
    Suppression, SuppressionKind, SuppressionScope, parse_marker as parse_suppression_marker,
};

use crate::abc::{self, Abc};
use crate::cognitive::{self, Cognitive};
use crate::cyclomatic::{self, Cyclomatic};
use crate::exit::{self, Exit};
use crate::getter::Getter;
use crate::halstead::{self, Halstead, HalsteadMaps};
use crate::loc::{self, Loc};
use crate::mi::{self, Mi};
use crate::nargs::{self, NArgs};
use crate::nom::{self, Nom};
use crate::npa::{self, Npa};
use crate::npm::{self, Npm};
use crate::tokens::{self, Tokens};
use crate::wmc::{self, Wmc};

use crate::output::dump_metrics::*;
use crate::traits::*;

/// The list of supported space kinds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
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
#[derive(Default, Debug, Clone)]
pub struct CodeMetrics {
    /// `NArgs` data
    pub nargs: nargs::Stats,
    /// `NExits` data
    pub nexits: exit::Stats,
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

impl Serialize for CodeMetrics {
    // Per-metric serialization gated by `self.selected`. We
    // pre-count the number of fields that will be emitted so the
    // `SerializeStruct` header is accurate (formats like CBOR write
    // the field count up front and reject mismatches at the end).
    //
    // The existing skip-when-disabled predicates for `wmc`, `npm`, and
    // `npa` are honored alongside the selection mask: a metric is
    // emitted iff it was selected AND not flagged as disabled by the
    // metric itself.
    #[allow(clippy::similar_names)] // wmc / npm / npa are domain terms
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let sel = self.selected;
        let emit_wmc = sel.contains(Metric::Wmc) && !self.wmc.is_disabled();
        let emit_npm = sel.contains(Metric::Npm) && !self.npm.is_disabled();
        let emit_npa = sel.contains(Metric::Npa) && !self.npa.is_disabled();

        // 10 always-on metrics (nargs, nexits, cognitive, cyclomatic,
        // halstead, loc, nom, tokens, mi, abc) plus up to 3 from the
        // class-only group (wmc, npm, npa). The count must track the
        // serialize_field arms below 1:1 — CBOR writes the field
        // count up front and rejects mismatches at end().
        let always_on = [
            Metric::NArgs,
            Metric::Exit,
            Metric::Cognitive,
            Metric::Cyclomatic,
            Metric::Halstead,
            Metric::Loc,
            Metric::Nom,
            Metric::Tokens,
            Metric::Mi,
            Metric::Abc,
        ];
        let field_count = always_on.iter().filter(|m| sel.contains(**m)).count()
            + usize::from(emit_wmc)
            + usize::from(emit_npm)
            + usize::from(emit_npa);

        let mut st = serializer.serialize_struct("CodeMetrics", field_count)?;
        // Each arm must match exactly one of the booleans counted into
        // `field_count` above — drift here will make CBOR reject the
        // payload at `st.end()`.
        macro_rules! emit_if {
            ($cond:expr, $key:literal, $field:expr) => {
                if $cond {
                    st.serialize_field($key, $field)?;
                }
            };
        }
        emit_if!(sel.contains(Metric::NArgs), "nargs", &self.nargs);
        emit_if!(sel.contains(Metric::Exit), "nexits", &self.nexits);
        emit_if!(
            sel.contains(Metric::Cognitive),
            "cognitive",
            &self.cognitive
        );
        emit_if!(
            sel.contains(Metric::Cyclomatic),
            "cyclomatic",
            &self.cyclomatic
        );
        emit_if!(sel.contains(Metric::Halstead), "halstead", &self.halstead);
        emit_if!(sel.contains(Metric::Loc), "loc", &self.loc);
        emit_if!(sel.contains(Metric::Nom), "nom", &self.nom);
        emit_if!(sel.contains(Metric::Tokens), "tokens", &self.tokens);
        emit_if!(sel.contains(Metric::Mi), "mi", &self.mi);
        emit_if!(sel.contains(Metric::Abc), "abc", &self.abc);
        emit_if!(emit_wmc, "wmc", &self.wmc);
        emit_if!(emit_npm, "npm", &self.npm);
        emit_if!(emit_npa, "npa", &self.npa);
        st.end()
    }
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
#[derive(Debug, Clone, Serialize)]
pub struct FuncSpace {
    /// The name of a function space.
    ///
    /// For the top-level (file-level) `FuncSpace`, this is the value
    /// supplied via [`Source::name`] to [`analyze`] — typically a file
    /// path or other display identifier chosen by the caller. The
    /// library no longer derives this from a `&Path` or applies lossy
    /// UTF-8 conversion; callers are expected to pass an
    /// already-stringified identifier (or `None` if they have no
    /// meaningful name to attach). The deprecated entry points
    /// `get_function_spaces` / [`metrics_with_options`] continue to
    /// derive a lossy string from the `&Path` argument for backwards
    /// compatibility.
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
    /// directives (see [`crate::suppression`]). The top-level
    /// (file-level) `FuncSpace` aggregates every file-scoped marker;
    /// nested function spaces aggregate every function-scoped marker
    /// whose comment lies inside their source range. Metric
    /// computation itself is unaffected — this field is consumed by
    /// downstream *threshold-check* code (e.g. `bca check`) to decide
    /// whether to surface a violation.
    ///
    /// Defaults to `SuppressionScope::default()` (an empty `Some`), so
    /// pre-existing code paths that do not honor suppressions see no
    /// behaviour change. The field is elided from JSON output when
    /// empty so the existing schema is unchanged for files without
    /// markers.
    #[serde(default, skip_serializing_if = "SuppressionScope::is_empty")]
    pub suppressed: SuppressionScope,
}

impl FuncSpace {
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

        // The top-level Unit's name is overwritten by `metrics_with_options`
        // (when called with an explicit name) before returning, so
        // computing it here is wasted work. Other kinds keep the
        // AST-derived name.
        let name = (kind != SpaceKind::Unit)
            .then(|| {
                T::get_func_space_name(node, code)
                    .map(|name| name.split_whitespace().collect::<Vec<_>>().join(" "))
            })
            .flatten();

        Self {
            name,
            spaces: Vec::new(),
            metrics: CodeMetrics::with_selected(selected),
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
    // `Nom::functions_sum / closures_sum / total` are only meaningful
    // if Nom was selected; when it isn't, the divisor is the Stats
    // default (0) and the per-metric `finalize` calls treat that as
    // "no functions, no closures, no items". Compute the divisors
    // once and feed them into each gated finalize.
    let nom_functions = state.space.metrics.nom.functions_sum() as usize;
    let nom_closures = state.space.metrics.nom.closures_sum() as usize;
    let nom_total = state.space.metrics.nom.total() as usize;
    // Cognitive average
    if selected.contains(Metric::Cognitive) {
        state.space.metrics.cognitive.finalize(nom_total);
    }
    // Nexit average
    if selected.contains(Metric::Exit) {
        state.space.metrics.nexits.finalize(nom_total);
    }
    // Nargs average
    if selected.contains(Metric::NArgs) {
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
    if selected.contains(Metric::Exit) {
        state.space.metrics.nexits.compute_minmax();
    }
    if selected.contains(Metric::Cognitive) {
        state.space.metrics.cognitive.compute_minmax();
    }
    if selected.contains(Metric::NArgs) {
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

fn finalize<T: ParserTrait>(state_stack: &mut Vec<State>, diff_level: usize, selected: MetricSet) {
    if state_stack.is_empty() {
        return;
    }
    for _ in 0..diff_level {
        if state_stack.len() == 1 {
            let last_state = state_stack
                .last_mut()
                .expect("invariant: state_stack has exactly one element");
            compute_minmax(last_state, selected);
            compute_sum(last_state, selected);
            compute_halstead_mi_and_wmc::<T>(last_state, selected);
            compute_averages(last_state, selected);
            break;
        }
        let mut state = state_stack
            .pop()
            .expect("invariant: state_stack has more than one element");
        compute_minmax(&mut state, selected);
        compute_sum(&mut state, selected);
        compute_halstead_mi_and_wmc::<T>(&mut state, selected);
        compute_averages(&mut state, selected);

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
/// used by the C++ preprocessor lookup (`Source::preproc_path`). The
/// older path-positional entry points (`get_function_spaces`,
/// `metrics_with_options`) conflate the two and derive the name via
/// lossy UTF-8 conversion of the path; for in-memory snippets, code
/// fetched over the network, or test fixtures, callers can now pass
/// `Source` directly without manufacturing a `Path`.
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
    pub lang: LANG,
    /// Raw source bytes. `Source` borrows them so callers retain
    /// ownership; `analyze` copies into the parser's owned buffer.
    pub code: &'a [u8],
    /// Display / identifier name for the top-level [`FuncSpace`].
    /// If `None`, the top-level [`FuncSpace::name`] is left `None`.
    pub name: Option<String>,
    /// Optional path used only by the C++ preprocessor lookup
    /// (`get_fake_code`) to resolve macro definitions in
    /// [`PreprocResults`]. For non-C++ languages this is ignored.
    /// Defaults to `None`.
    pub preproc_path: Option<&'a Path>,
    /// Preprocessor results paired with [`Source::preproc_path`].
    /// Same shape as the `pr` arg on the deprecated entry points.
    pub preproc: Option<Arc<PreprocResults>>,
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
            code,
            name: None,
            preproc_path: None,
            preproc: None,
        }
    }

    /// Builder-style setter for [`Source::name`].
    #[inline]
    #[must_use]
    pub fn with_name(mut self, name: Option<String>) -> Self {
        self.name = name;
        self
    }

    /// Builder-style setter for [`Source::preproc_path`].
    #[inline]
    #[must_use]
    pub fn with_preproc_path(mut self, preproc_path: Option<&'a Path>) -> Self {
        self.preproc_path = preproc_path;
        self
    }

    /// Builder-style setter for [`Source::preproc`].
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
/// Build one via [`Ast::parse`] (mirrors [`analyze`]) or
/// [`Ast::from_tree_sitter`] (mirrors [`metrics_from_tree`] but with an
/// explicit display name instead of a lossy path-to-string conversion).
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
/// assert!(loc.metrics.loc.ploc() > 0.0);
/// assert_eq!(loc.metrics.cyclomatic.cyclomatic_sum(), 0.0);
/// assert!(cyc.metrics.cyclomatic.cyclomatic_sum() > 0.0);
/// assert_eq!(cyc.metrics.loc.ploc(), 0.0);
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
        let inner = crate::langs::ast_parse_dispatch(lang, code, preproc_path, preproc)?;
        Ok(Self { inner, name })
    }

    /// Adopt a caller-built [`tree_sitter::Tree`]. The `Source`-flavored
    /// counterpart of [`metrics_from_tree`]: same tree-reuse semantics, but
    /// with `name: Option<String>` carried end-to-end instead of derived
    /// from a path via lossy UTF-8 conversion.
    ///
    /// The supplied `tree` must have been produced from `code` with the
    /// [`tree_sitter::Language`] returned by
    /// [`LANG::get_tree_sitter_language`] for `lang`; a mismatch is not
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
}

/// Compute every metric for a [`Source`].
///
/// This is the recommended library entry point. Unlike the
/// deprecated [`metrics`] / [`metrics_with_options`] family it does
/// not conflate the top-level [`FuncSpace::name`] with a filesystem
/// path: callers supply an explicit `Source::name` and an optional
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

/// Returns all function spaces data of a code. This function needs a parser to
/// be created a priori in order to work.
///
/// Equivalent to calling [`metrics_with_options`] with
/// [`MetricsOptions::default`] — every node is visited and counted.
/// Existing callers (including `get_function_spaces` and the
/// `Metrics` callback used by the CLI) keep their previous behaviour
/// through this entry point. Pass an explicit [`MetricsOptions`]
/// (e.g. `exclude_tests: true`) to opt in to subtree filtering.
///
/// # Deprecated
///
/// Prefer [`analyze`], which accepts a [`Source`] carrying an explicit
/// display name distinct from any on-disk path.
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
/// ```
/// use std::path::Path;
///
/// # #[allow(deprecated)]
/// use big_code_analysis::{CppParser, metrics, ParserTrait};
///
/// let source_code = "int a = 42;";
///
/// // The path to a dummy file used to contain the source code
/// let path = Path::new("foo.c");
/// let source_as_vec = source_code.as_bytes().to_vec();
///
/// // The parser of the code, in this case a CPP parser
/// let parser = CppParser::new(source_as_vec, &path, None);
///
/// // Gets all function spaces data of the code contained in foo.c
/// # #[allow(deprecated)]
/// metrics(&parser, &path).unwrap();
/// ```
#[deprecated(
    since = "0.0.26",
    note = "Use `analyze(Source::new(lang, code).with_name(Some(name)), MetricsOptions::default())` instead — the path-positional shim derives the top-level FuncSpace name via lossy UTF-8 conversion."
)]
// Hidden from rustdoc because the signature exposes `ParserTrait` and
// `Parser<T>` — both demoted to `#[doc(hidden)]` per issue #256. The
// deprecation note already redirects callers to `analyze` / `Source`,
// which is the documented surface.
#[doc(hidden)]
pub fn metrics<'a, T: ParserTrait>(
    parser: &'a T,
    path: &'a Path,
) -> Result<FuncSpace, MetricsError> {
    #[allow(deprecated)]
    metrics_with_options(parser, path, MetricsOptions::default())
}

/// Like [`metrics`], but consults `options` while walking the AST.
///
/// Setting `options.exclude_tests = true` calls the language
/// [`Checker`]'s `should_skip_subtree` hook on every node and prunes
/// matching subtrees before any per-metric `compute` runs. The hook
/// defaults to `false` for every language, so passing
/// `exclude_tests = true` is a no-op except where a language module
/// overrides it (today: `RustCode`, which filters Rust `#[test]` /
/// `#[cfg(test)]` items).
///
/// Comment nodes are additionally scanned for in-source suppression
/// markers (see [`crate::suppression`]); any matches are attached to
/// the enclosing [`FuncSpace::suppressed`]. Malformed `bca:` markers
/// produce a warning to stderr — they do not abort the walk, so a
/// single typo in one file cannot derail a workspace-wide run.
///
/// # Deprecated
///
/// Prefer [`analyze`], which accepts a [`Source`] carrying an explicit
/// display name distinct from any on-disk path. This entry point
/// remains for backwards compatibility for one minor release; it
/// derives [`FuncSpace::name`] from `path` via lossy UTF-8 conversion.
///
/// # Errors
///
/// The return type carries [`MetricsError::EmptyRoot`] for forward
/// compatibility, but the walker always pushes a synthetic top-level
/// [`SpaceKind::Unit`][crate::SpaceKind] `FuncSpace` before walking,
/// so this function does not return `Err` in practice today (see
/// the variant doc).
#[deprecated(
    since = "0.0.26",
    note = "Use `analyze(Source::new(lang, code).with_name(Some(name)), options)` instead — the path-positional shim derives the top-level FuncSpace name via lossy UTF-8 conversion and will be removed in a future release."
)]
// Hidden from rustdoc — see `metrics` above for the rationale (#256).
#[doc(hidden)]
pub fn metrics_with_options<'a, T: ParserTrait>(
    parser: &'a T,
    path: &'a Path,
    options: MetricsOptions,
) -> Result<FuncSpace, MetricsError> {
    // Backwards-compat shim: derive the top-level name from `path` via
    // lossy UTF-8 conversion, matching pre-#254 behaviour. The new
    // `analyze` entry point lets callers supply a name explicitly.
    metrics_inner(parser, Some(path.to_string_lossy().into_owned()), options)
}

// Per-node metric dispatch. Each `compute` call is paired with a bit
// check against the caller's selection. The bit tests are cheap
// (single AND-and-compare on a u16) and an unselected metric saves
// both the call overhead and any per-node text-slice / token-table
// work the metric does internally — Halstead in particular owns
// `HalsteadMaps` allocations and is the headline cost saving for
// `with_only(&[Metric::Loc])`. Extracted from `metrics_inner` so the
// walker stays under clippy's 100-line ceiling.
#[inline]
fn compute_per_node<'a, T: ParserTrait>(
    state: &mut State<'a>,
    node: &Node<'a>,
    code: &'a [u8],
    selected: MetricSet,
    func_space: bool,
    unit: bool,
    nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
) {
    let last = &mut state.space;
    if selected.contains(Metric::Cognitive) {
        T::Cognitive::compute(node, code, &mut last.metrics.cognitive, nesting_map);
    }
    if selected.contains(Metric::Cyclomatic) {
        T::Cyclomatic::compute(node, code, &mut last.metrics.cyclomatic);
    }
    if selected.contains(Metric::Halstead) {
        T::Halstead::compute(node, code, &mut state.halstead_maps);
    }
    if selected.contains(Metric::Loc) {
        T::Loc::compute(node, &mut last.metrics.loc, func_space, unit);
    }
    if selected.contains(Metric::Nom) {
        T::Nom::compute(node, &mut last.metrics.nom);
    }
    if selected.contains(Metric::Tokens) {
        T::Tokens::compute(node, &mut last.metrics.tokens);
    }
    if selected.contains(Metric::NArgs) {
        T::NArgs::compute(node, &mut last.metrics.nargs);
    }
    if selected.contains(Metric::Exit) {
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
    let code = parser.get_code();
    let node = parser.get_root();
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

    // Some grammars (e.g. tree-sitter-mozcpp on unparseable input) return a
    // non-Unit root. Wrap with a synthetic Unit space spanning the whole
    // file so the top-level FuncSpace upholds the LOC invariant
    // `blank = sloc - ploc - only_comment_lines >= 0`.
    if T::Getter::get_space_kind(&node) != SpaceKind::Unit {
        let mut synthetic = FuncSpace::new::<T::Getter>(&node, code, SpaceKind::Unit, selected);
        synthetic
            .metrics
            .loc
            .init_unit_span(node.start_row(), node.end_row());
        state_stack.push(State {
            space: synthetic,
            halstead_maps: HalsteadMaps::new(),
        });
    }

    stack.push((node, 0));

    while let Some((node, level)) = stack.pop() {
        // Prune test-only subtrees before any per-metric work runs.
        // The hook is gated on `exclude_tests` so the default
        // `metrics()` entry point keeps emitting the pre-#182
        // numbers byte-for-byte.
        if options.exclude_tests && T::Checker::should_skip_subtree(&node, code) {
            continue;
        }

        if level < last_level {
            finalize::<T>(&mut state_stack, last_level - level, selected);
            last_level = level;
        }

        let kind = T::Getter::get_space_kind(&node);

        let func_space = T::Checker::is_func(&node) || T::Checker::is_func_space(&node);
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

        // Scan comment nodes for suppression markers and apply them
        // immediately against `state_stack`. Doing this inline (rather
        // than queueing for a post-walk pass keyed on line number)
        // pins each marker to the syntactically nearest enclosing
        // function space — the only frame on the stack that the
        // grammar nested the comment inside. Line-only matching was
        // ambiguous when two sibling functions shared a source line
        // and the first-by-source-order won regardless of which body
        // actually contained the comment (issue #289).
        if T::Checker::is_comment(&node)
            && let Some(text) = node.utf8_text(code)
        {
            match parse_suppression_marker(text) {
                Ok(Some(s)) => apply_suppression(&mut state_stack, &s),
                Ok(None) => {}
                Err(e) => {
                    // Logged but non-fatal so a typo in one file
                    // cannot derail a workspace-wide walk. The
                    // malformed marker is dropped (no scope attached),
                    // which is the conservative behaviour: a typo
                    // should not accidentally silence anything. The
                    // `+ 1` converts tree-sitter's 0-based rows to the
                    // 1-based line numbers `FuncSpace::start_line` and
                    // the rest of this module report.
                    eprintln!("warning: {}:{}: {e}", diagnostic_path, node.start_row() + 1);
                }
            }
        }

        if let Some(state) = state_stack.last_mut() {
            compute_per_node::<T>(
                state,
                &node,
                code,
                selected,
                func_space,
                unit,
                &mut nesting_map,
            );
        }

        cursor.reset(&node);
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
    // - `File`: the root `Unit` space, regardless of comment location.
    //   The synthetic Unit pushed by `metrics_inner` for non-Unit-root
    //   grammars and every translation-unit/module/source-file being a
    //   `func_space` keep `state_stack[0]` populated for every input.
    // - `Function`: the topmost `SpaceKind::Function` frame — the
    //   syntactically nearest enclosing function body. Class / struct
    //   / trait spaces are skipped so a marker at class scope but
    //   outside any method does not silence thresholds on the entire
    //   class; authors who want class-wide suppression use `bca:
    //   suppress-file` or repeat the marker on each method. A marker
    //   outside every function body finds no `Function` frame and is
    //   silently dropped — the issue's "no enclosing function" rule.
    let target = match suppression.kind {
        SuppressionKind::File => state_stack.first_mut(),
        SuppressionKind::Function => state_stack
            .iter_mut()
            .rev()
            .find(|s| matches!(s.space.kind, SpaceKind::Function)),
    };
    if let Some(state) = target {
        state.space.suppressed.merge(&suppression.scope);
    }
}

/// Per-traversal options for [`metrics_with_options`].
///
/// Marked `#[non_exhaustive]` so future option fields can land
/// additively. Downstream callers must construct via the builder
/// methods rather than struct-literal syntax (rustc rejects external
/// struct literals on non-exhaustive types with E0639, including the
/// `..Default::default()` spread form). The defaults preserve every
/// metric value emitted by the pre-#182 [`metrics`] entry point.
///
/// ```
/// use big_code_analysis::MetricsOptions;
/// let opts = MetricsOptions::default().with_exclude_tests(true);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct MetricsOptions {
    /// When true, the traversal asks the language module to skip
    /// test-only subtrees (e.g. Rust `#[test]` / `#[cfg(test)]`
    /// functions and modules). Only languages that override the
    /// internal `should_skip_subtree` hook honor this; others ignore
    /// the flag.
    pub exclude_tests: bool,
    /// Which metrics to compute. Defaults to [`MetricSet::all`] —
    /// every metric is enabled, matching the pre-#257 behaviour.
    /// Restrict via [`MetricsOptions::with_only`].
    pub metrics: MetricSet,
}

impl MetricsOptions {
    /// Builder-style setter for [`MetricsOptions::exclude_tests`].
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
}

/// Configuration options for computing the metrics of a code.
///
/// Marked `#[non_exhaustive]` so future config fields can land
/// additively. Downstream callers must construct via the builder
/// methods rather than struct-literal syntax (rustc rejects external
/// struct literals on non-exhaustive types with E0639, including the
/// `..Default::default()` spread form).
///
/// ```
/// use std::path::PathBuf;
/// use big_code_analysis::{MetricsCfg, MetricsOptions};
///
/// let cfg = MetricsCfg::new(PathBuf::from("lib.rs"))
///     .with_options(MetricsOptions::default().with_exclude_tests(true));
/// ```
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct MetricsCfg {
    /// Path to the file containing the code
    pub path: PathBuf,
    /// Per-traversal options forwarded to [`metrics_with_options`].
    pub options: MetricsOptions,
}

impl MetricsCfg {
    /// Build a `MetricsCfg` for `path` with default options. Chain
    /// [`MetricsCfg::with_options`] to override the per-traversal
    /// flags. Required because `MetricsCfg` is `#[non_exhaustive]` —
    /// downstream crates cannot use the struct-literal form.
    #[inline]
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            ..Default::default()
        }
    }

    /// Builder-style setter for [`MetricsCfg::options`].
    #[inline]
    #[must_use]
    pub fn with_options(mut self, options: MetricsOptions) -> Self {
        self.options = options;
        self
    }
}

/// Type tag identifying the metric-computation action; carries no data.
pub struct Metrics {
    _guard: (),
}

impl Callback for Metrics {
    type Res = std::io::Result<()>;
    type Cfg = MetricsCfg;

    fn call<T: ParserTrait>(cfg: Self::Cfg, parser: &T) -> Self::Res {
        // `MetricsCfg::path` is the legacy filesystem-keyed identity
        // for this callback. The new `analyze` entry point fully
        // supersedes the path-positional API, but this internal
        // callback site still has a `&Path` in hand, so use the
        // shared `metrics_inner` directly with a lossy-string name —
        // matching pre-#254 behaviour byte-for-byte.
        let name = Some(cfg.path.to_string_lossy().into_owned());
        match metrics_inner(parser, name, cfg.options) {
            Ok(space) => dump_root(&space),
            Err(_) => Ok(()),
        }
    }
}

#[cfg(test)]
// The lossy-path / synthetic-Unit tests below intentionally exercise
// the deprecated path-positional entry points so we have regression
// coverage on the shim even after the recommended seam moved to
// `analyze(Source { ... }, ...)`. Scope the deprecation allowance to
// the whole module so individual tests do not need per-call
// attributes.
#[allow(deprecated)]
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
    use crate::metrics;
    use crate::{CppParser, ParserTrait, SpaceKind, check_func_space};

    /// Regression for issue #285: every `Cpp::FunctionDefinition*` alias
    /// must be classified as a function space.
    ///
    /// `Checker::is_func` already enumerated all four variants, but the
    /// `is_func_space` check and both getters (`get_func_space_name`,
    /// `get_space_kind`) listed only the first three — an FD4 node would
    /// have been mistaken for a non-function space and yielded
    /// `SpaceKind::Unknown`.
    ///
    /// The current `tree-sitter-mozcpp` parse tables do not surface
    /// kind_ids 489/491/494 on any input we have been able to construct,
    /// so we cannot synthesise a real `Node` of those variants to feed
    /// the predicates directly. Lesson 2 in
    /// `docs/development/lessons_learned.md` warns that aliased
    /// `kind_id`s can be latent in the enum yet absent from observed
    /// parses; the next grammar bump may start emitting them.
    ///
    /// This test pins the structural contract by inspecting the
    /// **source text** of the four predicates (`is_func_space`,
    /// `is_func` in `checker.rs`, `get_func_space_name`, `get_space_kind`
    /// in `getter.rs`) and asserting each one explicitly names
    /// `FunctionDefinition4`. If a future edit drops the variant from
    /// any of those arms, this test fails immediately — without
    /// requiring a parse tree that exposes id 494.
    #[test]
    fn cpp_function_definition4_is_named_in_every_predicate() {
        // The predicates live under the workspace root; resolve against
        // CARGO_MANIFEST_DIR so the test is invariant to where `cargo
        // test` is invoked from.
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let checker_src = std::fs::read_to_string(format!("{manifest_dir}/src/checker.rs"))
            .expect("checker.rs must be readable for this regression test");
        let getter_src = std::fs::read_to_string(format!("{manifest_dir}/src/getter.rs"))
            .expect("getter.rs must be readable for this regression test");

        // Locate `impl Checker for CppCode { ... }`, then assert the
        // body mentions `FunctionDefinition4` at least twice (once for
        // `is_func_space`, once for `is_func`).
        let cpp_checker = extract_impl_block(&checker_src, "impl Checker for CppCode")
            .expect("could not find `impl Checker for CppCode` in checker.rs");
        assert!(
            cpp_checker.matches("FunctionDefinition4").count() >= 2,
            "issue #285 regression: `impl Checker for CppCode` must \
             reference Cpp::FunctionDefinition4 in both is_func_space \
             and is_func"
        );

        // Same for `impl Getter for CppCode { ... }` — both
        // `get_func_space_name` and `get_space_kind` must list FD4.
        let cpp_getter = extract_impl_block(&getter_src, "impl Getter for CppCode")
            .expect("could not find `impl Getter for CppCode` in getter.rs");
        assert!(
            cpp_getter.matches("FunctionDefinition4").count() >= 2,
            "issue #285 regression: `impl Getter for CppCode` must \
             reference Cpp::FunctionDefinition4 in both \
             get_func_space_name and get_space_kind"
        );
    }

    /// Returns the substring between the line containing `header` and
    /// the matching closing brace, scanning brace depth from `{` after
    /// `header`. Used by the FD4 regression test to read a single
    /// `impl` block without pulling the rest of the file into the
    /// match.
    fn extract_impl_block<'a>(source: &'a str, header: &str) -> Option<&'a str> {
        let start = source.find(header)?;
        let open = start + source[start..].find('{')?;
        let mut depth = 0i32;
        for (i, b) in source[open..].bytes().enumerate() {
            match b {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&source[open..=open + i]);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Positive coverage for the C++ function-space predicates on the
    /// only `function_definition` `kind_id` (343) that
    /// `tree-sitter-mozcpp` currently emits. The structural
    /// `FunctionDefinition4` contract is locked separately by
    /// `cpp_function_definition4_is_named_in_every_predicate`.
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
        let root = parser.get_root();

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
    fn c_scope_resolution_operator() {
        check_func_space::<CppParser, _>(
            "void Foo::bar(){
                return;
            }",
            "foo.c",
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
            parser.get_root().0.is_error(),
            "test premise broken: grammar must yield ERROR root for this snippet"
        );

        let space = metrics(&parser, &path).unwrap();

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
        assert!(blank >= 0.0, "blank ({blank}) must be >= 0");
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
    /// assert `parser.get_root().0.is_error()` before calling this
    /// helper. See `cpp_error_root_yields_unit_top_level_space` and
    /// `lua_partial_input_yields_synthetic_unit_wrapper` — those two
    /// are the only tests in the corpus that today exercise the
    /// wrapper path. Issue #220 tracks finding additional per-grammar
    /// fixtures that surface ERROR roots so each language can have
    /// both a contract test and a wrapper-exercising test.
    fn assert_top_level_space_is_unit_contract<P: ParserTrait>(source: &str, filename: &str) {
        let path = std::path::PathBuf::from(filename);
        let parser = P::new(source.as_bytes().to_vec(), &path, None);
        let space = metrics(&parser, &path).expect("metrics must yield a top-level space");
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
        assert!(
            blank >= 0.0,
            "blank ({blank}) must be >= 0 for the file-level space of {filename:?}",
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
            parser.get_root().0.is_error(),
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

    /// Regression for issue #128 — the deprecated path-positional
    /// entry point still derives the top-level name from `path` via
    /// lossy UTF-8 conversion. Even when the original bytes are not
    /// valid UTF-8 on Linux (valid on ext4/tmpfs/etc.), the top-level
    /// name must be `Some(...)` (never the parse-error sentinel
    /// `None`) so downstream JSON consumers can distinguish the two
    /// cases.
    ///
    /// After #254, callers who want to avoid the lossy round-trip
    /// pass an explicit `Source::name` to [`analyze`] (see the
    /// `analyze_in_memory_snippet_carries_caller_supplied_name`
    /// test below).
    #[cfg(unix)]
    #[test]
    fn non_utf8_path_yields_lossy_top_level_name() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        use std::path::PathBuf;

        // Bytes that are not valid UTF-8 (lone continuation + invalid
        // start byte) framed with ASCII so the resulting filename
        // unambiguously contains the U+FFFD replacement character after
        // lossy conversion.
        let raw_bytes: &[u8] = b"foo_\xFF\xFE_bar.rs";
        let path = PathBuf::from(OsStr::from_bytes(raw_bytes));
        assert!(
            path.to_str().is_none(),
            "test premise broken: path must be non-UTF-8 for this test to be meaningful"
        );

        let source = "int a = 42;";
        let parser = CppParser::new(source.as_bytes().to_vec(), &path, None);
        #[allow(deprecated)]
        let space = metrics(&parser, &path).expect("metrics must yield a top-level space");

        let name = space
            .name
            .as_deref()
            .expect("top-level FuncSpace name must be Some, not the parse-error sentinel None");
        assert!(
            name.contains('\u{FFFD}'),
            "expected U+FFFD replacement char in lossy name, got {name:?}"
        );
        assert!(
            name.starts_with("foo_") && name.ends_with("_bar.rs"),
            "lossy name must preserve the surrounding ASCII bytes, got {name:?}"
        );
    }

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

    // --- #182: exclude_tests for Rust -----------------------------
    //
    // These exercise both flag values (`exclude_tests = false` is
    // the documented backward-compatible default; `true` opts in to
    // the new pruning). They are anchored on integer-valued
    // accessors (`nom_functions_sum`, `cyclomatic_sum`,
    // `cognitive_sum`, `n_operators`) rather than float magnitudes,
    // because Halstead floats are bit-brittle (lessons_learned.md).

    mod exclude_tests_rust {
        use crate::metrics_with_options;
        use crate::{MetricsOptions, ParserTrait, RustParser};
        use std::path::PathBuf;

        fn analyse(source: &str, exclude_tests: bool) -> crate::FuncSpace {
            let path = PathBuf::from("lib.rs");
            let parser = RustParser::new(source.as_bytes().to_vec(), &path, None);
            metrics_with_options(
                &parser,
                &path,
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
    }

    // Non-Rust languages must ignore `exclude_tests = true` because
    // they don't override `should_skip_subtree`. This is the
    // "spot-check non-Rust" check from issue #182.
    mod exclude_tests_non_rust {
        use crate::metrics_with_options;
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
            let baseline = metrics_with_options(
                &parser,
                &path,
                MetricsOptions::default().with_exclude_tests(false),
            )
            .expect("baseline must yield a top-level space");
            let parser = CppParser::new(source.as_bytes().to_vec(), &path, None);
            let pruned = metrics_with_options(
                &parser,
                &path,
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
            assert!(pruned.metrics.loc.ploc() >= 1.0);
            // Full run has > 0 cognitive/cyclomatic; pruned must be
            // exactly zero because the compute call is gated off.
            assert!(full.metrics.cognitive.cognitive_sum() > 0.0);
            assert_eq!(pruned.metrics.cognitive.cognitive_sum(), 0.0);
            assert!(full.metrics.cyclomatic.cyclomatic_sum() > 0.0);
            assert_eq!(pruned.metrics.cyclomatic.cyclomatic_sum(), 0.0);
            // Halstead operators count is at the default (0) — no
            // per-node token text was hashed.
            assert_eq!(pruned.metrics.halstead.u_operators(), 0.0);
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
                pruned.metrics.loc.ploc() > 0.0,
                "Loc must have run (Mi dependency); got ploc=0"
            );
            assert!(
                pruned.metrics.cyclomatic.cyclomatic_sum() > 0.0,
                "Cyclomatic must have run (Mi dependency); got sum=0"
            );
            // With non-zero inputs feeding the MI formula, the result
            // is a finite non-zero number (the MI for this snippet is
            // around 150 — a positive value well above the 0.0 that
            // `inputs_are_empty` would short-circuit to).
            let mi_value = pruned.metrics.mi.mi_original();
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
                pruned.metrics.cyclomatic.cyclomatic_sum() > 0.0,
                "Cyclomatic must have run (Wmc dependency); got sum=0"
            );
            assert!(
                pruned.metrics.nom.functions_sum() > 0.0,
                "Nom must have run (Wmc dependency); got functions_sum=0"
            );
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
    }
}
