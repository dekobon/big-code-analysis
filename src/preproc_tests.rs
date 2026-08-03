//! Unit tests for [`crate::preproc`].
//!
//! Split out under the `#[path]` convention `src/tools.rs` already uses:
//! the module is large enough that co-locating it pushed `preproc.rs`
//! past the *soft* tier of the repository's own `loc.sloc` gate. Only
//! the attributes and blank lines around a pruned `#[cfg(test)]` module
//! keep costing the parent file, so the split recovered 9 sloc — 769
//! down to 760, measured — not the module's own length.

#![allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]

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

/// Builds the include graph for `spec` — each entry pairs a file with
/// the `#include` spellings it contains — and returns every file's
/// transitive closure computed twice: by the reverse-topological pass
/// and by the per-file graph walk it replaced.
///
/// The walk is the oracle: it is what shipped before #1107 and what
/// every consumer's macro resolution is still derived from.
fn closures_both_ways(spec: &[(&str, &[&str])]) -> Vec<(String, HashSet<String>, HashSet<String>)> {
    let mut files: HashMap<PathBuf, PreprocFile> = HashMap::new();
    let mut all_files: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for (name, includes) in spec {
        let mut pf = PreprocFile::default();
        pf.direct_includes
            .extend(includes.iter().map(|i| (*i).to_string()));
        files.insert(PathBuf::from(name), pf);
        all_files
            .entry((*name).to_string())
            .or_default()
            .push(PathBuf::from(name));
    }

    let mut diagnostics = Vec::new();
    let (mut g, mut nodes) = build_include_graph(&files, &all_files, &mut diagnostics);
    let scc_map = collapse_scc(&mut g, &mut nodes, &mut diagnostics);
    let closures =
        compute_include_closures(&g, &scc_map).expect("collapse_scc leaves an acyclic graph");

    let mut both: Vec<(String, HashSet<String>, HashSet<String>)> = nodes
        .iter()
        .map(|(path, start)| {
            let mut merged = HashSet::new();
            closures.materialize(*start, &mut merged, &mut Vec::new());
            let mut walked = HashSet::new();
            accumulate_reachable_includes(&g, *start, &scc_map, &mut walked, &mut Vec::new());
            (path.display().to_string(), merged, walked)
        })
        .collect();
    both.sort_by(|a, b| a.0.cmp(&b.0));
    both
}

/// Asserts, for one include shape, that both closure implementations
/// agree with each other *and* with the hand-derived `expected`, which
/// is keyed by file name in sorted order.
fn assert_closures(shape: &str, spec: &[(&str, &[&str])], expected: &[(&str, &[&str])]) {
    let both = closures_both_ways(spec);
    for (path, merged, walked) in &both {
        assert_eq!(
            merged, walked,
            "{shape}: closure of {path} diverges from the walk it replaced"
        );
    }
    let want: Vec<(String, HashSet<String>)> = expected
        .iter()
        .map(|(name, reachable)| {
            (
                (*name).to_string(),
                reachable.iter().map(|r| (*r).to_string()).collect(),
            )
        })
        .collect();
    let have: Vec<(String, HashSet<String>)> = both
        .into_iter()
        .map(|(path, merged, _)| (path, merged))
        .collect();
    assert_eq!(have, want, "{shape}: unexpected closure");
}

/// [`merge_sorted_ids`] keeps a closure sorted and duplicate-free as it
/// absorbs each successor. A surviving duplicate is invisible in the
/// `HashSet` the closure materializes into but doubles the closure's
/// memory in every node above it, so it is pinned directly here.
#[test]
fn merge_sorted_ids_yields_one_sorted_copy_of_each_id() {
    let mut out = Vec::new();
    merge_sorted_ids(&[1, 3, 5], &[3, 4, 5, 9], &mut out);
    assert_eq!(out, vec![1, 3, 4, 5, 9]);

    // Either side empty degenerates to the other, order preserved.
    merge_sorted_ids(&[], &[2, 7], &mut out);
    assert_eq!(out, vec![2, 7]);
    merge_sorted_ids(&[2, 7], &[], &mut out);
    assert_eq!(out, vec![2, 7]);
    // Both empty: the leaf case every closure bottoms out in.
    merge_sorted_ids(&[], &[], &mut out);
    assert!(out.is_empty());
}

/// The scratch buffer the fold reuses is cleared by `merge_sorted_ids`
/// itself, not by its caller: a merge that appended to a stale buffer
/// would hand back a union that is neither sorted nor a closure of its
/// two inputs, and the `HashSet` it materializes into would hide it.
#[test]
fn merge_sorted_ids_replaces_the_scratch_buffer() {
    let mut out = vec![100, 200, 300];
    merge_sorted_ids(&[1, 4], &[2, 4], &mut out);
    assert_eq!(out, vec![1, 2, 4]);
}

/// A straight chain: every file reaches itself and everything below it.
#[test]
fn closure_matches_the_walk_on_a_chain() {
    assert_closures(
        "chain",
        &[
            ("a.h", &["b.h"]),
            ("b.h", &["c.h"]),
            ("c.h", &["d.h"]),
            ("d.h", &[]),
        ],
        &[
            ("a.h", &["a.h", "b.h", "c.h", "d.h"]),
            ("b.h", &["b.h", "c.h", "d.h"]),
            ("c.h", &["c.h", "d.h"]),
            ("d.h", &["d.h"]),
        ],
    );
}

/// A diamond: `d.h` is reached by two disjoint routes and must appear
/// once — the case a merge that failed to de-duplicate would inflate.
#[test]
fn closure_matches_the_walk_on_a_diamond() {
    assert_closures(
        "diamond",
        &[
            ("a.h", &["b.h", "c.h"]),
            ("b.h", &["d.h"]),
            ("c.h", &["d.h"]),
            ("d.h", &[]),
        ],
        &[
            ("a.h", &["a.h", "b.h", "c.h", "d.h"]),
            ("b.h", &["b.h", "d.h"]),
            ("c.h", &["c.h", "d.h"]),
            ("d.h", &["d.h"]),
        ],
    );
}

/// A self-inclusion adds no edge, so the closure is the file alone —
/// and, crucially, the graph stays acyclic enough to order.
#[test]
fn closure_matches_the_walk_on_a_self_cycle() {
    assert_closures(
        "self-cycle",
        &[("a.h", &["a.h"]), ("b.h", &[])],
        &[("a.h", &["a.h"]), ("b.h", &["b.h"])],
    );
}

/// A two-file cycle collapses to one replacement node, so both members
/// see the whole component.
#[test]
fn closure_matches_the_walk_on_a_mutual_cycle() {
    assert_closures(
        "mutual cycle",
        &[("a.h", &["b.h"]), ("b.h", &["a.h"])],
        &[("a.h", &["a.h", "b.h"]), ("b.h", &["a.h", "b.h"])],
    );
}

/// A three-member cycle with an external predecessor and an external
/// successor: the shape where the replacement node has to carry both
/// re-wired boundaries and stand in for three paths at once.
#[test]
fn closure_matches_the_walk_on_a_three_member_scc() {
    assert_closures(
        "SCC of 3",
        &[
            ("a.h", &["b.h"]),
            ("b.h", &["c.h"]),
            ("c.h", &["a.h", "y.h"]),
            ("x.h", &["a.h"]),
            ("y.h", &[]),
        ],
        &[
            ("a.h", &["a.h", "b.h", "c.h", "y.h"]),
            ("b.h", &["a.h", "b.h", "c.h", "y.h"]),
            ("c.h", &["a.h", "b.h", "c.h", "y.h"]),
            ("x.h", &["a.h", "b.h", "c.h", "x.h", "y.h"]),
            ("y.h", &["y.h"]),
        ],
    );
}

/// Two components that never meet. A reverse-topological pass orders
/// the whole graph at once, so an implementation that leaked state
/// between components would show up here.
#[test]
fn closure_matches_the_walk_on_disconnected_components() {
    assert_closures(
        "disconnected",
        &[
            ("a.h", &["b.h"]),
            ("b.h", &[]),
            ("c.h", &["d.h"]),
            ("d.h", &[]),
        ],
        &[
            ("a.h", &["a.h", "b.h"]),
            ("b.h", &["b.h"]),
            ("c.h", &["c.h", "d.h"]),
            ("d.h", &["d.h"]),
        ],
    );
}

/// An `#include` of a header that is nowhere in the tree resolves to
/// nothing and adds no edge.
#[test]
fn closure_matches_the_walk_on_a_missing_header() {
    assert_closures(
        "missing header",
        &[("a.h", &["nowhere.h"]), ("b.h", &[])],
        &[("a.h", &["a.h"]), ("b.h", &["b.h"])],
    );
}

/// A 200-file chain — the shape where the closure is largest relative
/// to the graph, and the one [`merge_sorted_ids`] is a linear merge
/// rather than a sort for. File `i` reaches exactly the 200 - i files
/// at or below it.
#[test]
fn closure_matches_the_walk_on_a_deep_chain() {
    const DEPTH: usize = 200;
    // Zero-padded so lexicographic order is chain order.
    let names: Vec<String> = (0..DEPTH).map(|i| format!("h{i:03}.h")).collect();
    let includes: Vec<Vec<&str>> = (0..DEPTH)
        .map(|i| {
            names
                .get(i + 1)
                .map(|n| vec![n.as_str()])
                .unwrap_or_default()
        })
        .collect();
    let spec: Vec<(&str, &[&str])> = names
        .iter()
        .zip(&includes)
        .map(|(name, inc)| (name.as_str(), inc.as_slice()))
        .collect();

    for (depth, (path, merged, walked)) in closures_both_ways(&spec).iter().enumerate() {
        assert_eq!(
            merged, walked,
            "deep chain: {path} diverges at depth {depth}"
        );
        assert_eq!(
            merged.len(),
            DEPTH - depth,
            "deep chain: {path} must reach every file below it"
        );
    }
}

/// Two `#include` spellings resolving to the same file leave parallel
/// edges; the closure must still hold one entry. Built at the graph
/// level because `direct_includes` is itself a set, so a literally
/// repeated `#include` never reaches the graph.
#[test]
fn duplicate_edges_contribute_one_closure_entry() {
    let mut g: IncludeGraph = StableGraph::new();
    let a = g.add_node(PathBuf::from("a.h"));
    let b = g.add_node(PathBuf::from("b.h"));
    g.add_edge(a, b, 0);
    g.add_edge(a, b, 0);
    let scc_map = HashMap::new();

    let closures = compute_include_closures(&g, &scc_map).expect("no cycle");
    let mut merged = HashSet::new();
    closures.materialize(a, &mut merged, &mut Vec::new());
    let mut walked = HashSet::new();
    accumulate_reachable_includes(&g, a, &scc_map, &mut walked, &mut Vec::new());

    assert_eq!(
        merged,
        HashSet::from(["a.h".to_string(), "b.h".to_string()])
    );
    assert_eq!(merged, walked);
}

/// A path that is not valid UTF-8 contributes no closure entry and
/// exactly one diagnostic per file that reaches it — the multiplicity
/// the per-file walk produced, since the closure is now shared but the
/// report is not. The closure must still reach *past* it.
#[cfg(unix)]
#[test]
fn non_utf8_nodes_report_once_and_do_not_break_the_closure() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let mut g: IncludeGraph = StableGraph::new();
    let a = g.add_node(PathBuf::from("a.h"));
    let undecodable = g.add_node(PathBuf::from(OsStr::from_bytes(b"b\xff.h")));
    let c = g.add_node(PathBuf::from("c.h"));
    g.add_edge(a, undecodable, 0);
    g.add_edge(undecodable, c, 0);
    let scc_map = HashMap::new();

    let closures = compute_include_closures(&g, &scc_map).expect("no cycle");
    let (mut merged, mut merged_diagnostics) = (HashSet::new(), Vec::new());
    closures.materialize(a, &mut merged, &mut merged_diagnostics);
    let (mut walked, mut walked_diagnostics) = (HashSet::new(), Vec::new());
    accumulate_reachable_includes(&g, a, &scc_map, &mut walked, &mut walked_diagnostics);

    assert_eq!(
        merged,
        HashSet::from(["a.h".to_string(), "c.h".to_string()])
    );
    assert_eq!(merged, walked);
    assert_eq!(merged_diagnostics, walked_diagnostics);
    assert_eq!(merged_diagnostics.len(), 1);
}

/// The guard for #1107's rewrite rather than for its output.
///
/// Every closure assertion above holds just as well against the
/// per-file walk — that is what makes them an oracle, and why none of
/// them can tell the two implementations apart. Only a count of graph
/// walks separates "computed once for the whole graph" from "computed
/// once per file", which is the entire content of the change. Runs on
/// its own thread so the count starts from zero.
#[test]
fn the_closure_is_computed_once_for_the_whole_graph() {
    std::thread::spawn(|| {
        assert_eq!(
            include_graph_walks::observed(),
            0,
            "a fresh thread must not have walked yet"
        );

        let mut files: HashMap<PathBuf, PreprocFile> = HashMap::new();
        let mut all_files: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for (name, includes) in [
            ("a.h", &["b.h"][..]),
            ("b.h", &["c.h"]),
            ("c.h", &["d.h"]),
            ("d.h", &[]),
        ] {
            let mut pf = PreprocFile::default();
            pf.direct_includes
                .extend(includes.iter().map(|i| (*i).to_string()));
            files.insert(PathBuf::from(name), pf);
            all_files.insert(name.to_string(), vec![PathBuf::from(name)]);
        }

        assert!(fix_includes(&mut files, &all_files).is_empty());
        // The closure is right, and it took one pass to get there.
        assert_eq!(
            files
                .get(&PathBuf::from("a.h"))
                .expect("a.h is retained")
                .indirect_includes
                .len(),
            4
        );
        assert_eq!(
            include_graph_walks::observed(),
            1,
            "four files must share one reverse-topological pass, \
             not walk the graph once each"
        );
    })
    .join()
    .expect("closure-count thread must not panic");
}

/// `visible_macros` borrows exactly the names `get_macros` owns,
/// including those reachable only through an indirect include, and
/// only the published owning form is counted as an owned copy.
#[test]
fn visible_macros_borrows_exactly_what_get_macros_owns() {
    std::thread::spawn(|| {
        let root = PathBuf::from("a.h");
        let mut a = PreprocFile::new_macros(&["FROM_A"]);
        a.direct_includes.insert("b.h".to_string());
        let mut b = PreprocFile::new_macros(&["FROM_B"]);
        b.direct_includes.insert("c.h".to_string());

        let mut files: HashMap<PathBuf, PreprocFile> = HashMap::new();
        files.insert(root.clone(), a);
        files.insert(PathBuf::from("b.h"), b);
        files.insert(PathBuf::from("c.h"), PreprocFile::new_macros(&["FROM_C"]));

        let mut all_files: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for name in ["a.h", "b.h", "c.h"] {
            all_files.insert(name.to_string(), vec![PathBuf::from(name)]);
        }
        assert!(fix_includes(&mut files, &all_files).is_empty());

        let borrowed = visible_macros(&root, &files);
        // `FROM_C` is two hops away, so a closure that stopped at the
        // direct include would drop it.
        assert_eq!(
            borrowed,
            HashSet::from(["FROM_A", "FROM_B", "FROM_C"]),
            "every transitively visible macro must be borrowed"
        );

        let owned_before = owned_macro_sets::observed();
        let owned = get_macros(&root, &files);
        assert_eq!(
            owned,
            borrowed.iter().map(|m| (*m).to_string()).collect(),
            "the borrowing form must not change what a caller sees"
        );
        assert_eq!(
            owned_macro_sets::observed(),
            owned_before + 1,
            "only the published owning form allocates a copy"
        );
    })
    .join()
    .expect("macro-view thread must not panic");
}

/// Parsing a C/C++ file must resolve its macros without owning a copy
/// of the set — the per-parse clone #1107 removed. Nothing in a parse's
/// *output* distinguishes the two, so the count is the test.
#[cfg(feature = "cpp")]
#[test]
fn parsing_a_cpp_file_never_owns_the_macro_set() {
    std::thread::spawn(|| {
        let path = PathBuf::from("foo.cpp");
        let mut unit = PreprocFile::default();
        unit.indirect_includes.insert("dep.h".to_string());
        let files = HashMap::from([
            (path.clone(), unit),
            (
                PathBuf::from("dep.h"),
                PreprocFile::new_macros(&["DBG", "FOO"]),
            ),
        ]);
        let pr = std::sync::Arc::new(PreprocResults { files });

        assert_eq!(
            owned_macro_sets::observed(),
            0,
            "a fresh thread must not have owned a set yet"
        );

        let space = crate::Ast::parse(
            crate::Source::new(
                crate::LANG::Cpp,
                b"int f(int x) { return DBG ? FOO : x; }".as_slice(),
            )
            .with_preproc_path(Some(&path))
            .with_preproc(Some(pr)),
        )
        .expect("cpp feature enabled")
        .metrics(crate::MetricsOptions::default())
        .expect("walker succeeds");
        // Both macros are three bytes, so the masking pass rewrites
        // each to the same `$$$` run and the two operands collapse
        // into one: `f`, `x`, `$$$`. Unmasked the count is four
        // (`f`, `x`, `DBG`, `FOO`), so deleting the C-family arm of
        // `get_fake_code` fails here.
        //
        // The name lengths are load-bearing. `$` is an identifier byte
        // in tree-sitter-cpp, so masking a pair of *differently* named
        // macros moves no metric at all — measured: the `DBG` /
        // `FROM_DEP` pair this fixture used to carry left cyclomatic,
        // both Halstead vocabularies, and lloc identical either way,
        // and the assertion here was decorative.
        assert_eq!(space.metrics.halstead.unique_operands(), 3);

        assert_eq!(
            owned_macro_sets::observed(),
            0,
            "the parse must borrow the macro names out of the \
             preprocessor results, not clone them"
        );
    })
    .join()
    .expect("parse thread must not panic");
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

/// The `Display` impl is the only rendering of these diagnostics a user
/// ever sees — the CLI writes it to stderr and `bca-web` may surface it
/// verbatim — so each variant's text is pinned exactly rather than
/// probed with `contains`. Asserting the whole string is what catches a
/// dropped path interpolation or a lost prefix; a substring check passes
/// against both.
///
/// Note the deliberate `Warning:` / `warning:` split recorded here: three
/// variants capitalise and two do not. That is the shipped text as of
/// this commit, pinned so a normalisation is a visible, reviewed diff
/// rather than an accident.
#[test]
fn preproc_diagnostic_display_renders_each_variant() {
    assert_eq!(
        PreprocDiagnostic::SelfInclusion {
            file: PathBuf::from("inc/self ref.h"),
        }
        .to_string(),
        "Warning: possible self inclusion inc/self ref.h",
    );

    assert_eq!(
        PreprocDiagnostic::NonUtf8CyclePath {
            path: "bad/\u{fffd}.h".to_owned(),
        }
        .to_string(),
        "warning: skipping non-UTF-8 path in include cycle: bad/\u{fffd}.h",
    );

    assert_eq!(
        PreprocDiagnostic::NonUtf8IndirectInclude {
            path: "bad/\u{fffd}.h".to_owned(),
        }
        .to_string(),
        "warning: skipping non-UTF-8 indirect include path: bad/\u{fffd}.h",
    );

    assert_eq!(
        PreprocDiagnostic::NotPreprocessed {
            file: PathBuf::from("vendor/unseen.h"),
        }
        .to_string(),
        "Warning: included file which has not been preprocessed: vendor/unseen.h",
    );
}

/// `IncludeCycle` is the only multi-line variant: a header line plus one
/// quoted line per member, each newline-terminated. The member list here
/// is deliberately unsorted and contains a path with a space — the
/// quoting exists precisely so that whitespace stays visible, and a
/// member set of one could not distinguish "prints every member" from
/// "prints the first".
#[test]
fn preproc_diagnostic_display_lists_every_cycle_member() {
    let rendered = PreprocDiagnostic::IncludeCycle {
        members: vec!["z.h".to_owned(), "a b.h".to_owned(), "m.h".to_owned()],
    }
    .to_string();

    assert_eq!(
        rendered,
        "Warning: possible include cycle:\n  - \"z.h\"\n  - \"a b.h\"\n  - \"m.h\"\n",
    );

    let empty = PreprocDiagnostic::IncludeCycle {
        members: Vec::new(),
    }
    .to_string();
    assert_eq!(
        empty, "Warning: possible include cycle:\n",
        "an empty member list still renders the header and nothing else"
    );
}
