// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

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

/// Constructs a dependency graph of the include directives
/// in a `C/C++` file.
///
/// The dependency graph is built using both preprocessor data and not
/// extracted from the considered `C/C++` files.
///
/// # Panics
///
/// Panics if the graph carries a node weight without a corresponding
/// `nodes` map entry — both maps are built in lockstep so this is a
/// load-bearing invariant rather than a recoverable condition.
pub fn fix_includes<S: ::std::hash::BuildHasher>(
    files: &mut HashMap<PathBuf, PreprocFile, S>,
    all_files: &HashMap<String, Vec<PathBuf>, S>,
) {
    let mut nodes: HashMap<PathBuf, NodeIndex> = HashMap::new();
    // Since we'll remove strong connected components we need to have a stable graph
    // in order to use the nodes we've in the nodes HashMap.
    let mut g = StableGraph::new();

    // First we build a graph of include dependencies
    for (file, pf) in files.iter() {
        let node = match nodes.entry(file.clone()) {
            hash_map::Entry::Occupied(l) => *l.get(),
            hash_map::Entry::Vacant(p) => *p.insert(g.add_node(file.clone())),
        };
        let direct_includes = &pf.direct_includes;
        for i in direct_includes {
            let possibilities = guess_file(file, i, all_files);
            for i in possibilities {
                if &i != file {
                    let i = match nodes.entry(i.clone()) {
                        hash_map::Entry::Occupied(l) => *l.get(),
                        hash_map::Entry::Vacant(p) => *p.insert(g.add_node(i)),
                    };
                    g.add_edge(node, i, 0);
                } else {
                    // TODO: add an option to display warning
                    eprintln!("Warning: possible self inclusion {}", file.display());
                }
            }
        }
    }

    // In order to walk in the graph without issues due to cycles
    // we replace strong connected components by a unique node
    // All the paths in a scc finally represents a kind of unique file containing
    // all the files in the scc.
    let mut scc = kosaraju_scc(&g);
    let mut scc_map: HashMap<NodeIndex, HashSet<String>> = HashMap::new();
    for component in &mut scc {
        if component.len() > 1 {
            // For Firefox, there are only few scc and all of them are pretty small
            // So no need to take a hammer here (for 'contains' stuff).
            // TODO: in some case a hammer can be useful: check perf Vec vs HashSet
            let mut incoming = Vec::new();
            let mut outgoing = Vec::new();
            let mut paths = HashSet::new();

            for c in component.iter() {
                for i in g.neighbors_directed(*c, Direction::Incoming) {
                    if !component.contains(&i) && !incoming.contains(&i) {
                        incoming.push(i);
                    }
                }
                for o in g.neighbors_directed(*c, Direction::Outgoing) {
                    if !component.contains(&o) && !outgoing.contains(&o) {
                        outgoing.push(o);
                    }
                }
            }

            let replacement = g.add_node(PathBuf::from(""));
            for i in incoming.drain(..) {
                g.add_edge(i, replacement, 0);
            }
            for o in outgoing.drain(..) {
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
                eprintln!("  - {p:?}");
            }
            eprintln!();

            scc_map.insert(replacement, paths);
        }
    }

    for (path, node) in nodes {
        let mut dfs = Dfs::new(&g, node);
        if let Some(pf) = files.get_mut(&path) {
            let x_inc = &mut pf.indirect_includes;
            while let Some(node) = dfs.next(&g) {
                let w = g
                    .node_weight(node)
                    .expect("invariant: DFS-visited node must have weight in graph");
                if w == &PathBuf::from("") {
                    let paths = scc_map.get(&node);
                    if let Some(paths) = paths {
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

                if file.kind_id() == Preproc::StringLiteral {
                    // remove the starting/ending double quote
                    let file = &code[file.start_byte() + 1..file.end_byte() - 1];
                    let Some(start) = file.iter().position(|&c| c != b' ' && c != b'\t') else {
                        continue;
                    };
                    let Some(end) = file.iter().rposition(|&c| c != b' ' && c != b'\t') else {
                        continue;
                    };
                    let file = &file[start..=end];
                    let Ok(file) = std::str::from_utf8(file) else {
                        continue;
                    };
                    file_result.direct_includes.insert(file.to_string());
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
}
