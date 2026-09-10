//! Guards on the parse layer's observation counters for the walks that
//! live in this crate.
//!
//! The counters (`node_resolved_sibling_lookups`, `child_scan_cursors`)
//! are declared in `big-code-analysis-ast`, which this crate's tests see
//! as an ordinary dependency; the `test-support` feature is what opens
//! their `observed()` accessors here. The walks under guard — the
//! `exclude_tests` prune and the metric / suppression scans — are this
//! crate's, so the assertions are too (#1376).

/// Under a parent narrow enough to read forward, the
/// `exclude_tests` prune finds the run of `#[…]` siblings before an
/// item through the walker's ancestor chain, never by resolving
/// siblings from the node.
///
/// Nothing in the output says so: the backward walk this replaced
/// returns the same answer, only `O(depth)` per step (#1100), and
/// `rust_outer_attr_scans_agree` in `checker.rs` exists precisely
/// to prove the two agree. The counter is the sole observable, so a
/// revert is a silent quadratic without this.
///
/// Every parent in the fixture holds at most five children, which
/// keeps it under `MAX_FORWARD_ATTRIBUTE_SCAN_CHILDREN` — the
/// backward walk is still the deliberate reading above that width,
/// so a wider fixture would assert the opposite of what it looks
/// like it asserts.
///
/// Seeding a real lookup first is what makes the assertion
/// falsifiable: compared against zero it would also pass with
/// `record()` never wired up at all.
#[cfg(feature = "rust")]
#[test]
fn the_exclude_tests_prune_resolves_no_sibling_from_a_node() {
    let source = "#[cfg(test)]\nmod tests {\nfn t() {}\n}\n\
                  #[inline]\nfn kept() {\n#[allow(dead_code)]\nfn nested() {}\nlet x = 1;\n}\n";
    let ast = crate::test_support::parse_named(crate::LANG::Rust, "lib.rs", source);

    let root = ast.root_node();
    let last = root.children().last().expect("the file has items");
    let _ = last.previous_sibling();
    let seeded = crate::node::node_resolved_sibling_lookups::observed();
    assert!(seeded > 0, "the seed call must be counted");

    ast.metrics(crate::MetricsOptions::default().with_exclude_tests(true))
        .expect("the walk must yield a top-level space");

    assert_eq!(
        crate::node::node_resolved_sibling_lookups::observed(),
        seeded,
        "the metric walk resolved a sibling from a node; \
         read it off the ancestor chain instead (#1096 / #1100)"
    );
}

/// Which arm the `exclude_tests` attribute-scan dispatch takes, at
/// the boundary in both directions and on both of its axes.
///
/// `rust_outer_attr_scans_agree` in `checker.rs` proves the two
/// readings answer the same thing, which is exactly why it cannot
/// see which one ran — it passes at any budget, including one that
/// never reads forward. This counter is the only observable that
/// tells them apart, and it lives here, so the boundary is pinned
/// here too.
///
/// The third case is the one #1100 got wrong: dispatching on width
/// alone sent any over-wide body to the `O(depth)` walk however deep
/// it sat, which on a nested `mod` tree is quadratic (a 3_200-deep
/// fixture measured 2.67 s against 0.045 s for the same shape one
/// child narrower).
#[cfg(feature = "rust")]
#[test]
fn the_exclude_tests_prune_reads_forward_up_to_its_depth_scaled_budget() {
    // Three attributed items make a `source_file` exactly six
    // children wide — the depth-1 budget. A fourth, bare item makes
    // seven, one over. Wrapping that in a `mod` puts the same seven
    // between two braces, so its `declaration_list` is nine wide, at
    // depth 3 — where the budget is also exactly nine.
    let at_budget = "#[cfg(test)]\nfn a() {}\n#[inline]\nfn b() {}\n#[cfg(test)]\nfn c() {}\n";
    let past_budget = format!("{at_budget}fn d() {{}}\n");
    let nested = format!("mod m {{\n{past_budget}}}\n");

    for (shape, source, resolves_siblings) in [
        ("six children at depth 1", at_budget.to_string(), false),
        ("seven children at depth 1", past_budget, true),
        ("nine children at depth 3", nested, false),
    ] {
        let before = crate::node::node_resolved_sibling_lookups::observed();
        crate::test_support::parse_named(crate::LANG::Rust, "lib.rs", &source)
            .metrics(crate::MetricsOptions::default().with_exclude_tests(true))
            .expect("the walk must yield a top-level space");
        let resolved = crate::node::node_resolved_sibling_lookups::observed() > before;
        assert_eq!(
            resolved, resolves_siblings,
            "{shape}: the prune took the wrong dispatch arm"
        );
    }
}

/// The metric and suppression walks #1112 moved onto
/// `Node::children_with` must scan a whole tree on one cursor, not one
/// per node. The `preorder` and `act_on_node` halves of this guard sit
/// beside the counter in `big-code-analysis-ast`.
///
/// Seeding a real scan first is what makes it falsifiable: compared
/// against zero these assertions would also pass with `record()` never
/// wired up at all.
// Gated on the union of the languages the body parses — `c` for the
// seed, `python` for the instance-attribute scan, `rust` for the
// suppression walk. Dropping `c` here would leave `CParser::new`
// reaching `Tree::new`'s disabled-language `expect` under
// `--features python,rust` (`.claude/rules/testing.md`).
#[cfg(all(feature = "c", feature = "python", feature = "rust"))]
#[test]
fn the_metric_and_suppression_walks_scan_a_tree_on_one_cursor() {
    use crate::ParserTrait;

    let seed = crate::CParser::new(
        b"int main() { int a; }".to_vec(),
        std::path::Path::new("a.c"),
        None,
    );
    let _ = seed.root().children().count();
    assert!(
        crate::node::child_scan_cursors::observed() > 0,
        "the seed scan must be counted"
    );

    // The Python instance-attribute scan walks every method body of
    // a class. Before #1112 it was 92 % of the metric walk's child
    // scans on the Python corpus slice — one per node under the
    // class. It is not the only scan a `metrics()` call makes, so
    // the bound is a fraction of the node count rather than zero.
    // Measured on this fixture: 18 scans over 81 nodes with the
    // cursor hoisted, 91 without, so the bound separates the two
    // with room on both sides.
    let source = "class C:\n    def a(self):\n        self.x = 1\n        self.y = [1, 2]\n\
                  \n    def b(self):\n        self.z, self.w = 1, 2\n        \
                  if self.x:\n            self.v = self.y\n";
    let ast = crate::test_support::parse_named(crate::LANG::Python, "c.py", source);
    let nodes = ast.root_node().preorder().count();
    let before = crate::node::child_scan_cursors::observed();
    ast.metrics(crate::MetricsOptions::default())
        .expect("the walk must yield a top-level space");
    let scans = crate::node::child_scan_cursors::observed() - before;
    assert!(nodes > 60, "fixture is too small to prove much");
    assert!(
        scans < nodes / 2,
        "the Python metric walk built {scans} cursors over {nodes} nodes; the \
         instance-attribute scan is meant to hold one for the subtree (#1112)"
    );

    // The suppression scan is a full-tree DFS of its own: 0 scans
    // over this fixture's 29 nodes with the cursor hoisted, 29
    // without.
    let parser = crate::langs::RustParser::new(
        b"// bca: suppress(cognitive)\nfn f() { if a { g(1, 2); } }\n".to_vec(),
        std::path::Path::new("lib.rs"),
        None,
    );
    let nodes = parser.root().preorder().count();
    let before = crate::node::child_scan_cursors::observed();
    let markers = crate::suppression::suppression_markers(&parser);
    let scans = crate::node::child_scan_cursors::observed() - before;
    assert_eq!(markers.len(), 1, "fixture carries one marker");
    assert!(nodes > 20, "fixture is too small to prove much");
    assert!(
        scans < nodes / 2,
        "the suppression scan built {scans} cursors over {nodes} nodes (#1112)"
    );
}
