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
    /// The name of a function space
    ///
    /// If `None`, an error is occurred in parsing
    /// the name of a function space
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
            spaces: Vec::new(),
            metrics: CodeMetrics::default(),
            kind,
            start_line: start_position,
            end_line: end_position,
        }
    }
}

#[inline(always)]
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

#[inline(always)]
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

#[inline(always)]
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

#[inline(always)]
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
            let last_state = state_stack.last_mut().unwrap();
            compute_minmax(last_state);
            compute_sum(last_state);
            compute_halstead_mi_and_wmc::<T>(last_state);
            compute_averages(last_state);
            break;
        } else {
            let mut state = state_stack.pop().unwrap();
            compute_minmax(&mut state);
            compute_sum(&mut state);
            compute_halstead_mi_and_wmc::<T>(&mut state);
            compute_averages(&mut state);

            let last_state = state_stack.last_mut().unwrap();
            last_state.halstead_maps.merge(&state.halstead_maps);
            compute_halstead_mi_and_wmc::<T>(last_state);

            // Merge function spaces
            last_state.space.metrics.merge(&state.space.metrics);
            last_state.space.spaces.push(state.space);
        }
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
            T::Cyclomatic::compute(&node, &mut last.metrics.cyclomatic);
            T::Halstead::compute(&node, code, &mut state.halstead_maps);
            T::Loc::compute(&node, &mut last.metrics.loc, func_space, unit);
            T::Nom::compute(&node, &mut last.metrics.nom);
            T::Tokens::compute(&node, &mut last.metrics.tokens);
            T::NArgs::compute(&node, &mut last.metrics.nargs);
            T::Exit::compute(&node, code, &mut last.metrics.nexits);
            T::Abc::compute(&node, &mut last.metrics.abc);
            T::Npm::compute(&node, &mut last.metrics.npm);
            T::Npa::compute(&node, &mut last.metrics.npa);
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
        state.space.name = path.to_str().map(|name| name.to_string());
        state.space
    })
}

/// Configuration options for computing
/// the metrics of a code.
#[derive(Debug)]
pub struct MetricsCfg {
    /// Path to the file containing the code
    pub path: PathBuf,
}

pub struct Metrics {
    _guard: (),
}

impl Callback for Metrics {
    type Res = std::io::Result<()>;
    type Cfg = MetricsCfg;

    fn call<T: ParserTrait>(cfg: Self::Cfg, parser: &T) -> Self::Res {
        match metrics(parser, &cfg.path) {
            Some(space) => dump_root(&space),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{CppParser, ParserTrait, SpaceKind, check_func_space, metrics};

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
}
