// bca: suppress-file(halstead, nargs)
// C/C++ preprocessor extraction; file-level halstead and summed nargs are
// many-fn aggregation artifacts. cognitive is deliberately NOT suppressed —
// the include-graph walk is genuine nested logic and stays baselined.

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
    Direction, algo::kosaraju_scc, graph::NodeIndex, stable_graph::StableGraph, visit::Dfs,
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
/// them as it sees fit. The CLI prints them to `stderr`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
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
                write!(f, "Warning: possible self inclusion {}", file.display())
            }
            Self::IncludeCycle { members } => {
                writeln!(f, "Warning: possible include cycle:")?;
                for member in members {
                    // Explicit quotes preserve whitespace visibility for
                    // paths that contain spaces — important when the cycle
                    // warning is the only signal a user gets.
                    writeln!(f, "  - \"{member}\"")?;
                }
                Ok(())
            }
            Self::NonUtf8CyclePath { path } => {
                write!(
                    f,
                    "warning: skipping non-UTF-8 path in include cycle: {path}"
                )
            }
            Self::NonUtf8IndirectInclude { path } => write!(
                f,
                "warning: skipping non-UTF-8 indirect include path: {path}"
            ),
            Self::NotPreprocessed { file } => write!(
                f,
                "Warning: included file which has not been preprocessed: {}",
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
    /// Adds new macros to the set of macro of a file.
    #[must_use]
    pub fn new_macros(macros: &[&str]) -> Self {
        let mut pf = Self::default();
        for m in macros {
            pf.macros.insert((*m).to_string());
        }
        pf
    }
}

/// Returns the macros contained in a `C/C++` file.
pub fn get_macros<S: ::std::hash::BuildHasher>(
    file: &Path,
    files: &HashMap<PathBuf, PreprocFile, S>,
) -> HashSet<String> {
    let mut macros = HashSet::new();
    if let Some(pf) = files.get(file) {
        for m in &pf.macros {
            macros.insert(m.clone());
        }
        for f in &pf.indirect_includes {
            if let Some(pf) = files.get(&PathBuf::from(f)) {
                for m in &pf.macros {
                    macros.insert(m.clone());
                }
            }
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
        if component.len() > 1 {
            // External boundaries must be captured before the replacement node
            // is added, so the new node is never mistaken for an external
            // neighbor.
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
                    .expect("invariant: every graph node must have a nodes map entry") =
                    replacement;
            }

            // A `HashSet` iterates in an unspecified order; sort the member
            // list so the emitted diagnostic is deterministic across runs.
            let mut members: Vec<String> = paths.iter().cloned().collect();
            members.sort_unstable();
            diagnostics.push(PreprocDiagnostic::IncludeCycle { members });

            scc_map.insert(replacement, paths);
        }
    }
    scc_map
}

/// Walks the include graph from every file's node and records the transitive
/// closure of reachable includes into that file's `indirect_includes`. An
/// SCC replacement node (empty path) contributes every member path it stands
/// in for. Files reachable only through the graph but never preprocessed are
/// warned about.
fn record_indirect_includes<S: ::std::hash::BuildHasher>(
    files: &mut HashMap<PathBuf, PreprocFile, S>,
    g: &IncludeGraph,
    nodes: &HashMap<PathBuf, NodeIndex>,
    scc_map: &HashMap<NodeIndex, HashSet<String>>,
    diagnostics: &mut Vec<PreprocDiagnostic>,
) {
    for (path, start) in nodes {
        let Some(pf) = files.get_mut(path) else {
            diagnostics.push(PreprocDiagnostic::NotPreprocessed { file: path.clone() });
            continue;
        };
        accumulate_reachable_includes(g, *start, scc_map, &mut pf.indirect_includes, diagnostics);
    }
}

/// Walk the include graph from `start`, inserting the transitive closure of
/// reachable include paths into `x_inc`. An SCC replacement node (empty path)
/// contributes every member path it stands in for; a non-UTF-8 path is
/// reported and skipped. Factored out of [`record_indirect_includes`] so the
/// per-file accumulation reads as one step rather than three nested loops.
fn accumulate_reachable_includes(
    g: &IncludeGraph,
    start: NodeIndex,
    scc_map: &HashMap<NodeIndex, HashSet<String>>,
    x_inc: &mut HashSet<String>,
    diagnostics: &mut Vec<PreprocDiagnostic>,
) {
    let mut dfs = Dfs::new(g, start);
    while let Some(node) = dfs.next(g) {
        let w = g
            .node_weight(node)
            .expect("invariant: DFS-visited node must have weight in graph");
        if w == &PathBuf::from("") {
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
/// node weight without a `nodes` map entry, a DFS-visited node without
/// a stored weight, or an empty-path replacement node without a
/// `scc_map` entry. These data structures are built in lockstep by
/// this function, so all four conditions represent unrecoverable
/// programmer errors rather than reachable input failures.
pub fn fix_includes<S: ::std::hash::BuildHasher>(
    files: &mut HashMap<PathBuf, PreprocFile, S>,
    all_files: &HashMap<String, Vec<PathBuf>, S>,
) -> Vec<PreprocDiagnostic> {
    let mut diagnostics = Vec::new();
    let (mut g, mut nodes) = build_include_graph(files, all_files, &mut diagnostics);
    let scc_map = collapse_scc(&mut g, &mut nodes, &mut diagnostics);
    record_indirect_includes(files, &g, &nodes, &scc_map, &mut diagnostics);
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
        classify_preproc_node(&node, code, &mut file_result, &mut macro_events);
    }

    apply_macro_events(macro_events, &mut file_result);

    results.files.insert(path.to_path_buf(), file_result);
}

/// Push `node`'s children onto `stack` for the stack-based DFS in
/// [`preprocess_with_parser`]. Children are pushed in source order so they
/// pop in reverse; directive order is recovered from byte offsets in
/// [`apply_macro_events`], so visit order does not affect the result.
fn push_children<'a>(cursor: &mut Cursor<'a>, node: &Node<'a>, stack: &mut Vec<Node<'a>>) {
    cursor.reset(node);
    if cursor.goto_first_child() {
        loop {
            stack.push(cursor.node());
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

/// Classify one node from the [`preprocess_with_parser`] walk: a
/// `#define`/`#undef` is captured as a [`MacroEvent`] tagged with its byte
/// offset (replayed in source order later), and a quoted `#include` is
/// recorded directly into `file_result`. All other nodes are ignored.
fn classify_preproc_node<'a>(
    node: &Node<'a>,
    code: &'a [u8],
    file_result: &mut PreprocFile,
    macro_events: &mut Vec<(usize, MacroEvent)>,
) {
    let id = Preproc::from(node.kind_id());
    match id {
        Preproc::Define | Preproc::Undef => {
            let mut cursor = node.cursor();
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
            let mut cursor = node.cursor();
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
    use super::*;

    fn parse(source: &str) -> PreprocParser {
        PreprocParser::new(source.as_bytes().to_vec(), &PathBuf::from("test.h"), None)
    }

    /// Empty include strings (`#include ""`) must not panic — earlier
    /// implementations called `unwrap()` on `position`/`rposition` of the
    /// trimmed slice, which returns `None` for an all-whitespace or empty
    /// payload.
    #[test]
    fn preprocess_empty_include_does_not_panic() {
        let parser = parse("#include \"\"\n");
        let mut results = PreprocResults::default();
        preprocess_with_parser(&parser, &PathBuf::from("test.h"), &mut results);
        let pf = results
            .files
            .get(&PathBuf::from("test.h"))
            .expect("file entry must be inserted");
        assert!(pf.direct_includes.is_empty());
    }

    /// Whitespace-only include strings (`#include "   "`) must not panic —
    /// `position` returns `None` because no non-whitespace byte exists.
    #[test]
    fn preprocess_whitespace_only_include_does_not_panic() {
        let parser = parse("#include \"   \"\n");
        let mut results = PreprocResults::default();
        preprocess_with_parser(&parser, &PathBuf::from("test.h"), &mut results);
        let pf = results
            .files
            .get(&PathBuf::from("test.h"))
            .expect("file entry must be inserted");
        assert!(pf.direct_includes.is_empty());
    }

    /// A well-formed include is still recorded with surrounding whitespace
    /// stripped.
    #[test]
    fn preprocess_valid_include_is_recorded() {
        let parser = parse("#include \"  foo.h  \"\n");
        let mut results = PreprocResults::default();
        preprocess_with_parser(&parser, &PathBuf::from("test.h"), &mut results);
        let pf = results
            .files
            .get(&PathBuf::from("test.h"))
            .expect("file entry must be inserted");
        assert!(pf.direct_includes.contains("foo.h"));
    }

    /// `#define` of a normal identifier records the macro name.
    #[test]
    fn preprocess_define_records_macro() {
        let parser = parse("#define FOO 1\n");
        let mut results = PreprocResults::default();
        preprocess_with_parser(&parser, &PathBuf::from("test.h"), &mut results);
        let pf = results
            .files
            .get(&PathBuf::from("test.h"))
            .expect("file entry must be inserted");
        assert!(pf.macros.contains("FOO"));
    }

    fn macros_of(source: &str) -> HashSet<String> {
        let parser = parse(source);
        let mut results = PreprocResults::default();
        preprocess_with_parser(&parser, &PathBuf::from("test.h"), &mut results);
        results
            .files
            .get(&PathBuf::from("test.h"))
            .expect("file entry must be inserted")
            .macros
            .clone()
    }

    /// Regression for #705: `#undef FOO` after `#define FOO` must REMOVE
    /// FOO from the macro set — the pre-fix code shared a `Define | Undef`
    /// arm that inserted the identifier for both, leaving `#undef FOO`
    /// recording FOO as *defined*.
    #[test]
    fn preprocess_undef_removes_defined_macro() {
        let macros = macros_of("#define FOO 1\n#undef FOO\n");
        assert!(
            !macros.contains("FOO"),
            "#undef FOO must un-define FOO; got {macros:?}"
        );
    }

    /// `#undef` of a macro that was never defined is a no-op (and must not
    /// leave the name recorded as defined).
    #[test]
    fn preprocess_undef_of_never_defined_is_noop() {
        let macros = macros_of("#undef NEVER_DEFINED\n");
        assert!(!macros.contains("NEVER_DEFINED"));
    }

    /// Regression for #705's source-order replay: a `#define` that follows a
    /// `#undef` in source order re-introduces the macro. The AST walk visits
    /// siblings in *reverse* source order, so the raw encounter order is
    /// `define` then `undef` (which would drop FOO); only the byte-offset
    /// re-sort in `apply_macro_events` recovers the correct `undef` → `define`
    /// order. The fixture is deliberately asymmetric (undef first, define
    /// last) so a missing or reversed sort flips the result — a `define`
    /// … `undef` … `define` sequence ends on a `define` either way and would
    /// not exercise the ordering at all.
    #[test]
    fn preprocess_define_after_undef_reintroduces_in_source_order() {
        let macros = macros_of("#undef FOO\n#define FOO 1\n");
        assert!(
            macros.contains("FOO"),
            "the trailing source-order #define must win; got {macros:?}"
        );
    }

    /// `#undef` removes only the named macro; unrelated defines survive.
    #[test]
    fn preprocess_undef_leaves_other_macros() {
        let macros = macros_of("#define FOO 1\n#define BAR 2\n#undef FOO\n");
        assert!(!macros.contains("FOO"));
        assert!(macros.contains("BAR"));
    }

    /// `classify_preproc_node` drops `#define`s of compiler/type "special"
    /// tokens (the `is_specials` filter — `size_t`, `NULL`, keywords, …) so
    /// they never pollute the recorded macro set, while an ordinary macro on
    /// an adjacent line is still recorded. Pins the `is_specials` guard that
    /// the #736 refactor moved out of the inline walk and into the helper.
    #[test]
    fn preprocess_define_of_special_token_is_skipped() {
        let macros = macros_of("#define size_t unsigned\n#define APP_FLAG 1\n");
        assert!(
            !macros.contains("size_t"),
            "special token `size_t` must be filtered out; got {macros:?}"
        );
        assert!(
            macros.contains("APP_FLAG"),
            "an ordinary adjacent macro must still be recorded; got {macros:?}"
        );
    }

    /// Regression for #705 (ambiguous include fan-out): when an `#include`
    /// basename resolves to several tied candidates, exactly ONE edge is
    /// added — the lexicographically smallest path — so macros do not leak
    /// from unrelated same-named files via `get_macros`, and the result is
    /// independent of `all_files` Vec ordering.
    #[test]
    fn ambiguous_include_resolves_to_single_deterministic_candidate() {
        // `main.c` includes `config.h`, which exists in two sibling
        // directories equidistant from the includer; neither resolution
        // heuristic disambiguates, so the min-distance fallback ties and
        // would otherwise return both candidates.
        let includer = PathBuf::from("proj/src/main.c");
        let cfg_a = PathBuf::from("proj/aaa/config.h");
        let cfg_b = PathBuf::from("proj/zzz/config.h");

        let mut files: HashMap<PathBuf, PreprocFile> = HashMap::new();
        let mut main = PreprocFile::default();
        main.direct_includes.insert("config.h".to_string());
        files.insert(includer.clone(), main);
        files.insert(cfg_a.clone(), PreprocFile::new_macros(&["FROM_A"]));
        files.insert(cfg_b.clone(), PreprocFile::new_macros(&["FROM_B"]));

        // Reversed Vec order proves the tie-break does not depend on it.
        let mut all_files: HashMap<String, Vec<PathBuf>> = HashMap::new();
        all_files.insert("config.h".to_string(), vec![cfg_b.clone(), cfg_a.clone()]);
        all_files.insert("main.c".to_string(), vec![includer.clone()]);

        let diagnostics = fix_includes(&mut files, &all_files);
        assert!(
            diagnostics.is_empty(),
            "no diagnostics expected for a clean ambiguous resolve; got {diagnostics:?}"
        );

        let main = files.get(&includer).expect("main.c retained");
        // Exactly the lexicographically smallest candidate
        // (`proj/aaa/config.h`) is wired in; the sibling does not leak.
        assert!(main.indirect_includes.contains("proj/aaa/config.h"));
        assert!(!main.indirect_includes.contains("proj/zzz/config.h"));

        let macros = get_macros(&includer, &files);
        assert!(macros.contains("FROM_A"));
        assert!(
            !macros.contains("FROM_B"),
            "macros from the unselected candidate must not leak; got {macros:?}"
        );
    }

    /// A `#include` that resolves back to the including file is reported as
    /// a `SelfInclusion` diagnostic (not written to stderr) and adds no
    /// self-edge.
    #[test]
    fn self_inclusion_is_reported_as_diagnostic() {
        let self_path = PathBuf::from("a.h");
        let mut files: HashMap<PathBuf, PreprocFile> = HashMap::new();
        let mut a = PreprocFile::default();
        a.direct_includes.insert("a.h".to_string());
        files.insert(self_path.clone(), a);

        let mut all_files: HashMap<String, Vec<PathBuf>> = HashMap::new();
        all_files.insert("a.h".to_string(), vec![self_path.clone()]);

        let diagnostics = fix_includes(&mut files, &all_files);
        assert_eq!(
            diagnostics,
            vec![PreprocDiagnostic::SelfInclusion {
                file: self_path.clone(),
            }]
        );
    }

    /// `fix_includes` collapses a 2-file include cycle into one SCC replacement
    /// node and propagates every member of that SCC into the `indirect_includes`
    /// of *both* files symmetrically. Also exercises the `let-else` /
    /// `expect`-with-invariant paths added in the panic-safety refactor (#72).
    #[test]
    fn fix_includes_handles_simple_cycle() {
        let mut files: HashMap<PathBuf, PreprocFile> = HashMap::new();
        let mut a = PreprocFile::default();
        a.direct_includes.insert("b.h".to_string());
        let mut b = PreprocFile::default();
        b.direct_includes.insert("a.h".to_string());
        files.insert(PathBuf::from("a.h"), a);
        files.insert(PathBuf::from("b.h"), b);

        let mut all_files: HashMap<String, Vec<PathBuf>> = HashMap::new();
        all_files.insert("a.h".to_string(), vec![PathBuf::from("a.h")]);
        all_files.insert("b.h".to_string(), vec![PathBuf::from("b.h")]);

        let diagnostics = fix_includes(&mut files, &all_files);

        // The cycle is reported as a single, deterministic diagnostic
        // (members sorted) rather than written to stderr.
        assert_eq!(
            diagnostics,
            vec![PreprocDiagnostic::IncludeCycle {
                members: vec!["a.h".to_string(), "b.h".to_string()],
            }]
        );

        // After resolving the cycle each file's indirect_includes should
        // contain both members of the SCC.
        let a = files
            .get(&PathBuf::from("a.h"))
            .expect("a.h must be retained");
        assert!(a.indirect_includes.contains("a.h"));
        assert!(a.indirect_includes.contains("b.h"));

        let b = files
            .get(&PathBuf::from("b.h"))
            .expect("b.h must be retained");
        assert!(b.indirect_includes.contains("a.h"));
        assert!(b.indirect_includes.contains("b.h"));
    }

    /// `ensure_node` must return the same `NodeIndex` for a repeated path
    /// lookup and must not add a second graph node — the include-graph build
    /// relies on this to coalesce a file referenced from multiple includes.
    #[test]
    fn ensure_node_returns_stable_index_on_repeat() {
        let mut g: IncludeGraph = StableGraph::new();
        let mut nodes: HashMap<PathBuf, NodeIndex> = HashMap::new();
        let p = PathBuf::from("a.h");

        let first = ensure_node(&mut g, &mut nodes, &p);
        let second = ensure_node(&mut g, &mut nodes, &p);

        assert_eq!(first, second);
        assert_eq!(g.node_count(), 1);
        assert_eq!(nodes.len(), 1);
    }

    /// `scc_external_neighbors` must (a) exclude intra-component nodes so the
    /// replacement node only re-wires the cycle's external boundary, and (b)
    /// de-duplicate a node reachable from multiple component members. Here the
    /// component `{a, b}` has one external predecessor `x` (pointing into both)
    /// and one external successor `y` (pointed to by both); each must appear
    /// exactly once and neither `a` nor `b` may leak in.
    #[test]
    fn scc_external_neighbors_dedups_and_excludes_intra_component() {
        let mut graph: IncludeGraph = StableGraph::new();
        let member_a = graph.add_node(PathBuf::from("a.h"));
        let member_b = graph.add_node(PathBuf::from("b.h"));
        let pred = graph.add_node(PathBuf::from("x.h"));
        let succ = graph.add_node(PathBuf::from("y.h"));
        // Intra-component cycle member_a <-> member_b.
        graph.add_edge(member_a, member_b, 0);
        graph.add_edge(member_b, member_a, 0);
        // `pred` points into both members (dedup on the incoming side).
        graph.add_edge(pred, member_a, 0);
        graph.add_edge(pred, member_b, 0);
        // Both members point out to `succ` (dedup on the outgoing side).
        graph.add_edge(member_a, succ, 0);
        graph.add_edge(member_b, succ, 0);

        let component = vec![member_a, member_b];
        let incoming = scc_external_neighbors(&graph, &component, Direction::Incoming);
        let outgoing = scc_external_neighbors(&graph, &component, Direction::Outgoing);

        assert_eq!(incoming, vec![pred]);
        assert_eq!(outgoing, vec![succ]);
    }

    /// Regression for #432: a `string_literal` span shorter than the two
    /// surrounding quote bytes must not panic. Tree-sitter error recovery on a
    /// truncated `#include "` (no closing quote) can yield such a node; the
    /// pre-fix code sliced `code[start + 1..end - 1]` unconditionally, which
    /// builds a reversed range and panics for `end < start + 2`.
    ///
    /// Exercised directly against the byte-span helper so the reversed-range
    /// path is genuinely hit regardless of what the current pinned grammar
    /// emits — reverting the `end < start + 2` guard makes the len-0 and len-1
    /// cases panic with `slice index starts at .. but ends at ..`.
    #[test]
    fn strip_include_quotes_rejects_too_short_spans() {
        let code = b"#include \"\"";
        // Length 0 (empty span) and length 1 (just an opening quote) cannot
        // hold both quotes and must be rejected before slicing.
        assert_eq!(strip_include_quotes(code, 9, 9), None);
        assert_eq!(strip_include_quotes(code, 9, 10), None);
    }

    /// The helper still trims and accepts well-formed spans, and rejects
    /// empty/whitespace-only payloads via the existing `position`/`rposition`
    /// guards rather than panicking.
    #[test]
    fn strip_include_quotes_handles_valid_and_empty_payloads() {
        // `"  foo.h  "` -> trimmed to `foo.h`.
        let code = b"#include \"  foo.h  \"";
        assert_eq!(strip_include_quotes(code, 9, code.len()), Some("foo.h"));
        // `""` (length 2) -> empty payload -> None.
        let code = b"#include \"\"";
        assert_eq!(strip_include_quotes(code, 9, 11), None);
        // `"   "` -> whitespace-only -> None.
        let code = b"#include \"   \"";
        assert_eq!(strip_include_quotes(code, 9, 14), None);
    }

    /// End-to-end: a truncated `#include "` with no closing quote must not
    /// panic the preprocessor pass (issue #432). The file entry is still
    /// inserted with no recorded include.
    #[test]
    fn preprocess_truncated_include_does_not_panic() {
        let parser = parse("#include \"\n");
        let mut results = PreprocResults::default();
        preprocess_with_parser(&parser, &PathBuf::from("test.h"), &mut results);
        let pf = results
            .files
            .get(&PathBuf::from("test.h"))
            .expect("file entry must be inserted");
        assert!(pf.direct_includes.is_empty());
    }
}
