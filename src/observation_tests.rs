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
/// a memoised walk from the quadratic one. Dropping `.with_folds` from
/// `metrics_inner` or `ops_inner` takes that seam's row to `width`; the
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

/// The word slot on the walk's `FoldCache` (#1697): the real
/// `Ast::metrics` walk reads an Elixir `call`'s target text once for the
/// visited node, however many of promotion, cognitive, cyclomatic, ABC,
/// nexits, NPM and NPA ask — the slot answers all but the first. The
/// output cannot show it (every read answers alike); only the read
/// counter does, and the AST crate's own tests drive a replica walker,
/// not this one. What this pins is the invariant the slot rests on:
/// `compute_per_node` runs the metrics back to back on one node, and
/// every ask about *another* node is one the design priced — so a
/// metric reorder that let a later metric re-ask after an eviction, or
/// a new consumer reading an enclosing call through the cached spelling
/// where it evicts, raises the count.
///
/// The budget, derived: one read per `call` at promotion (10), plus the
/// asks about other nodes that cost a miss each — the `def`'s quote
/// fold reading the enclosing `defmodule` once per walk (1); NPM's
/// uncached read of the module's two `def`-family statements (2); NPA's
/// `struct_macro` asked of the same two statements through the slot,
/// evicting the module (2); the `else:` section's read of its `if`
/// from the pair, after `throw` was visited in between (1);
/// and the `CaseArm` gate's uncached read of the enclosing `case` for
/// its one arm, by cyclomatic and ABC (2); the `defmodule` itself is
/// never re-asked after NPA, so its eviction costs nothing. 18 reads.
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
        call_nodes + 1 + 2 + 2 + 1 + 2,
        "one read per call, plus the priced asks about other nodes"
    );
}

/// The Tcl twin: the real metric walk reads a `command`'s leading word
/// once for the visited node, however many of cognitive, cyclomatic,
/// ABC and nexits ask (#1697). The asks about other nodes are all
/// cyclomatic's `CaseArm` gate reading a candidate arm's grandparent
/// through the uncached helper, for every command three levels under
/// another: the arm list's command reads `switch`, the two `return`s
/// read their arm's pattern word `x` / `y`, and the `for` body's three
/// commands (`$i < 3`, `incr`, `puts`) read `for` — six reads, five of
/// them rejected candidates. ABC does not read the arm row.
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
