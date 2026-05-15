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
use std::fmt;
use std::path::{Path, PathBuf};

use crate::checker::Checker;
use crate::node::Node;

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

use crate::dump_metrics::*;
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
#[derive(Default, Debug, Clone, Serialize)]
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
    #[serde(skip_serializing_if = "wmc::Stats::is_disabled")]
    pub wmc: wmc::Stats,
    /// `Npm` data
    #[serde(skip_serializing_if = "npm::Stats::is_disabled")]
    pub npm: npm::Stats,
    /// `Npa` data
    #[serde(skip_serializing_if = "npa::Stats::is_disabled")]
    pub npa: npa::Stats,
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
    }
}

/// Function space data.
#[derive(Debug, Clone, Serialize)]
pub struct FuncSpace {
    /// The name of a function space.
    ///
    /// For the top-level (file-level) `FuncSpace`, this is the file path
    /// supplied to [`metrics`] converted via lossy UTF-8 conversion, so it
    /// is always `Some`. Non-UTF-8 path components on Linux (or invalid
    /// UTF-16 on Windows) become U+FFFD replacement characters; in that
    /// case [`FuncSpace::name_was_lossy`] is `true` and downstream
    /// consumers must treat the name as display-only — never as a map
    /// key or for error correlation.
    ///
    /// For nested spaces, `None` means an error occurred in parsing the
    /// name of the function space from the AST.
    pub name: Option<String>,
    /// `true` when [`FuncSpace::name`] was produced by lossy conversion
    /// (the original path contained non-UTF-8 bytes and was rendered
    /// using U+FFFD replacement characters). Always `false` for nested
    /// spaces and for top-level spaces with valid-UTF-8 paths. Skipped
    /// from JSON output when `false` so existing schemas keep their shape.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub name_was_lossy: bool,
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
}

impl FuncSpace {
    fn new<T: Getter>(node: &Node, code: &[u8], kind: SpaceKind) -> Self {
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

        // The top-level Unit's name is unconditionally overwritten with the
        // file path by `metrics()` before returning, so computing it here is
        // wasted work. Other kinds keep the AST-derived name.
        let name = (kind != SpaceKind::Unit)
            .then(|| {
                T::get_func_space_name(node, code)
                    .map(|name| name.split_whitespace().collect::<Vec<_>>().join(" "))
            })
            .flatten();

        Self {
            name,
            name_was_lossy: false,
            spaces: Vec::new(),
            metrics: CodeMetrics::default(),
            kind,
            start_line: start_position,
            end_line: end_position,
        }
    }
}

#[inline]
fn compute_halstead_mi_and_wmc<T: ParserTrait>(state: &mut State) {
    state
        .halstead_maps
        .finalize(&mut state.space.metrics.halstead);
    T::Mi::compute(
        &state.space.metrics.loc,
        &state.space.metrics.cyclomatic,
        &state.space.metrics.halstead,
        &mut state.space.metrics.mi,
    );
    T::Wmc::compute(
        state.space.kind,
        &state.space.metrics.cyclomatic,
        &mut state.space.metrics.wmc,
    );
}

#[inline]
fn compute_averages(state: &mut State) {
    let nom_functions = state.space.metrics.nom.functions_sum() as usize;
    let nom_closures = state.space.metrics.nom.closures_sum() as usize;
    let nom_total = state.space.metrics.nom.total() as usize;
    // Cognitive average
    state.space.metrics.cognitive.finalize(nom_total);
    // Nexit average
    state.space.metrics.nexits.finalize(nom_total);
    // Nargs average
    state
        .space
        .metrics
        .nargs
        .finalize(nom_functions, nom_closures);
}

#[inline]
fn compute_minmax(state: &mut State) {
    state.space.metrics.cyclomatic.compute_minmax();
    state.space.metrics.nexits.compute_minmax();
    state.space.metrics.cognitive.compute_minmax();
    state.space.metrics.nargs.compute_minmax();
    state.space.metrics.nom.compute_minmax();
    state.space.metrics.loc.compute_minmax();
    state.space.metrics.abc.compute_minmax();
    state.space.metrics.tokens.compute_minmax();
}

#[inline]
fn compute_sum(state: &mut State) {
    state.space.metrics.wmc.compute_sum();
    state.space.metrics.npm.compute_sum();
    state.space.metrics.npa.compute_sum();
}

fn finalize<T: ParserTrait>(state_stack: &mut Vec<State>, diff_level: usize) {
    if state_stack.is_empty() {
        return;
    }
    for _ in 0..diff_level {
        if state_stack.len() == 1 {
            let last_state = state_stack
                .last_mut()
                .expect("invariant: state_stack has exactly one element");
            compute_minmax(last_state);
            compute_sum(last_state);
            compute_halstead_mi_and_wmc::<T>(last_state);
            compute_averages(last_state);
            break;
        }
        let mut state = state_stack
            .pop()
            .expect("invariant: state_stack has more than one element");
        compute_minmax(&mut state);
        compute_sum(&mut state);
        compute_halstead_mi_and_wmc::<T>(&mut state);
        compute_averages(&mut state);

        let last_state = state_stack
            .last_mut()
            .expect("invariant: state_stack has remaining elements after pop");
        last_state.halstead_maps.merge(&state.halstead_maps);
        compute_halstead_mi_and_wmc::<T>(last_state);

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

/// Returns all function spaces data of a code. This function needs a parser to
/// be created a priori in order to work.
///
/// Equivalent to calling [`metrics_with_options`] with
/// [`MetricsOptions::default`] — every node is visited and counted.
/// Existing callers (including [`get_function_spaces`] and the
/// `Metrics` callback used by the CLI) keep their previous behaviour
/// through this entry point. Pass an explicit [`MetricsOptions`]
/// (e.g. `exclude_tests: true`) to opt in to subtree filtering.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
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
/// metrics(&parser, &path).unwrap();
/// ```
pub fn metrics<'a, T: ParserTrait>(parser: &'a T, path: &'a Path) -> Option<FuncSpace> {
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
pub fn metrics_with_options<'a, T: ParserTrait>(
    parser: &'a T,
    path: &'a Path,
    options: MetricsOptions,
) -> Option<FuncSpace> {
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

    // Some grammars (e.g. tree-sitter-mozcpp on unparseable input) return a
    // non-Unit root. Wrap with a synthetic Unit space spanning the whole
    // file so the top-level FuncSpace upholds the LOC invariant
    // `blank = sloc - ploc - only_comment_lines >= 0`.
    if T::Getter::get_space_kind(&node) != SpaceKind::Unit {
        let mut synthetic = FuncSpace::new::<T::Getter>(&node, code, SpaceKind::Unit);
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
            finalize::<T>(&mut state_stack, last_level - level);
            last_level = level;
        }

        let kind = T::Getter::get_space_kind(&node);

        let func_space = T::Checker::is_func(&node) || T::Checker::is_func_space(&node);
        let unit = kind == SpaceKind::Unit;

        let new_level = if func_space {
            let state = State {
                space: FuncSpace::new::<T::Getter>(&node, code, kind),
                halstead_maps: HalsteadMaps::new(),
            };
            state_stack.push(state);
            last_level = level + 1;
            last_level
        } else {
            level
        };

        if let Some(state) = state_stack.last_mut() {
            let last = &mut state.space;
            T::Cognitive::compute(&node, &mut last.metrics.cognitive, &mut nesting_map);
            T::Cyclomatic::compute(&node, code, &mut last.metrics.cyclomatic);
            T::Halstead::compute(&node, code, &mut state.halstead_maps);
            T::Loc::compute(&node, &mut last.metrics.loc, func_space, unit);
            T::Nom::compute(&node, &mut last.metrics.nom);
            T::Tokens::compute(&node, &mut last.metrics.tokens);
            T::NArgs::compute(&node, &mut last.metrics.nargs);
            T::Exit::compute(&node, code, &mut last.metrics.nexits);
            T::Abc::compute(&node, &mut last.metrics.abc);
            T::Npm::compute(&node, code, &mut last.metrics.npm);
            T::Npa::compute(&node, code, &mut last.metrics.npa);
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

    finalize::<T>(&mut state_stack, usize::MAX);

    state_stack.pop().map(|mut state| {
        // `path.to_str()` returns `None` for non-UTF-8 paths (valid on
        // Linux ext4/tmpfs, and possible on Windows for invalid UTF-16),
        // which would silently collapse into the same `None` that signals
        // a parse error for nested spaces. Use lossy conversion so the
        // top-level space is always identifiable, and surface the lossy
        // bit in `name_was_lossy` so consumers can opt out of using the
        // U+FFFD-bearing name as an identifier (see the doc comment on
        // `FuncSpace::name`).
        let was_lossy = path.to_str().is_none();
        state.space.name = Some(path.to_string_lossy().into_owned());
        state.space.name_was_lossy = was_lossy;
        state.space
    })
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
    /// When true, the traversal asks the language [`Checker`] to
    /// skip test-only subtrees (e.g. Rust `#[test]` / `#[cfg(test)]`
    /// functions and modules). Only language modules that override
    /// [`Checker::should_skip_subtree`] honor this; others ignore
    /// the flag.
    pub exclude_tests: bool,
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
        match metrics_with_options(parser, &cfg.path, cfg.options) {
            Some(space) => dump_root(&space),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
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
    use crate::{CppParser, ParserTrait, RubyParser, SpaceKind, check_func_space, metrics};

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

    /// Robustness contract for malformed Ruby: tree-sitter-ruby tolerates
    /// nearly any input and returns a `program` (Unit) root, so the
    /// synthetic-Unit fallback path is unreachable today. This test pins
    /// the contract — top-level kind is `Unit`, `sloc >= ploc`, and
    /// `blank >= 0` — so a future grammar bump that starts promoting an
    /// inner `Method`/`Class` to root on partial input would fail here
    /// instead of silently producing a non-Unit top-level FuncSpace.
    /// Lesson 9 (`docs/development/lessons_learned.md`).
    #[test]
    fn ruby_partial_input_yields_unit_top_level_space() {
        // Truncated method definition (missing `end`) plus a stray
        // unbalanced sigil — tree-sitter-ruby treats both as ERROR
        // children of `program`.
        let source = "class Foo\n  def bar(\n    x\n  ";
        let path = std::path::PathBuf::from("partial.rb");
        let parser = RubyParser::new(source.as_bytes().to_vec(), &path, None);

        let space = metrics(&parser, &path).expect("metrics must yield a top-level space");

        assert_eq!(
            space.kind,
            SpaceKind::Unit,
            "top-level FuncSpace must be Unit, not {:?}",
            space.kind,
        );
        let loc = &space.metrics.loc;
        assert!(
            loc.sloc() >= loc.ploc(),
            "sloc ({}) must be >= ploc ({})",
            loc.sloc(),
            loc.ploc(),
        );
        assert!(loc.blank() >= 0.0, "blank ({}) must be >= 0", loc.blank());
    }

    /// Regression for issue #128 — non-UTF-8 paths on Linux (valid on
    /// ext4/tmpfs/etc.) must not be silently collapsed into `name: None`,
    /// which is the sentinel for AST-name parse failures and would be
    /// indistinguishable in JSON output.
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
        assert!(
            space.name_was_lossy,
            "name_was_lossy must be true when the source path was non-UTF-8"
        );
    }

    /// Top-level spaces with valid UTF-8 paths must NOT have
    /// `name_was_lossy` set — otherwise the flag is useless.
    #[test]
    fn utf8_path_does_not_set_name_was_lossy() {
        use std::path::PathBuf;
        let path = PathBuf::from("foo.cpp");
        let source = "int a = 42;";
        let parser = CppParser::new(source.as_bytes().to_vec(), &path, None);
        let space = metrics(&parser, &path).expect("metrics must yield a top-level space");
        assert!(
            !space.name_was_lossy,
            "name_was_lossy must be false for valid-UTF-8 paths"
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
        use crate::{MetricsOptions, ParserTrait, RustParser, metrics_with_options};
        use std::path::PathBuf;

        fn analyse(source: &str, exclude_tests: bool) -> crate::FuncSpace {
            let path = PathBuf::from("lib.rs");
            let parser = RustParser::new(source.as_bytes().to_vec(), &path, None);
            metrics_with_options(&parser, &path, MetricsOptions { exclude_tests })
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
";
            let pruned = analyse(source, true);
            // None of the three attributes mark test-only code.
            // All three functions must survive.
            assert_eq!(pruned.metrics.nom.functions_sum() as usize, 3);
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
        use crate::{CppParser, MetricsOptions, ParserTrait, metrics_with_options};
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
                MetricsOptions {
                    exclude_tests: false,
                },
            )
            .expect("baseline must yield a top-level space");
            let parser = CppParser::new(source.as_bytes().to_vec(), &path, None);
            let pruned = metrics_with_options(
                &parser,
                &path,
                MetricsOptions {
                    exclude_tests: true,
                },
            )
            .expect("pruned must yield a top-level space");
            // Anchor on the absolute count (2) so a regression that
            // dropped all C++ functions wouldn't satisfy a bare
            // `baseline == pruned` check.
            assert_eq!(baseline.metrics.nom.functions_sum() as usize, 2);
            assert_eq!(pruned.metrics.nom.functions_sum() as usize, 2);
        }
    }
}
