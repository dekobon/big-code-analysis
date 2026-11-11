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
/// `metrics_inner` or `ops_inner` with `Ancestors::checked` alone, the
/// cache unlent, takes that seam's row to `width`; the
/// dump and count seams are pinned the same way beside their walks in
/// `big-code-analysis-ast`, and listed here so the four cannot drift
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
    // walk that stopped lending it (`with_siblings` dropped from
    // `metrics_inner`) seeks instead, answers the same, and resolves no
    // sibling from a node, so only `sibling_cache_reads` sees it.
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
/// the same, and only the fold's stored-state count tells a memoised walk
/// from the quadratic climb. A seam that lent no cache would fold
/// nothing and climb instead, reading 0 here; the suppression seam is
/// the one whose lend #1767 added.
#[cfg(feature = "javascript")]
#[test]
fn every_seam_folds_the_js_binding_question() {
    use crate::node::fold_steps;

    type Seam<'a> = &'a dyn Fn(&crate::Ast);

    let depth = 200;
    let source = format!(
        "x = {}0{};\n",
        "[() => 1,\n".repeat(depth),
        "]\n".repeat(depth)
    );
    let ast = crate::test_support::parse_named(crate::LANG::Javascript, "a.js", &source);
    let nodes = ast.root_node().preorder().count();
    let seams: [(&str, Seam); 4] = [
        ("Ast::metrics", &|ast| {
            ast.metrics(crate::MetricsOptions::default())
                .expect("the walk must yield a top-level space");
        }),
        ("Ast::ops", &|ast| {
            ast.ops().expect("the walk must yield a top-level space");
        }),
        ("Ast::functions", &|ast| {
            assert_eq!(ast.functions().len(), depth, "every arrow is a function");
        }),
        ("Ast::suppressions", &|ast| {
            assert!(
                ast.suppressions().is_empty(),
                "the fixture carries no marker"
            );
        }),
    ];
    for (name, run) in seams {
        let before = fold_steps::observed();
        run(&ast);
        let folded = fold_steps::observed() - before;
        assert!(
            (depth..=4 * nodes).contains(&folded),
            "{name} folded {folded} ancestor states over {nodes} nodes: it must \
             lend its walk a FoldCache, and fold each ancestor about once"
        );
    }
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
