// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(
    clippy::enum_glob_use,
    clippy::if_not_else,
    clippy::too_many_lines,
    clippy::wildcard_imports
)]

use std::collections::{HashMap, HashSet, hash_map};
use std::path::{Path, PathBuf};

use petgraph::{
    Direction,
    algo::{kosaraju_scc, toposort},
    graph::NodeIndex,
    stable_graph::StableGraph,
    visit::{Dfs, NodeIndexable},
};
use serde::{Deserialize, Serialize};

use crate::c_langs_macros::is_specials;

use crate::langs::*;
use crate::languages::language_preproc::*;
use crate::node::{Cursor, Node};
use crate::tools::*;
use crate::traits::*;

/// A non-fatal diagnostic produced while resolving the C/C++ include
/// graph in [`fix_includes`].
///
/// Resolution is best-effort: self-inclusions, include cycles, paths
/// that cannot be decoded as UTF-8, and files referenced but never
/// preprocessed are all reported here rather than written to `stderr`,
/// so an embedder (e.g. `bca-web`) can capture, suppress, or surface
/// them as it sees fit. The CLI prints them to `stderr` through its
/// `warning:` helper.
///
/// [`Display`](std::fmt::Display) renders the bare message with no
/// severity prefix and no trailing newline; prefixing is the presenting
/// layer's job, so the prefix is written in exactly one place per crate
/// (#1199).
// `Ord` exists so [`fix_includes`] can return a stable sequence; see the
// sort there for why. The derived order is by variant declaration first,
// then by field, which groups a run's diagnostics by kind and orders each
// kind by path — the shape a reader scanning `bca preproc` warnings wants.
// It is an output-ordering aid, not a severity ranking.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum PreprocDiagnostic {
    /// A file's `#include` resolved back to the file itself; the
    /// self-edge was skipped.
    SelfInclusion {
        /// The file that includes itself.
        file: PathBuf,
    },
    /// A strongly connected component (an include cycle) was collapsed
    /// into a single replacement node. Carries the member paths.
    IncludeCycle {
        /// The files participating in the cycle.
        members: Vec<String>,
    },
    /// A path could not be decoded as UTF-8 and was skipped while
    /// collapsing an include cycle.
    NonUtf8CyclePath {
        /// The lossy rendering of the offending path.
        path: String,
    },
    /// A path could not be decoded as UTF-8 and was skipped while
    /// recording indirect includes.
    NonUtf8IndirectInclude {
        /// The lossy rendering of the offending path.
        path: String,
    },
    /// A file appears in the include graph but was never preprocessed,
    /// so its own macros and includes are unknown.
    NotPreprocessed {
        /// The file referenced but not preprocessed.
        file: PathBuf,
    },
}

impl std::fmt::Display for PreprocDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SelfInclusion { file } => {
                write!(f, "possible self inclusion {}", file.display())
            }
            Self::IncludeCycle { members } => {
                write!(f, "possible include cycle:")?;
                for member in members {
                    // Explicit quotes preserve whitespace visibility for
                    // paths that contain spaces — important when the cycle
                    // warning is the only signal a user gets.
                    //
                    // The newline leads rather than trails so the rendered
                    // block carries no trailing newline of its own: a
                    // `Display` that ends in one stacks with the caller's
                    // `println!`/`warn` and prints a stray blank line
                    // (#1199).
                    write!(f, "\n  - \"{member}\"")?;
                }
                Ok(())
            }
            Self::NonUtf8CyclePath { path } => {
                write!(f, "skipping non-UTF-8 path in include cycle: {path}")
            }
            Self::NonUtf8IndirectInclude { path } => {
                write!(f, "skipping non-UTF-8 indirect include path: {path}")
            }
            Self::NotPreprocessed { file } => write!(
                f,
                "included file which has not been preprocessed: {}",
                file.display()
            ),
        }
    }
}

/// Preprocessor data of a `C/C++` file.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct PreprocFile {
    /// The set of include directives explicitly written in a file
    pub direct_includes: HashSet<String>,
    /// The set of include directives implicitly imported in a file
    /// from other files
    pub indirect_includes: HashSet<String>,
    /// The set of macros of a file
    pub macros: HashSet<String>,
}

/// Preprocessor data of a series of `C/C++` files.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct PreprocResults {
    /// The preprocessor data of each `C/C++` file
    pub files: HashMap<PathBuf, PreprocFile>,
}

impl PreprocFile {
    /// Builds a new `PreprocFile` whose macro set contains the given
    /// macro names (and no includes).
    #[must_use]
    pub fn new_macros(macros: &[&str]) -> Self {
        let mut pf = Self::default();
        for m in macros {
            pf.macros.insert((*m).to_string());
        }
        pf
    }
}

crate::observation::counter!(owned_macro_sets);

/// Returns the macros contained in a `C/C++` file.
pub fn get_macros<S: ::std::hash::BuildHasher>(
    file: &Path,
    files: &HashMap<PathBuf, PreprocFile, S>,
) -> HashSet<String> {
    // Counts owned copies of a file's visible macro set. `Parser::new` used
    // to build one per C/C++ file parsed and then only ever ask it
    // `contains`; it now borrows through [`visible_macros`], so this stays
    // at zero across a parse — which a parse's output cannot show.
    owned_macro_sets::record();
    visible_macros(file, files)
        .into_iter()
        .map(ToOwned::to_owned)
        .collect()
}

/// Every macro name visible to `file` — its own `#define`s plus those of
/// every header it transitively includes — borrowed from `files`.
///
/// The crate-internal form of [`get_macros`], which owns the same names
/// only because its `HashSet<String>` return type is published. Callers
/// inside the crate just probe the result with `contains`, so borrowing
/// suffices (issue #1107). Still a *merged* set and not a list of
/// per-file sets to probe in turn, because it is consulted once per
/// identifier run in the whole translation unit: an `O(headers)` probe
/// would cost far more than the one-off merge it saves.
pub(crate) fn visible_macros<'a, S: ::std::hash::BuildHasher>(
    file: &Path,
    files: &'a HashMap<PathBuf, PreprocFile, S>,
) -> HashSet<&'a str> {
    let mut macros = HashSet::new();
    let Some(pf) = files.get(file) else {
        return macros;
    };
    macros.extend(pf.macros.iter().map(String::as_str));
    for include in &pf.indirect_includes {
        // `Path::new` re-types the borrowed `str` in place; the
        // `PathBuf::from` it replaced allocated once per indirect
        // include, on every parse, purely to bridge to this map's key
        // type.
        if let Some(included) = files.get(Path::new(include)) {
            macros.extend(included.macros.iter().map(String::as_str));
        }
    }
    macros
}

/// The include dependency graph: nodes are file paths, edges point from a
/// file to each file it directly includes. SCC replacement nodes carry an
/// empty [`PathBuf`] as their weight.
type IncludeGraph = StableGraph<PathBuf, i32>;

/// Returns the graph node for `file`, inserting one (and recording it in
/// `nodes`) on first lookup so that repeat lookups of the same path return a
/// stable [`NodeIndex`]. The owned-path call site pays one extra clone here,
/// which is allocation only and never affects output.
fn ensure_node(
    g: &mut IncludeGraph,
    nodes: &mut HashMap<PathBuf, NodeIndex>,
    file: &Path,
) -> NodeIndex {
    match nodes.entry(file.to_path_buf()) {
        hash_map::Entry::Occupied(l) => *l.get(),
        hash_map::Entry::Vacant(p) => *p.insert(g.add_node(file.to_path_buf())),
    }
}

/// Resolves an `#include` to a single, deterministic target.
///
/// [`guess_file`]'s last-resort `min_distance_candidates` fallback can
/// return several tied candidates (a basename like `config.h` living in
/// multiple directories). Adding an edge to *every* tied candidate would
/// leak macros from unrelated files through [`get_macros`] and make the
/// resolved set depend on `all_files` Vec ordering. We instead pick the
/// lexicographically smallest path among the ties — a stable, content-
/// independent tie-break — and document the choice as best-effort.
fn resolve_single_include<S: ::std::hash::BuildHasher>(
    file: &Path,
    include: &str,
    all_files: &HashMap<String, Vec<PathBuf>, S>,
) -> Option<PathBuf> {
    guess_file(file, include, all_files).into_iter().min()
}

/// Builds the include dependency graph from the preprocessor data: one node
/// per file, one edge per resolved direct include. Each include resolves to a
/// single deterministic target (see [`resolve_single_include`]). Self-
/// inclusions are reported as a diagnostic and skipped rather than added as
/// self-edges. Returns the graph, the path→node map, and any diagnostics.
fn build_include_graph<S: ::std::hash::BuildHasher>(
    files: &HashMap<PathBuf, PreprocFile, S>,
    all_files: &HashMap<String, Vec<PathBuf>, S>,
    diagnostics: &mut Vec<PreprocDiagnostic>,
) -> (IncludeGraph, HashMap<PathBuf, NodeIndex>) {
    let mut nodes: HashMap<PathBuf, NodeIndex> = HashMap::new();
    // Since we'll remove strong connected components we need to have a stable graph
    // in order to use the nodes we've in the nodes HashMap.
    let mut g = StableGraph::new();

    for (file, pf) in files {
        let node = ensure_node(&mut g, &mut nodes, file);
        for i in &pf.direct_includes {
            let Some(included) = resolve_single_include(file, i, all_files) else {
                continue;
            };
            if &included == file {
                diagnostics.push(PreprocDiagnostic::SelfInclusion { file: file.clone() });
                continue;
            }
            let included = ensure_node(&mut g, &mut nodes, &included);
            g.add_edge(node, included, 0);
        }
    }

    (g, nodes)
}

/// Collects the neighbors of `component` in the given `direction` that lie
/// outside the component, de-duplicated and in first-seen order. Intra-
/// component edges are excluded so the replacement node only re-wires the
/// SCC's external boundary. A `Vec` (not a `HashSet`) suffices: SCCs in real
/// codebases are few and small, so linear `contains` checks stay cheap.
fn scc_external_neighbors(
    g: &IncludeGraph,
    component: &[NodeIndex],
    direction: Direction,
) -> Vec<NodeIndex> {
    let mut neighbors = Vec::new();
    for c in component {
        for n in g.neighbors_directed(*c, direction) {
            if !component.contains(&n) && !neighbors.contains(&n) {
                neighbors.push(n);
            }
        }
    }
    neighbors
}

/// Replaces every strongly connected component (an include cycle) with a
/// single replacement node carrying an empty path, re-wiring the component's
/// external incoming/outgoing edges onto it and rewriting the `nodes` map so
/// each member path now resolves to the replacement. Returns a map from each
/// replacement node to the set of member paths it stands in for.
fn collapse_scc(
    g: &mut IncludeGraph,
    nodes: &mut HashMap<PathBuf, NodeIndex>,
    diagnostics: &mut Vec<PreprocDiagnostic>,
) -> HashMap<NodeIndex, HashSet<String>> {
    // In order to walk in the graph without issues due to cycles
    // we replace strong connected components by a unique node
    // All the paths in a scc finally represents a kind of unique file containing
    // all the files in the scc.
    let mut scc = kosaraju_scc(&*g);
    let mut scc_map: HashMap<NodeIndex, HashSet<String>> = HashMap::new();
    for component in &mut scc {
        // A single-node "component" is not a cycle and needs no replacement.
        if component.len() > 1 {
            let (replacement, paths) = collapse_one_component(g, nodes, diagnostics, component);
            scc_map.insert(replacement, paths);
        }
    }
    scc_map
}

/// Replace one strongly connected component with a single empty-path node,
/// re-wiring its external edges and repointing every member's `nodes` entry at
/// the replacement. Returns the replacement node and its member paths.
///
/// Split out of [`collapse_scc`] because that function's whole body was this
/// operation nested inside a `for` and an `if`; naming the per-component step
/// leaves the caller reading as "for each cycle, collapse it".
fn collapse_one_component(
    g: &mut IncludeGraph,
    nodes: &mut HashMap<PathBuf, NodeIndex>,
    diagnostics: &mut Vec<PreprocDiagnostic>,
    component: &mut Vec<NodeIndex>,
) -> (NodeIndex, HashSet<String>) {
    // External boundaries must be captured before the replacement node is
    // added, so the new node is never mistaken for an external neighbor.
    let incoming = scc_external_neighbors(g, component, Direction::Incoming);
    let outgoing = scc_external_neighbors(g, component, Direction::Outgoing);
    let mut paths = HashSet::new();

    let replacement = g.add_node(PathBuf::from(""));
    for i in incoming {
        g.add_edge(i, replacement, 0);
    }
    for o in outgoing {
        g.add_edge(replacement, o, 0);
    }
    for c in component.drain(..) {
        let path = g
            .remove_node(c)
            .expect("invariant: SCC component node must exist in graph");
        if let Some(s) = path.to_str() {
            paths.insert(s.to_string());
        } else {
            diagnostics.push(PreprocDiagnostic::NonUtf8CyclePath {
                path: path.display().to_string(),
            });
        }
        *nodes
            .get_mut(&path)
            .expect("invariant: every graph node must have a nodes map entry") = replacement;
    }

    // A `HashSet` iterates in an unspecified order; sort the member list so
    // the emitted diagnostic is deterministic across runs.
    let mut members: Vec<String> = paths.iter().cloned().collect();
    members.sort_unstable();
    diagnostics.push(PreprocDiagnostic::IncludeCycle { members });

    (replacement, paths)
}

crate::observation::counter!(include_graph_walks);

/// What one include-graph node contributes to every closure that reaches it.
enum NodeContribution<'a> {
    /// A decodable path, inserted into the reaching file's
    /// `indirect_includes`.
    Path(&'a str),
    /// A path that is not valid UTF-8: it cannot go into the `String`-keyed
    /// set, so every file reaching it reports it instead — the same
    /// once-per-(file, node) multiplicity the per-file walk produced.
    NonUtf8(&'a Path),
}

/// Every node's transitive closure over the cycle-free include graph.
///
/// `closures[node.index()]` holds sorted, de-duplicated indices into
/// `entries` for everything reachable from that node, the node itself
/// included. They are filled in reverse topological order, so each node
/// merges its successors' finished closures instead of re-walking the graph;
/// the per-file [`Dfs`] this replaced ran once per file and re-visited every
/// shared header once per file that reached it (issue #1107).
struct IncludeClosures<'a> {
    entries: Vec<NodeContribution<'a>>,
    closures: Vec<Vec<usize>>,
}

impl IncludeClosures<'_> {
    /// Writes the closure of `start` into `x_inc`, reporting the
    /// undecodable paths it reaches rather than inserting them.
    fn materialize(
        &self,
        start: NodeIndex,
        x_inc: &mut HashSet<String>,
        diagnostics: &mut Vec<PreprocDiagnostic>,
    ) {
        let Some(ids) = self.closures.get(start.index()) else {
            return;
        };
        // The closure size is known up front, which the walk it replaced
        // could not know: sizing once skips the repeated rehash a set
        // growing from empty to ~70 entries would pay.
        x_inc.reserve(ids.len());
        for entry in ids.iter().filter_map(|&id| self.entries.get(id)) {
            match entry {
                NodeContribution::Path(path) => {
                    x_inc.insert((*path).to_string());
                }
                NodeContribution::NonUtf8(path) => {
                    diagnostics.push(PreprocDiagnostic::NonUtf8IndirectInclude {
                        path: path.display().to_string(),
                    });
                }
            }
        }
    }
}

/// Merges two sorted, individually de-duplicated id slices into `out`,
/// replacing whatever it held — the fold below reuses one scratch
/// buffer, and appending to a stale one would emit a union that is
/// neither sorted nor a closure. A linear merge rather than `extend` +
/// `sort` + `dedup` because the sort's log factor would land on the
/// *closure* size — exactly what grows on a deep include chain.
fn merge_sorted_ids(a: &[usize], b: &[usize], out: &mut Vec<usize>) {
    out.clear();
    out.reserve(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    while let (Some(&left), Some(&right)) = (a.get(i), b.get(j)) {
        // The smaller head is emitted once; an equal pair advances both
        // cursors, which is what de-duplicates across the two inputs.
        out.push(left.min(right));
        if left <= right {
            i += 1;
        }
        if right <= left {
            j += 1;
        }
    }
    out.extend_from_slice(&a[i..]);
    out.extend_from_slice(&b[j..]);
}

/// Indexes each node's own contribution as a contiguous range of `entries`.
/// An SCC replacement node (empty path) stands in for every member of the
/// cycle and so contributes one entry per member.
fn index_node_contributions<'a>(
    g: &'a IncludeGraph,
    scc_map: &'a HashMap<NodeIndex, HashSet<String>>,
) -> (Vec<NodeContribution<'a>>, Vec<std::ops::Range<usize>>) {
    let mut entries = Vec::with_capacity(g.node_count());
    let mut own = vec![0..0; g.node_bound()];
    for node in g.node_indices() {
        let start = entries.len();
        match g.node_weight(node) {
            Some(weight) if weight.as_os_str().is_empty() => {
                if let Some(paths) = scc_map.get(&node) {
                    entries.extend(paths.iter().map(|p| NodeContribution::Path(p)));
                }
            }
            Some(weight) => entries.push(
                weight
                    .to_str()
                    .map_or_else(|| NodeContribution::NonUtf8(weight), NodeContribution::Path),
            ),
            None => {}
        }
        own[node.index()] = start..entries.len();
    }
    (entries, own)
}

/// Computes every node's closure in one reverse-topological pass.
///
/// Returns `None` when the graph still holds a cycle — which [`collapse_scc`]
/// has removed by construction, since a node both entering and leaving a
/// component would itself belong to it. Reporting rather than asserting keeps
/// a violated assumption a slowdown (the caller falls back to the per-file
/// walk) rather than a panic or a wrong closure.
fn compute_include_closures<'a>(
    g: &'a IncludeGraph,
    scc_map: &'a HashMap<NodeIndex, HashSet<String>>,
) -> Option<IncludeClosures<'a>> {
    let order = toposort(g, None).ok()?;
    include_graph_walks::record();

    let (entries, own) = index_node_contributions(g, scc_map);
    let mut closures: Vec<Vec<usize>> = vec![Vec::new(); g.node_bound()];
    let mut merged = Vec::new();
    // Reverse topological order: every successor's closure is final by the
    // time the node that reaches it is merged.
    for node in order.into_iter().rev() {
        // A contiguous range is already sorted and unique.
        let mut acc: Vec<usize> = own[node.index()].clone().collect();
        for succ in g.neighbors_directed(node, Direction::Outgoing) {
            merge_sorted_ids(&acc, &closures[succ.index()], &mut merged);
            std::mem::swap(&mut acc, &mut merged);
        }
        closures[node.index()] = acc;
    }
    Some(IncludeClosures { entries, closures })
}

/// Records into every file's `indirect_includes` the transitive closure of
/// includes reachable from its node. An SCC replacement node (empty path)
/// contributes every member path it stands in for. Files reachable only
/// through the graph but never preprocessed are warned about.
fn record_indirect_includes<S: ::std::hash::BuildHasher>(
    files: &mut HashMap<PathBuf, PreprocFile, S>,
    g: &IncludeGraph,
    nodes: &HashMap<PathBuf, NodeIndex>,
    scc_map: &HashMap<NodeIndex, HashSet<String>>,
    diagnostics: &mut Vec<PreprocDiagnostic>,
) {
    let precomputed = compute_include_closures(g, scc_map);
    for (path, start) in nodes {
        let Some(pf) = files.get_mut(path) else {
            diagnostics.push(PreprocDiagnostic::NotPreprocessed { file: path.clone() });
            continue;
        };
        if let Some(closures) = &precomputed {
            closures.materialize(*start, &mut pf.indirect_includes, diagnostics);
        } else {
            // Unreachable, hence untested: `collapse_scc` leaves the
            // graph acyclic. `assert_closures` pins the two agree.
            accumulate_reachable_includes(
                g,
                *start,
                scc_map,
                &mut pf.indirect_includes,
                diagnostics,
            );
        }
    }
}

/// Walk the include graph from `start`, inserting the transitive closure of
/// reachable include paths into `x_inc`. An SCC replacement node (empty path)
/// contributes every member path it stands in for; a non-UTF-8 path is
/// reported and skipped.
///
/// The fallback for a graph [`compute_include_closures`] could not order,
/// which is why it re-derives the same closure one file at a time.
fn accumulate_reachable_includes(
    g: &IncludeGraph,
    start: NodeIndex,
    scc_map: &HashMap<NodeIndex, HashSet<String>>,
    x_inc: &mut HashSet<String>,
    diagnostics: &mut Vec<PreprocDiagnostic>,
) {
    include_graph_walks::record();
    let mut dfs = Dfs::new(g, start);
    while let Some(node) = dfs.next(g) {
        let w = g
            .node_weight(node)
            .expect("invariant: DFS-visited node must have weight in graph");
        if w.as_os_str().is_empty() {
            let paths = scc_map.get(&node).expect(
                "every empty-path node is an SCC replacement and must have a scc_map entry",
            );
            x_inc.extend(paths.iter().cloned());
        } else if let Some(s) = w.to_str() {
            x_inc.insert(s.to_string());
        } else {
            diagnostics.push(PreprocDiagnostic::NonUtf8IndirectInclude {
                path: w.display().to_string(),
            });
        }
    }
}

/// Constructs a dependency graph of the include directives
/// in a `C/C++` file.
///
/// The dependency graph is built using both preprocessor data and not
/// extracted from the considered `C/C++` files.
///
/// Best-effort include resolution emits non-fatal
/// [`PreprocDiagnostic`]s (self-inclusions, include cycles, non-UTF-8
/// paths, files referenced but never preprocessed) as the returned
/// `Vec` rather than writing to `stderr`, so an embedder can capture or
/// suppress them. The CLI prints them to `stderr`; callers that do not
/// care may discard the result.
///
/// # Panics
///
/// Panics if any of the lockstep invariants between the include graph
/// `g`, the `nodes` map, and the `scc_map` is violated at runtime —
/// specifically: an SCC component node missing from the graph, a graph
/// node weight without a `nodes` map entry, a DFS-visited node without a
/// stored weight, or an empty-path replacement node without a `scc_map`
/// entry. These are built in lockstep here, so all four are unrecoverable
/// programmer errors rather than reachable input failures. The last two
/// live on the per-file fallback walk, which now runs only when the
/// precomputed include closures could not order the graph.
pub fn fix_includes<S: ::std::hash::BuildHasher>(
    files: &mut HashMap<PathBuf, PreprocFile, S>,
    all_files: &HashMap<String, Vec<PathBuf>, S>,
) -> Vec<PreprocDiagnostic> {
    let mut diagnostics = Vec::new();
    let (mut g, mut nodes) = build_include_graph(files, all_files, &mut diagnostics);
    let scc_map = collapse_scc(&mut g, &mut nodes, &mut diagnostics);
    record_indirect_includes(files, &g, &nodes, &scc_map, &mut diagnostics);
    // Both producers push while iterating a `HashMap` — `files` in
    // `build_include_graph`, `nodes` in `record_indirect_includes` — so
    // without this the *sequence* varies run to run for identical input
    // even though its content does not. Measured before the sort: 40
    // distinct orders across 40 runs of one 8-file input, one distinct
    // set. The CLI prints this Vec straight to stderr, so that surfaced
    // as `bca preproc` emitting the same warnings in a different order
    // every time — the #1091 class.
    //
    // Note the inner half of this was already fixed: `IncludeCycle`
    // sorts its own member list "so the emitted diagnostic is
    // deterministic across runs". Only the outer sequence was left.
    diagnostics.sort();
    diagnostics
}

/// Strips the surrounding double quotes from an `#include` `string_literal`
/// spanning `code[start..end]` and trims leading/trailing whitespace from the
/// enclosed path.
///
/// Returns `None` for any malformed span that cannot hold both quote bytes.
/// Tree-sitter's error recovery can emit a `string_literal` shorter than the
/// two surrounding quotes (e.g. a truncated `#include "` with no closing
/// quote), so the byte span is validated *before* slicing — `end < start + 2`
/// would otherwise produce a reversed `start + 1..end - 1` range and panic
/// (issue #432). An empty (`""`), whitespace-only, or non-UTF-8 payload also
/// yields `None`.
fn strip_include_quotes(code: &[u8], start: usize, end: usize) -> Option<&str> {
    // A valid quoted literal needs at least the opening and closing quote.
    const MIN_QUOTED_LEN: usize = 2;
    if end < start + MIN_QUOTED_LEN {
        return None;
    }

    let inner = &code[start + 1..end - 1];
    let first = inner.iter().position(|&c| c != b' ' && c != b'\t')?;
    let last = inner.iter().rposition(|&c| c != b' ' && c != b'\t')?;
    std::str::from_utf8(&inner[first..=last]).ok()
}

/// Extracts preprocessor data from a `C/C++` source buffer and inserts
/// it into a [`PreprocResults`] object.
///
/// Builds the preprocessor parse internally, so callers supply the raw
/// `source` and need not name the parser type. `path` keys the
/// per-file results.
pub fn preprocess(source: Vec<u8>, path: &Path, results: &mut PreprocResults) {
    preprocess_with_parser(&PreprocParser::new(source, path, None), path, results);
}

/// Walk an already-built [`PreprocParser`] tree, accumulating its
/// preprocessor data into `results`. Internal core shared by the public
/// [`preprocess`] seam and the crate's own preprocessor tests.
pub(crate) fn preprocess_with_parser(
    parser: &PreprocParser,
    path: &Path,
    results: &mut PreprocResults,
) {
    let node = parser.root();
    let mut cursor = node.cursor();
    let code = parser.code();
    let mut file_result = PreprocFile::default();

    // The stack-based walk visits siblings in reverse source order, so a
    // `#define FOO` / `#undef FOO` pair would be observed undef-first.
    // Collect each directive with its byte offset and replay in source
    // order afterwards, so `#undef` removes a macro a *preceding*
    // `#define` introduced — and a `#define` that follows a `#undef`
    // re-introduces it (issue #705).
    let mut macro_events: Vec<(usize, MacroEvent)> = Vec::new();

    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        push_children(&mut cursor, &node, &mut stack);
        classify_preproc_node(
            &mut cursor,
            &node,
            code,
            &mut file_result,
            &mut macro_events,
        );
    }

    apply_macro_events(macro_events, &mut file_result);

    results.files.insert(path.to_path_buf(), file_result);
}

/// Push `node`'s children onto `stack` for the stack-based DFS in
/// [`preprocess_with_parser`]. Children are pushed in source order so they
/// pop in reverse; directive order is recovered from byte offsets in
/// [`apply_macro_events`], so visit order does not affect the result.
fn push_children<'a>(cursor: &mut Cursor<'a>, node: &Node<'a>, stack: &mut Vec<Node<'a>>) {
    // No reversal, unlike the metric walk's namesake: directives are
    // collected with their byte offsets and replayed in source order
    // afterwards (see `macro_events`), so visit order does not matter
    // here and imposing one would imply a guarantee nothing relies on.
    stack.extend(node.children_with(cursor));
}

/// Classify one node from the [`preprocess_with_parser`] walk: a
/// `#define`/`#undef` is captured as a [`MacroEvent`] tagged with its byte
/// offset (replayed in source order later), and a quoted `#include` is
/// recorded directly into `file_result`. All other nodes are ignored.
///
/// Takes the walk's shared `cursor` by `&mut` and `reset`s it to reach the
/// directive's first child, rather than allocating a fresh cursor per node —
/// the caller is done with `cursor` by the time this runs.
fn classify_preproc_node<'a>(
    cursor: &mut Cursor<'a>,
    node: &Node<'a>,
    code: &'a [u8],
    file_result: &mut PreprocFile,
    macro_events: &mut Vec<(usize, MacroEvent)>,
) {
    let id = Preproc::from(node.kind_id());
    match id {
        Preproc::Define | Preproc::Undef => {
            cursor.reset(node);
            cursor.goto_first_child();
            let identifier = cursor.node();
            if identifier.kind_id() == Preproc::Identifier
                && let Some(macro_text) = identifier.utf8_text(code)
                && !is_specials(macro_text)
            {
                // `#undef` un-defines: a macro is in the final set only if
                // its last directive was a `#define`.
                let event = if id == Preproc::Undef {
                    MacroEvent::Undef(macro_text.to_string())
                } else {
                    MacroEvent::Define(macro_text.to_string())
                };
                macro_events.push((identifier.start_byte(), event));
            }
        }
        Preproc::PreprocInclude => {
            cursor.reset(node);
            cursor.goto_first_child();
            let file = cursor.node();
            if file.kind_id() == Preproc::StringLiteral
                && let Some(include) =
                    strip_include_quotes(code, file.start_byte(), file.end_byte())
            {
                file_result.direct_includes.insert(include.to_string());
            }
        }
        _ => {}
    }
}

/// Replay collected `#define`/`#undef` directives in source order so the
/// final macro set reflects the last directive seen for each name (issue
/// #705). A stable sort on the byte offset preserves the (already unique)
/// directive order; ties cannot occur because each identifier starts at a
/// distinct byte.
fn apply_macro_events(mut macro_events: Vec<(usize, MacroEvent)>, file_result: &mut PreprocFile) {
    macro_events.sort_by_key(|(offset, _)| *offset);
    for (_, event) in macro_events {
        match event {
            MacroEvent::Define(name) => {
                file_result.macros.insert(name);
            }
            MacroEvent::Undef(name) => {
                file_result.macros.remove(&name);
            }
        }
    }
}

/// A single `#define`/`#undef` directive captured during the AST walk,
/// replayed in source order so `#undef` removes a previously defined
/// macro (issue #705).
enum MacroEvent {
    /// `#define NAME` — adds NAME to the file's macro set.
    Define(String),
    /// `#undef NAME` — removes NAME from the file's macro set.
    Undef(String),
}

#[cfg(test)]
#[path = "preproc_tests.rs"]
mod tests;
