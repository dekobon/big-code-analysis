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
use crate::tools::*;
use crate::traits::*;

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

/// Builds the include dependency graph from the preprocessor data: one node
/// per file, one edge per resolved direct include. Self-inclusions are warned
/// about and skipped rather than added as self-edges.
fn build_include_graph<S: ::std::hash::BuildHasher>(
    files: &HashMap<PathBuf, PreprocFile, S>,
    all_files: &HashMap<String, Vec<PathBuf>, S>,
) -> (IncludeGraph, HashMap<PathBuf, NodeIndex>) {
    let mut nodes: HashMap<PathBuf, NodeIndex> = HashMap::new();
    // Since we'll remove strong connected components we need to have a stable graph
    // in order to use the nodes we've in the nodes HashMap.
    let mut g = StableGraph::new();

    for (file, pf) in files {
        let node = ensure_node(&mut g, &mut nodes, file);
        for i in &pf.direct_includes {
            let possibilities = guess_file(file, i, all_files);
            for included in possibilities {
                if &included != file {
                    let included = ensure_node(&mut g, &mut nodes, &included);
                    g.add_edge(node, included, 0);
                } else {
                    // TODO: add an option to display warning
                    eprintln!("Warning: possible self inclusion {}", file.display());
                }
            }
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
                    eprintln!(
                        "warning: skipping non-UTF-8 path in include cycle: {}",
                        path.display()
                    );
                }
                *nodes
                    .get_mut(&path)
                    .expect("invariant: every graph node must have a nodes map entry") =
                    replacement;
            }

            eprintln!("Warning: possible include cycle:");
            for p in &paths {
                // Explicit quotes preserve whitespace visibility for
                // paths that contain spaces — important when the cycle
                // warning is the only signal a user gets.
                eprintln!("  - \"{p}\"");
            }
            eprintln!();

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
) {
    for (path, start) in nodes {
        let mut dfs = Dfs::new(g, *start);
        if let Some(pf) = files.get_mut(path) {
            let x_inc = &mut pf.indirect_includes;
            while let Some(node) = dfs.next(g) {
                let w = g
                    .node_weight(node)
                    .expect("invariant: DFS-visited node must have weight in graph");
                if w == &PathBuf::from("") {
                    if let Some(paths) = scc_map.get(&node) {
                        for p in paths {
                            x_inc.insert(p.clone());
                        }
                    } else {
                        unreachable!(
                            "every empty-path node is an SCC replacement and must have a scc_map entry"
                        );
                    }
                } else {
                    let Some(s) = w.to_str() else {
                        eprintln!(
                            "warning: skipping non-UTF-8 indirect include path: {}",
                            w.display()
                        );
                        continue;
                    };
                    x_inc.insert(s.to_string());
                }
            }
        } else {
            eprintln!(
                "Warning: included file which has not been preprocessed: {}",
                path.display()
            );
        }
    }
}

/// Constructs a dependency graph of the include directives
/// in a `C/C++` file.
///
/// The dependency graph is built using both preprocessor data and not
/// extracted from the considered `C/C++` files.
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
) {
    let (mut g, mut nodes) = build_include_graph(files, all_files);
    let scc_map = collapse_scc(&mut g, &mut nodes);
    record_indirect_includes(files, &g, &nodes, &scc_map);
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

/// Extracts preprocessor data from a `C/C++` file
/// and inserts these data in a [`PreprocResults`] object.
///
///
/// [`PreprocResults`]: struct.PreprocResults.html
pub fn preprocess(parser: &PreprocParser, path: &Path, results: &mut PreprocResults) {
    let node = parser.get_root();
    let mut cursor = node.cursor();
    let mut stack = Vec::new();
    let code = parser.get_code();
    let mut file_result = PreprocFile::default();

    stack.push(node);

    while let Some(node) = stack.pop() {
        cursor.reset(&node);
        if cursor.goto_first_child() {
            loop {
                stack.push(cursor.node());
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }

        let id = Preproc::from(node.kind_id());
        match id {
            Preproc::Define | Preproc::Undef => {
                cursor.reset(&node);
                cursor.goto_first_child();
                let identifier = cursor.node();

                if identifier.kind_id() == Preproc::Identifier {
                    let Some(macro_text) = identifier.utf8_text(code) else {
                        continue;
                    };
                    if !is_specials(macro_text) {
                        file_result.macros.insert(macro_text.to_string());
                    }
                }
            }
            Preproc::PreprocInclude => {
                cursor.reset(&node);
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

    results.files.insert(path.to_path_buf(), file_result);
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
        preprocess(&parser, &PathBuf::from("test.h"), &mut results);
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
        preprocess(&parser, &PathBuf::from("test.h"), &mut results);
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
        preprocess(&parser, &PathBuf::from("test.h"), &mut results);
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
        preprocess(&parser, &PathBuf::from("test.h"), &mut results);
        let pf = results
            .files
            .get(&PathBuf::from("test.h"))
            .expect("file entry must be inserted");
        assert!(pf.macros.contains("FOO"));
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

        fix_includes(&mut files, &all_files);

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
        preprocess(&parser, &PathBuf::from("test.h"), &mut results);
        let pf = results
            .files
            .get(&PathBuf::from("test.h"))
            .expect("file entry must be inserted");
        assert!(pf.direct_includes.is_empty());
    }
}
