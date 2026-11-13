//! Guards on the parse layer's observation counters for the walks that
//! live in this crate.
//!
//! The counters (`node_resolved_sibling_lookups`, `child_scan_cursors`)
//! are declared in `big-code-analysis-ast`, which this crate's tests see
//! as an ordinary dependency; the `test-support` feature is what opens
//! their `observed()` accessors here. The walks under guard — the
//! `exclude_tests` prune and the metric / suppression scans — are this
//! crate's, so the assertions are too (#1376).

/// Every seam that reaches a Tcl braced word's role lends its walk a
/// `FoldCache`, so a wide erroring `eval` line's argument list is
/// scanned once per walk rather than once per braced word (#1686).
///
/// The readings cannot see it: each word keeps the construct-wide
/// script answer whether the list is scanned once or `width` times,
/// so every metric, the Halstead vocabulary, the dump and the `string`
/// count come out the same either way, and only the scan counter tells
/// a memoised walk from the quadratic one. Rebuilding the chain in
/// `metrics_inner` or `ops_inner` with no `FoldCache` lent takes that
/// seam's row to `width`; the dump and count seams are pinned the same
/// way beside their walks in `big-code-analysis-ast`, and listed here
/// so the four cannot drift
/// apart.
#[cfg(feature = "tcl")]
#[test]
fn every_seam_scans_an_erroring_tcl_argument_list_once() {
    use crate::lang_helpers::argument_slot_scans;

    type Seam<'a> = &'a dyn Fn(&crate::Ast);

    let width = 200;
    let source = big_code_analysis_ast::test_support::tcl_wide_erroring_eval(width);
    let ast = crate::test_support::parse_named(crate::LANG::Tcl, "wide.tcl", &source);
    let cfg = || crate::AstCfg {
        id: String::new(),
        language: String::new(),
        comment: false,
        span: false,
    };
    let seams: [(&str, Seam); 4] = [
        ("Ast::metrics", &|ast| {
            ast.metrics(crate::MetricsOptions::default())
                .expect("the walk must yield a top-level space");
        }),
        ("Ast::ops", &|ast| {
            ast.ops().expect("the walk must yield a top-level space");
        }),
        ("Ast::dump", &|ast| {
            let _ = ast.dump(cfg());
        }),
        ("Ast::count", &|ast| {
            let (strings, _) = ast.count(&["string".to_owned()]);
            assert_eq!(
                strings, 0,
                "an unreadable `eval` list keeps every word a script"
            );
        }),
    ];
    for (name, run) in seams {
        let before = argument_slot_scans::observed();
        run(&ast);
        assert_eq!(
            argument_slot_scans::observed() - before,
            1,
            "{name} must lend its walk one FoldCache and scan the list once"
        );
    }
}

/// The `exclude_tests` prune finds the run of `#[…]` siblings before an
/// item through the walker's ancestor chain, never by resolving
/// siblings from the node.
///
/// Nothing in the output says so: the backward walk this replaced
/// returns the same answer, only `O(depth)` per step (#1100), and
/// `rust_outer_attr_scans_agree` in `checker.rs` exists precisely
/// to prove the two agree. The counter is the sole observable, so a
/// revert is a silent quadratic without this. Parent widths either
/// side of `WIDE_PARENT` are swept in the test below; this one is the
/// mixed shape.
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

/// The `exclude_tests` prune resolves no sibling from a node under a
/// narrow parent or a wide one, at the root or below it: the attribute
/// run and the item it decorates are both read through `Ancestors`
/// (#1447).
///
/// Until #1447 the prune budgeted the parent's width against the
/// node's depth and fell back to `Node::previous_sibling` past it, so a
/// seven-child root resolved siblings on purpose; restoring the budget
/// fails that row and every wide one. `sibling_cache_reads` pins which
/// lookup `Ancestors` took. Neither counter sees a pass over the
/// parent's children (the defect #1447 fixed) or a prune handed no
/// chain: each answers the same and touches neither counter.
/// `rust_outer_attr_scans_agree` checks the lookups' answers, the
/// `exclude_tests_rust` tests in `spaces_tests.rs` check the walk's,
/// `the_exclude_tests_prune_scans_no_children_for_an_attribute` below
/// catches the pass through `child_scan_cursors` (#1784), and the
/// scaling probes price the prune handed no chain.
#[cfg(feature = "rust")]
#[test]
fn the_exclude_tests_prune_resolves_no_sibling_from_a_node_either_side_of_wide_parent() {
    use big_code_analysis_ast::node::WIDE_PARENT;
    use big_code_analysis_ast::test_support::rust_root_of_width;

    // Seeded with a real lookup, as the test above is, so a counter that
    // stopped counting fails here rather than reading `0 == 0`.
    let seed =
        crate::test_support::parse_named(crate::LANG::Rust, "lib.rs", "fn a() {}\nfn b() {}\n");
    let _ = seed
        .root_node()
        .children()
        .last()
        .expect("the file has items")
        .previous_sibling();
    assert!(
        crate::node::node_resolved_sibling_lookups::observed() > 0,
        "the seed call must be counted"
    );

    let nesting = 8;
    let nested_wide = format!(
        "{}{}{}",
        "fn f() {\n".repeat(nesting),
        "#[cfg(test)]\nfn t() {}\n".repeat(WIDE_PARENT),
        "}\n".repeat(nesting),
    );

    // The last column is whether the walk's `SiblingCache` answers: a
    // walk that lent an empty one (`metrics_inner` built through
    // `for_walk_without_siblings`) seeks instead in a release build,
    // answers the same, and resolves no sibling from a node, so of the
    // two counters read here only `sibling_cache_reads` sees it there;
    // a debug build panics first.
    for (shape, source, cached) in [
        ("a seven-child root", rust_root_of_width(7), false),
        (
            "a `WIDE_PARENT`-child root, sought",
            rust_root_of_width(WIDE_PARENT),
            false,
        ),
        (
            "a `WIDE_PARENT + 1`-child root, cached",
            rust_root_of_width(WIDE_PARENT + 1),
            true,
        ),
        ("a cached body eight functions deep", nested_wide, true),
    ] {
        let reads = crate::node::sibling_cache_reads::observed();
        let before = crate::node::node_resolved_sibling_lookups::observed();
        crate::test_support::parse_named(crate::LANG::Rust, "lib.rs", &source)
            .metrics(crate::MetricsOptions::default().with_exclude_tests(true))
            .expect("the walk must yield a top-level space");
        assert_eq!(
            crate::node::node_resolved_sibling_lookups::observed(),
            before,
            "{shape}: the prune resolved a sibling from a node; \
             read it through `Ancestors` instead (#1447)"
        );
        assert_eq!(
            crate::node::sibling_cache_reads::observed() > reads,
            cached,
            "{shape}: the walk's `SiblingCache` was {} consulted",
            if cached { "not" } else { "unexpectedly" },
        );
    }
}

/// What one metric walk over `ast` adds to `counter`, with
/// `exclude_tests` as given: the measurement the two prune guards below
/// compare.
#[cfg(feature = "rust")]
fn walk_delta(counter: fn() -> usize, ast: &crate::Ast, exclude_tests: bool) -> usize {
    let before = counter();
    ast.metrics(crate::MetricsOptions::default().with_exclude_tests(exclude_tests))
        .expect("the walk must yield a top-level space");
    counter() - before
}

/// The `exclude_tests` prune's attribute reading builds no child-scan
/// cursor of its own: turning the prune on leaves `child_scan_cursors`
/// where the unpruned walk leaves it (#1784).
///
/// #1447's defect was a pass from the parent's first child on every ask,
/// and neither counter above sees it: the pass resolves no sibling from
/// a node and touches no cache, and it answers what the seek answers.
/// `Node::children` is what such a pass calls, and it records here;
/// `Ancestors::siblings_before` / `siblings_after` seek through
/// `Node::cursor`, which does not. Restoring an unconditional
/// `parent.children()` pass in `rust_attributed_item` or in
/// `rust_outer_attr_marks_test` fails this test, and no value test. The
/// backward one also fails `rust_outer_attr_scans_agree`, but only
/// because a parent wider than `WIDE_PARENT` stops consulting the
/// sibling cache; under a narrower parent this test is its only guard.
///
/// Neither mutant is the body #1447 replaced (`749fa327^`). That one
/// gated the pass on a budget of `max(6, 3 × depth)` children and took
/// `Node::next_sibling` past it, which `node_resolved_sibling_lookups`
/// already pins. Both rows sit inside the old budget, so a gated pass
/// would fail here as well: the root's six children against a budget of
/// 6, and the innermost body's 22 against 51 (a chain of 17: the root
/// plus a `function_item` and a `block` per level).
///
/// The fixture holds no `mod`, because `rust_inner_attr_marks_test` scans
/// a module body with `children()` on purpose. Nothing is pruned either:
/// `#[inline]` keeps its `fn`, and `#[cfg(test)]` cannot prune a
/// `use_declaration` (#1431). So both walks visit the same nodes, and
/// the deltas must be equal, not just ordered.
#[cfg(feature = "rust")]
#[test]
fn the_exclude_tests_prune_scans_no_children_for_an_attribute() {
    let attributed = |items: usize| {
        format!(
            "{}#[cfg(test)]\nuse x;\n",
            "#[inline]\nfn g() {}\n".repeat(items)
        )
    };
    let nesting = 8;
    let rows = [
        ("the root", attributed(2), 6),
        (
            "a body eight functions deep",
            format!(
                "{}{}{}",
                "fn f() {\n".repeat(nesting),
                attributed(9),
                "}\n".repeat(nesting),
            ),
            22,
        ),
    ];

    // Seeded with a real scan, so a counter that stopped counting fails
    // here rather than reading `0 == 0`.
    let seed = crate::test_support::parse_named(crate::LANG::Rust, "lib.rs", "fn a() {}\n");
    let _ = seed.root_node().children().count();
    assert!(
        crate::node::child_scan_cursors::observed() > 0,
        "the seed scan must be counted"
    );

    let cursors = crate::node::child_scan_cursors::observed;
    for (shape, source, widest) in rows {
        let ast = crate::test_support::parse_named(crate::LANG::Rust, "lib.rs", &source);
        assert_eq!(
            ast.root_node().preorder().map(|n| n.child_count()).max(),
            Some(widest),
            "{shape}: the fixture's widest parent moved; re-derive the budget above"
        );
        assert_eq!(
            walk_delta(cursors, &ast, true),
            walk_delta(cursors, &ast, false),
            "{shape}: the prune scanned a parent's children to read an attribute run; \
             read it through `Ancestors` instead (#1447)"
        );
    }
}

/// The `exclude_tests` walk asks about an attributed item once: the
/// attribute run's verdict names the item it decorates, so the walk
/// reuses it there instead of asking again (#1783).
///
/// The verdict is the item's own either way, so no value moves, and two
/// counters see the second ask:
///
/// - Each retained `mod` costs one `child_scan_cursors` cursor per
///   ask, because `rust_inner_attr_marks_test` scans its body for
///   `#![…]`. Re-asking reads two per `mod`.
/// - A pruned `#[cfg(test)] fn` pair under a parent wider than
///   `WIDE_PARENT` costs two `sibling_cache_reads`: the attribute looks
///   ahead to its item and back along its run. Re-asking adds the
///   item's backward read, so three. The width is even so that
///   `rust_root_of_width` adds no bare `fn`, whose own ask would be a
///   read more.
///
/// Reverting `rust_attribute_run_verdict` to
/// `covering_through(item.start_byte())` fails both.
#[cfg(feature = "rust")]
#[test]
fn the_exclude_tests_prune_asks_once_per_attributed_item() {
    use big_code_analysis_ast::node::WIDE_PARENT;
    use big_code_analysis_ast::test_support::rust_root_of_width;

    let mods = 3;
    let source = "#[inline]\nmod m {}\n".repeat(mods);
    let ast = crate::test_support::parse_named(crate::LANG::Rust, "lib.rs", &source);
    let cursors = crate::node::child_scan_cursors::observed;
    assert_eq!(
        walk_delta(cursors, &ast, true),
        walk_delta(cursors, &ast, false) + mods,
        "the prune did not scan each retained `mod` body exactly once"
    );

    let pairs = WIDE_PARENT / 2 + 1;
    let ast = crate::test_support::parse_named(
        crate::LANG::Rust,
        "lib.rs",
        &rust_root_of_width(2 * pairs),
    );
    assert_eq!(
        walk_delta(crate::node::sibling_cache_reads::observed, &ast, true),
        2 * pairs,
        "the prune asked about a pruned item after its attribute had answered"
    );
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
// `--features python,rust` (`docs/development/testing.md`).
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

/// Every seam that asks the JS-family binding question folds it through
/// the walk's `FoldCache`, so an arrow per level of a nested array costs
/// each ancestor once per walk rather than a climb per arrow (#1767).
///
/// Nothing in the output can see it: every arrow is bound to `x` either
/// way, so the function counts, the space tree and the markers come out
/// the same, and only the fold's two counts tell a memoised walk from
/// the quadratic climb. Both are exact (#1869). A seam that lent no cache
/// climbs instead and reads (0, 0); the suppression seam is the one whose
/// lend #1767 added. A memo that never matched would refold the whole
/// chain of every arrow `k`, `Σ (k + 2)` = 20,500 states and as many
/// slots per ask, and a search for the stored state started at the root
/// would read those 20,500 slots while folding the same 202 states.
#[cfg(feature = "javascript")]
#[test]
fn every_seam_folds_the_js_binding_question() {
    use crate::node::{fold_slot_probes, fold_steps};

    type Seam<'a> = &'a dyn Fn(&crate::Ast);

    let depth = 200;
    let source = format!(
        "x = {}0{};\n",
        "[() => 1,\n".repeat(depth),
        "]\n".repeat(depth)
    );
    let ast = crate::test_support::parse_named(crate::LANG::Javascript, "a.js", &source);
    // How often each seam asks about an arrow. `Ast::metrics` asks in its
    // promotion test and in `nom` and `nargs`, whose `is_func_with_code`
    // settles a bound arrow before `is_closure` is reached; cognitive
    // takes an arrow's own arm ahead of its callable test, and the space
    // kind and name read no binding. The other seams ask once.
    let seams: [(&str, usize, Seam); 4] = [
        ("Ast::metrics", 3, &|ast| {
            ast.metrics(crate::MetricsOptions::default())
                .expect("the walk must yield a top-level space");
        }),
        ("Ast::ops", 1, &|ast| {
            ast.ops().expect("the walk must yield a top-level space");
        }),
        ("Ast::functions", 1, &|ast| {
            assert_eq!(ast.functions().len(), depth, "every arrow is a function");
        }),
        ("Ast::suppressions", 1, &|ast| {
            assert!(
                ast.suppressions().is_empty(),
                "the fixture carries no marker"
            );
        }),
    ];
    // expected: every arrow's chain is a prefix of the innermost one's,
    // the program, the statement, the assignment and `depth` arrays, and
    // the fold stores a state per entry below the root, once whichever
    // end a walk starts from.
    let stored = depth + 2;
    // expected: on its first ask, left to right, the outermost arrow reads
    // its three empty slots and each later one the slot its own array
    // takes and the one above, which matches; right to left (the
    // suppression walk) the innermost reads all `depth + 2` and every
    // other arrow one, which matches: `2 * depth + 1` both ways. A repeat
    // ask reads the arrow's own array's slot alone.
    let read = |asks| 2 * depth + 1 + (asks - 1) * depth;
    let readings: Vec<_> = seams
        .iter()
        .map(|&(name, _, run)| {
            let before = (fold_steps::observed(), fold_slot_probes::observed());
            run(&ast);
            let folded = fold_steps::observed() - before.0;
            (name, folded, fold_slot_probes::observed() - before.1)
        })
        .collect();
    let expected: Vec<_> = seams
        .iter()
        .map(|&(name, asks, _)| (name, stored, read(asks)))
        .collect();
    assert_eq!(
        readings, expected,
        "(seam, states folded, slots probed) over {depth} levels"
    );
}

/// The cognitive walk folds "is this function inside a function" down
/// its chain, so a function in each of `depth` nested classes costs each
/// ancestor once per walk rather than a scan of the whole chain per
/// function (#1864).
///
/// The values cannot see it: the fold and the scan answer the same, and
/// only the fold's two counts tell them apart. The scan stores nothing
/// and reads 0 folded states. A memo that never matched would refold the
/// chain per method, about `depth²` states, and a search for the stored
/// state that read more slots than it refolds — one started at the root
/// — would probe about `depth²` slots while folding each state once.
/// Both counts are exact, so neither a full nor a partial relapse fits.
///
/// Each level holds a method before its nested class and one after it.
/// `m` asks on the way down, where its class and the body around it are
/// new to the lane; `n` asks on the way back up, where every ancestor is
/// stored and the first slot read must match. Only `Cognitive` is
/// selected, which brings in `Nom` alone, and neither folds another
/// question; Java's `loc`, which folds its own for each local variable
/// declaration, does not run. `p`, the method of the local class, is the
/// one function the question answers "yes" for, so the sum pins both
/// answers: one per method of a level, and two for the `if` under `p`.
#[cfg(feature = "java")]
#[test]
fn the_cognitive_walk_folds_the_enclosing_function_question() {
    use crate::node::{fold_slot_probes, fold_steps};

    let depth = 200;
    let source = format!(
        "{}void o() {{ class L {{ void p(boolean b) {{ if (b) {{ }} }} }} }}\n{}",
        "class C {\nvoid m(boolean a) { if (a) { } }\n".repeat(depth),
        "void n(boolean a) { if (a) { } }\n}\n".repeat(depth)
    );
    let ast = crate::test_support::parse_named(crate::LANG::Java, "C.java", &source);
    let options = crate::MetricsOptions::default().with_only(&[crate::Metric::Cognitive]);
    let before = (fold_steps::observed(), fold_slot_probes::observed());
    let space = ast
        .metrics(options)
        .expect("the walk must yield a top-level space");
    let folded = fold_steps::observed() - before.0;
    let probed = fold_slot_probes::observed() - before.1;
    assert_eq!(
        space.metrics.cognitive.cognitive_sum(),
        2 * depth as u64 + 2,
        "one `if` per method at no surcharge, and one under a nested function"
    );
    // expected: each `m` stores the two ancestors its level adds (the
    // class and its body), and `p` the four between it and the innermost
    // class body (`o`, its block, `L`, `L`'s body). `o` and every `n` find
    // their whole chain stored.
    let stored = 2 * depth + 4;
    // expected: each `m` reads its two new slots and then the one that
    // matches, except the outermost, which has none above to match; `p`
    // reads its four and one that matches; `o` and each `n` read one.
    let read = (3 * depth - 1) + 5 + 1 + depth;
    assert_eq!(
        (folded, probed),
        (stored, read),
        "(states folded, slots probed) over {depth} levels"
    );
}

#[cfg(feature = "rust")]
type Dump = fn(&crate::Ast, crate::AstCfg) -> crate::AstResponse;

/// Which export builder each `Ast` entry point runs (#1684).
///
/// `Ast::dump` and `Ast::dump_cursor` must enter only the cursor walk,
/// and `Ast::dump_indexed` only the indexed one. Nothing in the output
/// can say: the two walks produce the same tree, so a default re-routed
/// to the indexed builder would pass every assertion on a dump and show
/// up only as a quadratic `/ast` response over a flat recovery node.
/// The AST crate pins its own `dump_inner`; this pins the three `Ast`
/// forwarders above it, which the AST crate cannot see and which are
/// what the web and Python surfaces reach. Each delta is taken against
/// a seeded count, so a `record()` never wired up cannot pass either.
#[cfg(feature = "rust")]
#[test]
fn each_ast_dump_entry_point_enters_exactly_its_own_builder() {
    let ast = crate::test_support::parse_named(crate::LANG::Rust, "a.rs", "fn f() {}\n");
    let cfg = || crate::AstCfg {
        id: String::new(),
        language: "rust".to_owned(),
        comment: false,
        span: false,
    };
    // Seeded through `dump`, not `dump_cursor`, so a `dump_cursor`
    // rerouted to the indexed builder is reported by its own row below
    // rather than by the seed guard.
    let _ = ast.dump(cfg());
    let _ = ast.dump_indexed(cfg());
    let mut cursor_total = crate::ast::cursor_builds::observed();
    let mut indexed_total = crate::ast::indexed_builds::observed();
    assert!(
        cursor_total > 0 && indexed_total > 0,
        "the seed calls must be counted"
    );

    let entries: [(&str, Dump, usize, usize); 3] = [
        ("Ast::dump", crate::Ast::dump, 1, 0),
        ("Ast::dump_cursor", crate::Ast::dump_cursor, 1, 0),
        ("Ast::dump_indexed", crate::Ast::dump_indexed, 0, 1),
    ];
    for (name, dump, cursor_delta, indexed_delta) in entries {
        let _ = dump(&ast, cfg());
        cursor_total += cursor_delta;
        indexed_total += indexed_delta;
        assert_eq!(
            (
                crate::ast::cursor_builds::observed(),
                crate::ast::indexed_builds::observed()
            ),
            (cursor_total, indexed_total),
            "{name} must enter the cursor builder {cursor_delta} time(s) and the indexed \
             builder {indexed_delta} time(s)"
        );
    }
}

/// The word slot on the walk's `FoldCache` (#1697, #1737): the real
/// `Ast::metrics` walk reads an Elixir `call`'s target text once for the
/// visited node, however many of promotion, cognitive, cyclomatic, ABC,
/// nexits, NPM and NPA ask — the slot answers all but the first — and
/// once per ask about any *other* node, which is computed and never
/// stored. The output cannot show it (every read answers alike); only
/// the read counter does, and the AST crate's own tests drive a replica
/// walker, not this one. What this pins is that the walk records its
/// visit on every node (`Ancestors::for_walk`) and that nothing re-reads
/// the visited node after an ask about another: a lender that rebuilt
/// the chain without the visit would read once per metric, and a
/// consumer re-asking about an enclosing call pays one read per ask.
///
/// The budget, derived: one read per `call` at promotion (10), plus the
/// asks about other nodes, each computed without storing, in walk
/// order — while the `defmodule` is visited, NPM's read of its two
/// `def`-family statements under their own chain (2) and NPA's
/// `struct_macro` asked of the same two (2), the module's own word
/// staying in the slot throughout; the `def`'s quote fold reading the
/// enclosing `defmodule` through the text primitive, since the fold's
/// step chain lends no cache (1); the `else:` section's read of its
/// `if` from the pair (1); and the `CaseArm` gate's read of the
/// enclosing `case` for its one arm, by cyclomatic and ABC, through the
/// slot about a node the walk is not visiting (2). 18 reads — the
/// count #1697 derived for the last-node-asked slot, whose evictions
/// never happened to a node any later metric re-asked. That slot could
/// hold 18 only while NPM read its statements uncached; NPM now reads
/// them through the slot, and restoring the eviction (`classified`
/// storing every ask) reads the module again at NPA's `defmodule`
/// check, 19. The count no longer depends on `compute_per_node`'s
/// metric order.
#[cfg(feature = "elixir")]
#[test]
fn the_metric_walk_reads_each_elixir_call_target_once() {
    use crate::lang_helpers::elixir::call_keyword_reads;

    let source = "defmodule M do\n  defstruct [:a]\n  def f(a) do\n    if a, do: throw(:x), else: raise(\"y\")\n    case a do\n      _ -> g()\n    end\n    Kernel.exit(1)\n  end\nend\n";
    let ast = crate::test_support::parse_named(crate::LANG::Elixir, "m.ex", source);
    let mut call_nodes = 0;
    let mut stack = vec![ast.as_tree_sitter().root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == "call" {
            call_nodes += 1;
        }
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor));
    }
    assert_eq!(call_nodes, 10);

    let before = call_keyword_reads::observed();
    ast.metrics(crate::MetricsOptions::default())
        .expect("the walk must yield a top-level space");
    let reads = call_keyword_reads::observed() - before;
    assert_eq!(
        reads,
        call_nodes + 2 + 2 + 1 + 1 + 2,
        "one read per call, plus the priced asks about other nodes"
    );
}

/// The Tcl twin: the real metric walk reads a `command`'s leading word
/// once for the visited node, however many of cognitive, cyclomatic,
/// ABC and nexits ask (#1697, #1737). The asks about other nodes are
/// all cyclomatic's `CaseArm` gate reading a candidate arm's
/// grandparent through the slot — about a node the walk is not
/// visiting, so computed and not stored — for every command three
/// levels under another: the arm list's command reads `switch`, the
/// two `return`s read the arm-list command, whose leading word is `x`
/// (`x { return 1 } y { return 2 }` is one `command` named `x`, `y` a
/// `simple_word` in its `word_list`, so in this spelling the `y` arm
/// is not an arm to the gate), and the `for` body's three commands
/// (`$i < 3`, `incr`, `puts`) read `for` — six reads, five of them
/// rejected candidates. ABC does not read the arm row. Each ask
/// evicted nothing under the last-node-asked slot either, so the count
/// is the same as before #1737.
#[cfg(feature = "tcl")]
#[test]
fn the_metric_walk_reads_each_tcl_command_word_once() {
    use crate::lang_helpers::tcl::command_name_reads;

    let source = "proc f {a} {\n    switch -- $a { x { return 1 } y { return 2 } }\n    for {set i 0} {$i < 3} {incr i} { puts $i }\n    exit 0\n}\n";
    let ast = crate::test_support::parse_named(crate::LANG::Tcl, "f.tcl", source);
    let mut commands = 0;
    let mut stack = vec![ast.as_tree_sitter().root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == "command" {
            commands += 1;
        }
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor));
    }
    // switch, the arm list's one command, return 1, return 2, for,
    // `$i < 3`, incr, puts, exit.
    assert_eq!(commands, 9);

    let before = command_name_reads::observed();
    ast.metrics(crate::MetricsOptions::default())
        .expect("the walk must yield a top-level space");
    let reads = command_name_reads::observed() - before;
    assert_eq!(
        reads,
        commands + 6,
        "one word read per command, plus the arm gate's six grandparent reads"
    );
}

/// The member walks derive a chain per container body, never per node
/// they visit (#1696, lesson 103): over a class whose one method holds a
/// `depth`-deep expression, the `npa` and `npm` walks derive the same
/// number of chains at any depth.
///
/// The scaling probe `npa/java-deep-expression` priced a build hoisted
/// ahead of the body-kind test while that build was an `O(depth)` copy;
/// since #1805 it is one allocation, a constant factor whose exponent
/// stays under the bound, so only this counter tells a per-body
/// derivation from a per-node one. Compared across two depths rather
/// than against a fixed count, so the test pins the invariant and not
/// today's number of consumers; a per-node build grows the second count
/// by the parentheses added.
#[cfg(feature = "java")]
#[test]
fn the_member_walks_derive_a_chain_per_body_not_per_node() {
    use crate::node::child_chains_derived;

    let class = |depth: usize| {
        format!(
            "class C {{ int x; int f() {{ return {}1{}; }} }}\n",
            "(".repeat(depth),
            ")".repeat(depth)
        )
    };
    let derived = |depth: usize| {
        let ast = crate::test_support::parse_named(crate::LANG::Java, "C.java", &class(depth));
        let nodes = ast.root_node().preorder().count();
        let before = child_chains_derived::observed();
        ast.metrics(
            crate::MetricsOptions::default().with_only(&[crate::Metric::Npa, crate::Metric::Npm]),
        )
        .expect("the walk must yield a top-level space");
        (child_chains_derived::observed() - before, nodes)
    };
    let (shallow, shallow_nodes) = derived(2);
    let (deep, deep_nodes) = derived(200);
    assert!(
        deep_nodes > shallow_nodes + 300,
        "the deep fixture must visit many more nodes: {shallow_nodes} vs {deep_nodes}"
    );
    assert!(
        shallow > 0,
        "the member walks derive a chain for the class body"
    );
    assert_eq!(
        shallow, deep,
        "a chain was derived per visited node, not per container body (#1696)"
    );
}

/// Kotlin's Halstead classification asks for a previous sibling only of
/// a `string_content` token whose own text could be the `name` of a
/// short template's `$name` (#454). `get_operand_id` asked it of every
/// operand, ahead of the kind test inside the short-template check, so
/// each identifier and literal paid a lookup that could only answer
/// "not a name"; and the check asked it of every `$` sigil before
/// reading the sigil's own text (#1871).
///
/// The operand keys cannot see it: the answer was discarded each time.
/// Each row gives the seeks and the record reads apart: a lookup seeks
/// under a parent of at most `WIDE_PARENT` children and reads the
/// walk's record under a wider one, which the last row's string is.
/// The two counters of lookups that resolve from the node itself, the
/// spelling that goes round `Ancestors`, are held at zero; the parent
/// one only without `make chain-audit`, whose exact check asks
/// `Node::parent` of every chain a walk builds. Only Halstead is
/// selected, and no fixture spells a modifier keyword, whose row gate
/// asks for a sibling of its own.
#[cfg(feature = "kotlin")]
#[test]
fn kotlin_halstead_asks_for_a_sibling_only_of_string_content() {
    use crate::node::{
        WIDE_PARENT, node_resolved_parent_lookups, node_resolved_sibling_lookups,
        sibling_cache_reads, sibling_seeks,
    };

    type Seam = fn(&crate::Ast);
    let seams: [(&str, Seam); 2] = [
        ("Ast::metrics", |ast| {
            ast.metrics(crate::MetricsOptions::default().with_only(&[crate::Metric::Halstead]))
                .expect("the walk must yield a top-level space");
        }),
        ("Ast::ops", |ast| {
            ast.ops().expect("the walk must yield a top-level space");
        }),
    ];
    // (seeks, record reads), and the siblings and parents resolved from
    // a node itself.
    let lookups = || (sibling_seeks::observed(), sibling_cache_reads::observed());
    let resolved = || {
        (
            node_resolved_sibling_lookups::observed(),
            node_resolved_parent_lookups::observed(),
        )
    };

    let wide = format!(
        "fun h(a: Int): String {{\n    return \"{}\"\n}}\n",
        "$a ".repeat(WIDE_PARENT)
    );
    // Each row: the source, its identifier and number leaves (operands
    // that are no `string_content`), its `string_content` tokens, and
    // the sibling lookups one walk makes, as (seeks, record reads).
    let rows = [
        (
            "no string",
            "fun f(a: Int, b: Int): Int {\n    val c = a + b + 1\n    return c * a\n}\n",
            12,
            0,
            // expected: none, the fixture holds no `string_content`.
            (0, 0),
        ),
        (
            "a plain string",
            "fun p(): String {\n    return \"plain\"\n}\n",
            2,
            1,
            // expected: 1 seek. The token's text could be a name, so
            // `get_op_type_with_code` asks; the token before it is the
            // quote, so it is none, and the literal is the operand.
            (1, 0),
        ),
        (
            "a short template",
            "fun g(a: Int, b: Int): String {\n    return \"$a and $b\"\n}\n",
            6,
            4,
            // expected: 4 seeks, the string being narrow. Of the four
            // tokens (`$`, `a and `, `$`, `b`) the two sigils are told by
            // their own text. `get_op_type_with_code` asks once of each
            // of the other two, and `get_operand_id` once more, since
            // both are names.
            (4, 0),
        ),
        (
            "a template wider than `WIDE_PARENT`",
            wide.as_str(),
            4,
            2 * WIDE_PARENT,
            // expected: as above per repeat, for the name: one ask by
            // `get_op_type_with_code` and one by `get_operand_id`, both
            // answered from the record of the string's children.
            (0, 2 * WIDE_PARENT),
        ),
    ];
    for (shape, source, plain_operands, string_contents, expected) in rows {
        let ast = crate::test_support::parse_named(crate::LANG::Kotlin, "f.kt", source);
        let count = |kinds: &[&str]| {
            ast.root_node()
                .preorder()
                .filter(|node| kinds.contains(&node.kind()))
                .count()
        };
        assert_eq!(
            count(&["identifier", "number_literal"]),
            plain_operands,
            "{shape}: operands that are no `string_content`"
        );
        assert_eq!(
            count(&["string_content"]),
            string_contents,
            "{shape}: `string_content` tokens"
        );
        for (seam, run) in seams {
            let (seeks, reads) = lookups();
            let (siblings, parents) = resolved();
            run(&ast);
            let (sought, read) = lookups();
            assert_eq!(
                (sought - seeks, read - reads),
                expected,
                "{shape}: {seam} asked for the previous sibling of a node that could be no \
                 name, or stopped asking it of one that could (seeks, record reads)"
            );
            let (siblings_after, parents_after) = resolved();
            assert_eq!(
                siblings_after, siblings,
                "{shape}: {seam} resolved a sibling from a node instead of through the chain"
            );
            if !cfg!(chain_audit) {
                assert_eq!(
                    parents_after, parents,
                    "{shape}: {seam} resolved a parent from a node instead of through the chain"
                );
            }
        }
    }
}

/// Perl ABC's pattern-role check reads a pattern's neighbours in its
/// list through the walk's record of the list's children, and never by
/// a seek or a scan from the list's first child (#1868).
///
/// Two of the lists open with a run of comments. Those are the list's
/// own children, outside the `repeat()` its operands are stored under,
/// so a read that starts at the first child steps over every one of
/// them, once per pattern, and answers what the record answers. No
/// value shows it, and `sibling_cache_reads` does. Every list is wider
/// than `WIDE_PARENT`, so the walk recorded it and no lookup of it
/// seeks.
///
/// - A plain list: each pattern asks for the sibling after it, to tell
///   a last operand, and nothing else, since no call holds the list.
///   A `Node::children_after` in its place never reads the record.
/// - `split`'s list: each pattern is asked for the operands before it,
///   to tell the delimiter, and not for the one after, since the list
///   is the call's own. A `children().find(..)` or a
///   `Node::children_before` in its place never reads the record. The
///   first pattern has none before it and is the delimiter, which pays
///   nothing.
/// - A binding's list: the pattern is its last operand, so it climbs to
///   the `=~` that binds it and only the `=~` pays.
/// - A parenthesised second argument: the pattern's own read is a seek
///   inside its narrow parenthesis, and the parenthesis is then asked
///   for the operands before it in the list, through the list's
///   record. A climb that kept the pattern's chain for the parenthesis
///   would seek inside the parenthesis again, find nothing before it,
///   and make the pattern `split`'s delimiter.
#[cfg(feature = "perl")]
#[test]
fn the_perl_pattern_role_check_reads_a_commented_list_through_the_sibling_record() {
    use crate::node::{WIDE_PARENT, sibling_cache_reads, sibling_seeks};

    let n = 40;
    let (comments, patterns, fillers) = ("# c\n".repeat(n), "/a/, ".repeat(n), "1, ".repeat(n));
    // Each row: the source, the children of its list, and what one
    // `abc` walk makes of it as (conditions, seeks, record reads).
    let rows = [
        (
            "a commented list",
            format!("my @r = (\n{comments}{patterns}1);\n"),
            // expected: the parentheses, the comments, each pattern and
            // its comma, and the closing `1`.
            3 * n + 3,
            (n, 0, n),
        ),
        (
            "a commented `split` list",
            format!("my @p = split(\n{comments}{patterns}1);\n"),
            3 * n + 3,
            (n - 1, 0, n),
        ),
        (
            "a binding's list",
            format!("my $r = $x =~ ({fillers}/foo/);\n"),
            // expected: the parentheses, each filler and its comma, and
            // the pattern.
            2 * n + 3,
            (1, 0, 1),
        ),
        (
            "a parenthesised second argument",
            format!("my @p = split(1, (/x/), {fillers}3);\n"),
            // expected: the parentheses, `1` and `(/x/)` each with a
            // comma, each filler and its comma, and the closing `3`.
            2 * n + 7,
            (1, 1, 1),
        ),
    ];
    let lookups = || (sibling_seeks::observed(), sibling_cache_reads::observed());
    for (shape, source, widest, (conditions, seeks, reads)) in rows {
        let ast = crate::test_support::parse_named(crate::LANG::Perl, "list.pl", &source);
        assert_eq!(
            ast.root_node().preorder().map(|n| n.child_count()).max(),
            Some(widest),
            "{shape}: the fixture's list moved; re-derive its counts"
        );
        assert!(
            widest > WIDE_PARENT,
            "{shape}: the walk must record the list"
        );

        let (sought_before, read_before) = lookups();
        let space = ast
            .metrics(crate::MetricsOptions::default().with_only(&[crate::Metric::Abc]))
            .expect("the walk must yield a top-level space");
        let (sought, read) = lookups();
        assert_eq!(
            (
                space.metrics.abc.conditions_sum(),
                sought - sought_before,
                read - read_before
            ),
            (
                u64::try_from(conditions).expect("a small count"),
                seeks,
                reads
            ),
            "{shape}: (conditions, seeks, record reads)"
        );
    }
}

// The member walks ask for no sibling (#1871): the fixture table and the
// test. The table names each language as a `LANG` value and filters at
// run time with `is_enabled`, so the module is gated on the union.
#[cfg(any(
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "typescript",
))]
mod member_walk_siblings {
    use crate::node::{
        Ancestors, FoldCache, SiblingCache, WIDE_PARENT, child_chains_derived, sibling_cache_reads,
        sibling_seeks,
    };
    use crate::{LANG, Node};

    /// What the member walks count over a whole fixture.
    #[derive(Clone, Copy)]
    enum Count {
        /// One per unit: `WIDE_PARENT + 1`.
        Each,
        /// This many, whatever the width.
        Just(u64),
    }

    /// The chains the member walks derive with `Ancestors::of_children`
    /// over a whole fixture.
    #[derive(Clone, Copy)]
    enum Chains {
        /// This many, whatever the width.
        Just(usize),
        /// This many, and one more per unit.
        JustAndEach(usize),
    }

    /// One source: `head`, then `unit` repeated `WIDE_PARENT + 1` times,
    /// then `tail`.
    struct Fixture {
        lang: LANG,
        source: [&'static str; 3],
        /// The kind of the body the repeats make wider than
        /// `WIDE_PARENT`.
        wide_body: &'static str,
        /// The methods `npm` counts and the attributes `npa` counts:
        /// the witness that each walk read the wide body's members.
        methods: Count,
        attributes: Count,
        /// The chains the two walks derive: a per-body constant, so a
        /// walk that came to derive one per member moves it.
        chains: Chains,
    }

    const JAVA: [&str; 3] = ["class C {\n", "int x;\nvoid m() {}\n", "}\n"];
    const CPP: [&str; 3] = ["class C {\npublic:\n", "int x;\nvoid m() {}\n", "};\n"];
    const JS: [&str; 3] = ["class C {\n", "x = 1;\nm() {}\n", "}\n"];
    const TS: [&str; 3] = ["class C {\n", "public x = 1;\nm() {}\n", "}\n"];

    /// A row for every language whose `Npa` or `Npm` is not the no-op
    /// impl, and two for Rust and for Go, whose attributes and methods
    /// sit in different bodies. Python counts an attribute once however
    /// often it is assigned (the class's `x`, the instance's `y`).
    /// Elixir's one attribute is the `defstruct` field.
    ///
    /// Most walks derive one chain per body, with `Ancestors::of_children`,
    /// and read the members through it: two chains a fixture, one for
    /// `npa` and one for `npm`. A sibling asked for through such a chain
    /// seeks. Rust and Go derive two for a struct, the struct's and its
    /// field list's, in `npa` alone; Elixir two in each walk, the
    /// module's and its `do` block's. Three rows derive none. Rust's
    /// `impl` methods and Go's receiver methods are counted as the walk
    /// visits each, through the walk's own chain, where an ask reads the
    /// record. Python's members are asked from the class, through the
    /// class's chain, where an ask looks under the class's parent.
    const FIXTURES: &[Fixture] = &[
        Fixture {
            lang: LANG::Java,
            source: JAVA,
            wide_body: "class_body",
            methods: Count::Each,
            attributes: Count::Each,
            chains: Chains::Just(2),
        },
        Fixture {
            lang: LANG::Groovy,
            source: ["class C {\n", "int x\nvoid m() {}\n", "}\n"],
            wide_body: "class_body",
            methods: Count::Each,
            attributes: Count::Each,
            chains: Chains::Just(2),
        },
        Fixture {
            lang: LANG::Csharp,
            source: ["class C {\n", "public int x;\nvoid M() {}\n", "}\n"],
            wide_body: "declaration_list",
            methods: Count::Each,
            attributes: Count::Each,
            // FIXME(#1879): `npm` derives a chain for every node with a
            // `parameter_list`, so one per method, before it knows the
            // node is a type declaration. The constant is the two.
            chains: Chains::JustAndEach(2),
        },
        Fixture {
            lang: LANG::Kotlin,
            source: ["class C {\n", "val x = 1\nfun m() {}\n", "}\n"],
            wide_body: "class_body",
            methods: Count::Each,
            attributes: Count::Each,
            chains: Chains::Just(2),
        },
        Fixture {
            lang: LANG::Cpp,
            source: CPP,
            wide_body: "field_declaration_list",
            methods: Count::Each,
            attributes: Count::Each,
            chains: Chains::Just(2),
        },
        Fixture {
            lang: LANG::Mozcpp,
            source: CPP,
            wide_body: "field_declaration_list",
            methods: Count::Each,
            attributes: Count::Each,
            chains: Chains::Just(2),
        },
        Fixture {
            lang: LANG::Objc,
            source: [
                "@interface C : NSObject\n",
                "@property int x;\n- (void)m;\n",
                "@end\n",
            ],
            wide_body: "class_interface",
            methods: Count::Each,
            attributes: Count::Each,
            chains: Chains::Just(2),
        },
        Fixture {
            lang: LANG::Php,
            source: ["<?php\nclass C {\n", "public $x;\nfunction m() {}\n", "}\n"],
            wide_body: "declaration_list",
            methods: Count::Each,
            attributes: Count::Each,
            chains: Chains::Just(2),
        },
        Fixture {
            lang: LANG::Python,
            source: [
                "class C:\n",
                "    x = 1\n    def m(self):\n        self.y = 1\n",
                "",
            ],
            wide_body: "block",
            methods: Count::Each,
            attributes: Count::Just(2),
            chains: Chains::Just(0),
        },
        Fixture {
            lang: LANG::Ruby,
            source: ["class C\n", "  attr_reader :x\n  def m\n  end\n", "end\n"],
            wide_body: "body_statement",
            methods: Count::Each,
            attributes: Count::Each,
            chains: Chains::Just(2),
        },
        Fixture {
            lang: LANG::Rust,
            source: ["struct S {\n", "pub x: i32,\n", "}\n"],
            wide_body: "field_declaration_list",
            methods: Count::Just(0),
            attributes: Count::Each,
            chains: Chains::Just(2),
        },
        Fixture {
            lang: LANG::Rust,
            source: ["struct S;\nimpl S {\n", "pub fn m(&self) {}\n", "}\n"],
            wide_body: "declaration_list",
            methods: Count::Each,
            attributes: Count::Just(0),
            chains: Chains::Just(0),
        },
        Fixture {
            lang: LANG::Go,
            source: ["package p\ntype S struct {\n", "X int\n", "}\n"],
            wide_body: "field_declaration_list",
            methods: Count::Just(0),
            attributes: Count::Each,
            chains: Chains::Just(2),
        },
        Fixture {
            lang: LANG::Go,
            source: ["package p\ntype S int\n", "func (s S) M() {}\n", ""],
            wide_body: "source_file",
            methods: Count::Each,
            attributes: Count::Just(0),
            chains: Chains::Just(0),
        },
        Fixture {
            lang: LANG::Javascript,
            source: JS,
            wide_body: "class_body",
            methods: Count::Each,
            attributes: Count::Each,
            chains: Chains::Just(2),
        },
        Fixture {
            lang: LANG::Mozjs,
            source: JS,
            wide_body: "class_body",
            methods: Count::Each,
            attributes: Count::Each,
            chains: Chains::Just(2),
        },
        Fixture {
            lang: LANG::Typescript,
            source: TS,
            wide_body: "class_body",
            methods: Count::Each,
            attributes: Count::Each,
            chains: Chains::Just(2),
        },
        Fixture {
            lang: LANG::Tsx,
            source: TS,
            wide_body: "class_body",
            methods: Count::Each,
            attributes: Count::Each,
            chains: Chains::Just(2),
        },
        Fixture {
            lang: LANG::Elixir,
            source: [
                "defmodule M do\n  defstruct [:a]\n",
                "  def a, do: 1\n",
                "end\n",
            ],
            wide_body: "do_block",
            methods: Count::Each,
            attributes: Count::Just(1),
            chains: Chains::Just(4),
        },
    ];

    /// The languages whose `Npa` and `Npm` are the no-op impls
    /// (`implement_metric_trait!` in `metrics/npa.rs` and
    /// `metrics/npm.rs`): no member walk runs for them. The test holds
    /// the list to `HAS_MEMBERS`, read off the blocks the walk emits, so
    /// one of them gaining a member walk fails until it has a fixture.
    const NO_MEMBER_WALK: &[LANG] = &[
        LANG::C,
        LANG::Preproc,
        LANG::Ccomment,
        LANG::Perl,
        LANG::Bash,
        LANG::Lua,
        LANG::Tcl,
        LANG::Irules,
    ];

    /// Whether the walk emits an `npm` or `npa` block for `lang`'s file
    /// root, which it does exactly where `Npm::HAS_MEMBERS` or
    /// `Npa::HAS_MEMBERS` is set. Read off the serialized space: the
    /// stats are always there as a struct, and only the key is gated.
    fn emits_a_member_block(lang: LANG) -> bool {
        let space = crate::test_support::parse_named(lang, "fixture", "\n")
            .metrics(
                crate::MetricsOptions::default()
                    .with_only(&[crate::Metric::Npa, crate::Metric::Npm]),
            )
            .expect("the walk must yield a top-level space");
        let value = serde_json::to_value(&space).expect("a space serializes");
        let metrics = value["metrics"]
            .as_object()
            .expect("a space serializes a metrics object");
        metrics.contains_key("npm") || metrics.contains_key("npa")
    }

    /// The `npa` and `npm` walks ask for no sibling of a member, under a
    /// body wider than `WIDE_PARENT`, in any language that has them
    /// (#1871). The members are the two plainest each language has, a
    /// field and a method: a gate on another member kind, such as a
    /// record header or an accessor, is not asked here.
    ///
    /// Most of them read a body's members through a chain derived with
    /// `Ancestors::of_children`, which carries no sibling record: the
    /// walk has not recorded the body's children while it visits the
    /// container. A row gate that asked for a sibling of each member
    /// would seek from the body's first child every time, quadratic in
    /// the members, and answer the same. `sibling_seeks` is what sees
    /// it. A walk that counts each member as it is visited, as Rust's
    /// `impl` row does, would read the record instead, so
    /// `sibling_cache_reads` is held at zero as well. Both counts cover
    /// the whole walk with only `npa` and `npm` selected, so a
    /// space-opening gate that starts to ask moves them too.
    ///
    /// A probe in `Ancestors::lookup`, run over the whole suite and the
    /// repository corpora, found no walk asking for a sibling through a
    /// derived chain, under a wide parent or a narrow one. The predicate
    /// it found that can is Java's package gate, asked of a parse
    /// error's children;
    /// `a_wide_error_s_children_ask_for_one_sibling_in_java` in
    /// `lang_helpers/doc_comment.rs` counts its seek. So the walk's
    /// record is not forwarded to a derived chain: no caller would read
    /// it.
    #[test]
    fn the_member_walks_ask_for_no_sibling_under_a_wide_body() {
        for lang in LANG::into_enum_iter() {
            let has_fixture = FIXTURES.iter().any(|row| row.lang == lang);
            assert_eq!(
                has_fixture,
                !NO_MEMBER_WALK.contains(&lang),
                "{} needs a fixture, or a place among the languages with no member walk",
                lang.name()
            );
            if lang.is_enabled() {
                assert_eq!(
                    emits_a_member_block(lang),
                    has_fixture,
                    "{}: the walk runs `npa` or `npm` for exactly the languages with a fixture",
                    lang.name()
                );
            }
        }
        let units = WIDE_PARENT + 1;
        let expected = |count: Count| match count {
            Count::Each => units as u64,
            Count::Just(fixed) => fixed,
        };
        let mut ran = 0;
        for fixture in FIXTURES.iter().filter(|fixture| fixture.lang.is_enabled()) {
            ran += 1;
            let [head, unit, tail] = fixture.source;
            let source = format!("{head}{}{tail}", unit.repeat(units));
            let wide_body = fixture.wide_body;
            let label = format!("{} under a wide {wide_body}", fixture.lang.name());
            let ast = crate::test_support::parse_named(fixture.lang, "fixture", &source);
            let root = ast.root_node();
            assert!(
                !root.preorder().any(|node| node.is_error()),
                "{label}: the fixture must parse"
            );
            let body = root
                .preorder()
                .find(|node| node.kind() == wide_body && node.child_count() > WIDE_PARENT)
                .unwrap_or_else(|| panic!("{label}: the fixture holds no such body"));
            // A positive control for each counter, so the zeros below
            // are walks that asked for nothing: a seek through a chain
            // that lends no record, and a read through a walk's chain
            // that recorded the body.
            let mut chain: Vec<Node> = std::iter::successors(Some(body), Node::parent).collect();
            chain.reverse();
            let member = body.child(0).expect("a wide body has a first child");
            let (seeks, reads) = (sibling_seeks::observed(), sibling_cache_reads::observed());
            let _ = Ancestors::known(&chain).siblings_after(&member).next();
            let mut recorded = SiblingCache::default();
            recorded.record(&body, chain.len() - 1, body.children());
            let folds = FoldCache::default();
            let _ = Ancestors::for_walk(&chain, &member, &folds, &recorded)
                .siblings_after(&member)
                .next();
            assert_eq!(
                (
                    sibling_seeks::observed() - seeks,
                    sibling_cache_reads::observed() - reads
                ),
                (1, 1),
                "{label}: the controls (seeks, record reads)"
            );

            let (seeks, reads, derived) = (
                sibling_seeks::observed(),
                sibling_cache_reads::observed(),
                child_chains_derived::observed(),
            );
            let space = ast
                .metrics(
                    crate::MetricsOptions::default()
                        .with_only(&[crate::Metric::Npa, crate::Metric::Npm]),
                )
                .expect("the walk must yield a top-level space");
            assert_eq!(
                (space.metrics.npm.total_nm(), space.metrics.npa.total_na()),
                (expected(fixture.methods), expected(fixture.attributes)),
                "{label}: the methods and attributes the walks counted"
            );
            let chains = match fixture.chains {
                Chains::Just(fixed) => fixed,
                Chains::JustAndEach(fixed) => fixed + units,
            };
            assert_eq!(
                child_chains_derived::observed() - derived,
                chains,
                "{label}: the chains the member walks derived"
            );
            // expected: 0 and 0, no member row's gate asks for a sibling.
            assert_eq!(
                (
                    sibling_seeks::observed() - seeks,
                    sibling_cache_reads::observed() - reads
                ),
                (0, 0),
                "{label}: a member walk asked for a sibling (seeks, record reads); under a \
                 derived chain each ask seeks from the body's first child"
            );
        }
        assert!(ran > 0, "no fixture's language is enabled");
    }
}

// The walks that record no siblings ask for none (#1601): the fixture
// table, the witnesses that each walk did its work, and the test.
// The table names every language as a `LANG` value and filters at run
// time with `is_enabled`, so the module is gated on the union.
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "c-family-helpers",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
mod no_sibling_walks {
    use crate::LANG;
    use crate::node::{Ancestors, WIDE_PARENT, sibling_seeks};

    /// One source: `head`, then `unit` repeated `WIDE_PARENT + 1` times,
    /// then `tail`.
    struct Fixture {
        lang: LANG,
        /// `[head, unit, tail]`. The head carries the one suppression
        /// marker and each unit one `comment`.
        source: [&'static str; 3],
        /// The kind of the parent the repeats make wider than
        /// `WIDE_PARENT`.
        wide_parent: &'static str,
        /// The kinds the source must spell for a chain-reading predicate
        /// of these walks to run.
        constructs: &'static [&'static str],
        /// The comment each unit carries, which `strip_comments` removes.
        comment: &'static str,
    }

    const JS_UNIT: &str = "function a() {}\nconst f = function () {};\nconst g = () => 1;\n// c\n";
    const JS_KINDS: &[&str] = &[
        "function_declaration",
        "function_expression",
        "arrow_function",
    ];

    /// A fixture for every `LANG` variant, the two C-family helper
    /// grammars included, and a second shape for Rust. The
    /// wide parent is a root or a body, a flat `ERROR` (Rust, Python,
    /// Perl) or an argument list (Tcl, iRules). Where a predicate of
    /// these walks reads the chain, the unit spells what makes it run:
    /// a comment (Rust's `is_useful_comment` reads its parent), a JS
    /// family function expression and arrow (the binding gates), a Java
    /// or Groovy record header and a C# primary constructor (the gate,
    /// and the name taken from the owner), a Kotlin primary constructor
    /// (its name), an Elixir `def` (the `quote` fold), Tcl and iRules
    /// braced words (`keeps_children`). A gate on a row that is not
    /// `callable`, such as Ruby's `not_a_lambda_body`, is not asked by
    /// these walks.
    const FIXTURES: &[Fixture] = &[
        Fixture {
            lang: LANG::Rust,
            source: ["// bca: suppress\n", "fn a() {}\n// c\n", ""],
            wide_parent: "source_file",
            constructs: &["line_comment"],
            comment: "// c",
        },
        Fixture {
            lang: LANG::Rust,
            source: ["fn f() {\n// bca: suppress\n", "let x = ) ; // c\n", ""],
            wide_parent: "ERROR",
            constructs: &["line_comment"],
            comment: "// c",
        },
        Fixture {
            lang: LANG::Python,
            source: ["def f():\n    # bca: suppress\n", "    x = ) # c\n", ""],
            wide_parent: "ERROR",
            constructs: &["comment"],
            comment: "# c",
        },
        Fixture {
            lang: LANG::Perl,
            source: [
                "# bca: suppress\nmy @r = (",
                "{\nlast,\n$a && $b, # c\n",
                "",
            ],
            wide_parent: "ERROR",
            constructs: &["comments"],
            comment: "# c",
        },
        Fixture {
            lang: LANG::Java,
            source: [
                "class C {\n// bca: suppress\n",
                "void a() {}\nrecord R(int x) {}\n// c\n",
                "}\n",
            ],
            wide_parent: "class_body",
            constructs: &["method_declaration", "record_declaration"],
            comment: "// c",
        },
        Fixture {
            lang: LANG::Groovy,
            source: [
                "class C {\n// bca: suppress\n",
                "void a() {}\nrecord R(int x) {}\n// c\n",
                "}\n",
            ],
            wide_parent: "class_body",
            constructs: &["method_declaration", "record_components"],
            comment: "// c",
        },
        Fixture {
            lang: LANG::Csharp,
            source: [
                "class C {\n// bca: suppress\n",
                "void A() {}\nclass P(int x) {}\n// c\n",
                "}\n",
            ],
            wide_parent: "declaration_list",
            constructs: &["method_declaration", "parameter_list"],
            comment: "// c",
        },
        Fixture {
            lang: LANG::Kotlin,
            source: [
                "// bca: suppress\n",
                "fun a() {}\nclass K(val x: Int)\n// c\n",
                "",
            ],
            wide_parent: "source_file",
            constructs: &["function_declaration", "primary_constructor"],
            comment: "// c",
        },
        Fixture {
            lang: LANG::Javascript,
            source: ["// bca: suppress\n", JS_UNIT, ""],
            wide_parent: "program",
            constructs: JS_KINDS,
            comment: "// c",
        },
        Fixture {
            lang: LANG::Mozjs,
            source: ["// bca: suppress\n", JS_UNIT, ""],
            wide_parent: "program",
            constructs: JS_KINDS,
            comment: "// c",
        },
        Fixture {
            lang: LANG::Typescript,
            source: ["// bca: suppress\n", JS_UNIT, ""],
            wide_parent: "program",
            constructs: JS_KINDS,
            comment: "// c",
        },
        Fixture {
            lang: LANG::Tsx,
            source: ["// bca: suppress\n", JS_UNIT, ""],
            wide_parent: "program",
            constructs: JS_KINDS,
            comment: "// c",
        },
        Fixture {
            lang: LANG::Ruby,
            source: ["# bca: suppress\n", "def a; end\n# c\n", ""],
            wide_parent: "program",
            constructs: &["method"],
            comment: "# c",
        },
        Fixture {
            lang: LANG::Elixir,
            source: [
                "defmodule M do\n# bca: suppress\n",
                "def a, do: 1\n# c\n",
                "end\n",
            ],
            wide_parent: "do_block",
            constructs: &["call"],
            comment: "# c",
        },
        Fixture {
            lang: LANG::Tcl,
            source: [
                "# bca: suppress\nproc f {} {\n# c\neval ",
                "{a b} ",
                "]\n}\n",
            ],
            wide_parent: "word_list",
            constructs: &["braced_word"],
            comment: "# c",
        },
        Fixture {
            lang: LANG::Irules,
            source: [
                "# bca: suppress\nwhen HTTP_REQUEST {\n# c\neval ",
                "{a b} ",
                "]\n}\n",
            ],
            wide_parent: "word_list",
            constructs: &["braced_word"],
            comment: "# c",
        },
        Fixture {
            lang: LANG::Go,
            source: ["package p\n// bca: suppress\n", "func a() {}\n// c\n", ""],
            wide_parent: "source_file",
            constructs: &["function_declaration"],
            comment: "// c",
        },
        Fixture {
            lang: LANG::C,
            source: ["// bca: suppress\n", "void a(void) {}\n// c\n", ""],
            wide_parent: "translation_unit",
            constructs: &["function_definition"],
            comment: "// c",
        },
        Fixture {
            lang: LANG::Cpp,
            source: ["// bca: suppress\n", "void a() {}\n// c\n", ""],
            wide_parent: "translation_unit",
            constructs: &["function_definition"],
            comment: "// c",
        },
        Fixture {
            lang: LANG::Mozcpp,
            source: ["// bca: suppress\n", "void a() {}\n// c\n", ""],
            wide_parent: "translation_unit",
            constructs: &["function_definition"],
            comment: "// c",
        },
        Fixture {
            lang: LANG::Objc,
            source: ["// bca: suppress\n", "void a(void) {}\n// c\n", ""],
            wide_parent: "translation_unit",
            constructs: &["function_definition"],
            comment: "// c",
        },
        Fixture {
            lang: LANG::Php,
            source: ["<?php\n// bca: suppress\n", "function a() {}\n// c\n", ""],
            wide_parent: "program",
            constructs: &["function_definition"],
            comment: "// c",
        },
        Fixture {
            lang: LANG::Lua,
            source: ["-- bca: suppress\n", "function a() end\n-- c\n", ""],
            wide_parent: "chunk",
            constructs: &["function_declaration"],
            comment: "-- c",
        },
        Fixture {
            lang: LANG::Bash,
            source: ["# bca: suppress\n", "a() { :; }\n# c\n", ""],
            wide_parent: "program",
            constructs: &["function_definition"],
            comment: "# c",
        },
        Fixture {
            lang: LANG::Ccomment,
            source: ["// bca: suppress\n", "void a(void) {}\n// c\n", ""],
            wide_parent: "translation_unit",
            constructs: &["comment"],
            comment: "// c",
        },
        Fixture {
            lang: LANG::Preproc,
            source: ["// bca: suppress\n", "#define A 1\n// c\n", ""],
            wide_parent: "translation_unit",
            constructs: &["define", "comment"],
            comment: "// c",
        },
    ];

    fn contains(haystack: &[u8], needle: &str) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle.as_bytes())
    }

    fn finds_the_marker(ast: &crate::Ast, _: &Fixture, label: &str) {
        // expected: 1, the marker in the fixture's head.
        assert_eq!(ast.suppressions().len(), 1, "{label}: markers found");
    }

    fn strips_every_comment(ast: &crate::Ast, fixture: &Fixture, label: &str) {
        let stripped = ast.strip_comments().expect("the fixture holds comments");
        assert!(!stripped.is_empty(), "{label}: the code was stripped too");
        assert!(
            !contains(&stripped, fixture.comment) && !contains(&stripped, "bca: suppress"),
            "{label}: a comment survived `strip_comments`"
        );
    }

    /// Asserts `dump` holds the fixture's wide parent with more than
    /// `WIDE_PARENT` children: the export visits a node's children
    /// before the node, so the walk asked its predicates about each.
    fn exports_the_wide_parent(dump: &crate::AstResponse, fixture: &Fixture, label: &str) {
        let mut pending: Vec<&crate::AstNode> = dump.root.iter().collect();
        let mut widest = 0;
        while let Some(node) = pending.pop() {
            if node.r#type == fixture.wide_parent {
                widest = widest.max(node.children.len());
            }
            pending.extend(&node.children);
        }
        assert!(
            widest > WIDE_PARENT,
            "{label}: the widest {} the dump exported has {widest} children",
            fixture.wide_parent
        );
    }

    fn dump_cfg() -> crate::AstCfg {
        crate::AstCfg {
            id: String::new(),
            language: String::new(),
            comment: false,
            span: false,
        }
    }

    fn dumps_by_cursor(ast: &crate::Ast, fixture: &Fixture, label: &str) {
        exports_the_wide_parent(&ast.dump_cursor(dump_cfg()), fixture, label);
    }

    fn dumps_by_index(ast: &crate::Ast, fixture: &Fixture, label: &str) {
        exports_the_wide_parent(&ast.dump_indexed(dump_cfg()), fixture, label);
    }

    type Walk = fn(&crate::Ast, &Fixture, &str);

    /// Each walk that records no siblings, as a run that also witnesses
    /// the walk did its work on the fixture: the marker found, every
    /// comment stripped, the wide parent exported with its children.
    const WALKS: [(&str, Walk); 4] = [
        ("Ast::suppressions", finds_the_marker),
        ("Ast::strip_comments", strips_every_comment),
        ("Ast::dump_cursor", dumps_by_cursor),
        ("Ast::dump_indexed", dumps_by_index),
    ];

    /// The walks that record no siblings (the suppression scan, comment
    /// removal and both dump builders) ask for none through `Ancestors`,
    /// in any language and under narrow parents too (#1601).
    ///
    /// Their chains lend an empty record, so a sibling asked under a
    /// parent wider than `WIDE_PARENT` panics in a debug build, and each
    /// fixture holds one. A narrow ask answers the same as it would
    /// through a lent cache and panics nowhere, so only `sibling_seeks`
    /// sees it; the counter is exact at zero. Neither reading of the
    /// output can tell a walk that asks from one that does not: the
    /// answers agree.
    ///
    /// A predicate that fails this needs a sibling on one of these
    /// walks. The walk must then record each wide node's children as it
    /// pushes them and build its chain with `Ancestors::for_walk`, as
    /// the metric walk does; the dump cannot, since it asks after a
    /// node's children are exported, and needs another way to ask.
    #[test]
    fn the_walks_that_record_no_siblings_ask_for_none() {
        for lang in LANG::into_enum_iter() {
            assert!(
                FIXTURES.iter().any(|fixture| fixture.lang == lang),
                "{} has no fixture: add a row, or these walks are unguarded for it",
                lang.name()
            );
        }
        let mut ran = 0;
        for fixture in FIXTURES.iter().filter(|fixture| fixture.lang.is_enabled()) {
            ran += 1;
            let [head, unit, tail] = fixture.source;
            let source = format!("{head}{}{tail}", unit.repeat(WIDE_PARENT + 1));
            let wide_parent = fixture.wide_parent;
            let label = format!("{} under a wide {wide_parent}", fixture.lang.name());
            let ast = crate::test_support::parse_named(fixture.lang, "fixture", &source);
            let root = ast.root_node();
            assert!(
                root.preorder()
                    .any(|node| node.kind() == wide_parent && node.child_count() > WIDE_PARENT),
                "{label}: the fixture holds no such parent"
            );
            for construct in fixture.constructs {
                assert!(
                    root.preorder().any(|node| node.kind() == *construct),
                    "{label}: the fixture must spell a {construct}"
                );
            }
            // A positive control: the counter counts a seek, so the zeros
            // below are a walk that asked for nothing, not a counter that
            // never moves.
            let first = root.child(0).expect("the fixture has a first node");
            let chain = [root];
            let before = sibling_seeks::observed();
            let _ = Ancestors::known(&chain).siblings_after(&first).next();
            assert_eq!(
                sibling_seeks::observed() - before,
                1,
                "{label}: the control"
            );
            for (walk, run) in WALKS {
                let before = sibling_seeks::observed();
                run(&ast, fixture, &label);
                // expected: 0, none of the predicates these walks call
                // asks for a sibling through `Ancestors`.
                assert_eq!(
                    sibling_seeks::observed() - before,
                    0,
                    "{label}: {walk} asked for a sibling, and its walk records none; \
                     see this test's doc for what the walk must do instead"
                );
            }
        }
        assert!(ran > 0, "no fixture's language is enabled");
    }
}
