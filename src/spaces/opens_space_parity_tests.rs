//! The walk promotes a node to a space ⇔ its language's `opens_space`
//! labels it (#1696, umbrella contract 2). This is the derived parity
//! check between the two halves of the space tree: the walker's
//! promotion predicate (`promotes_to_func_space_with_code`) and the
//! getter's label (`get_space_kind_with_code`), which the migrated
//! languages both read from `roles/<lang>.rs`. It compares promotion
//! against the *space mapping*, never against `Closure` recognition —
//! a lambda that is a closure and opens nothing is the case the two
//! must be allowed to differ on.
//!
//! The metric walk is the subject: each fixture is run through
//! `analyze`, every opened space is recorded as `(kind, start, end)`,
//! and the same fixture is then walked node by node asking
//! `opens_space` of each; the two sets must be equal. Comparing the
//! real walk — not a replica of its truncate/push rule — is what makes
//! this a check on `metrics_inner` rather than on the test (lesson 82).

use big_code_analysis_ast::test_support::for_each_node_with_chain;
use big_code_analysis_ast::{Ancestors, Node, SpaceKind};

use crate::test_support::assert_fixtures_present;
use crate::{FuncSpace, LANG, MetricsOptions, Source, analyze};

/// `(start line, end line, kind)` of every space below the root; the
/// kind rendered, since `SpaceKind` has no ordering of its own.
fn opened(space: &FuncSpace, out: &mut Vec<(usize, usize, String)>) {
    for child in &space.spaces {
        out.push((
            child.start_line,
            child.end_line,
            format!("{:?}", child.kind),
        ));
        opened(child, out);
    }
}

type OpensSpace = fn(&Node<'_>, &[u8], Ancestors<'_, '_>) -> SpaceKind;
type NodeWalk = fn(&[u8], &mut dyn FnMut(&Node<'_>, &[Node<'_>]));

/// One language's fixture, its node-walk, and its table's fold.
struct Row {
    lang: LANG,
    source: &'static [u8],
    walk: NodeWalk,
    opens_space: OpensSpace,
}

/// Every migrated language, one row each; a row's fixture must open at
/// least one nested space, or the check is vacuous. The walk is named
/// per row because `for_each_node_with_chain` is generic over the
/// language tag.
const ROWS: &[Row] = &[
    // A lambda (closure, no space), an anonymous class (gated on its
    // body) beside a plain `new`, `static { }` (space, not callable), a
    // record's compact constructor, an enum with a body, an interface
    // and an annotation type.
    #[cfg(feature = "java")]
    Row {
        lang: LANG::Java,
        source: b"enum E { A; void em() { } }\nrecord R(int a) { R { } }\ninterface I { default void d() { } }\n@interface An { int v(); }\nclass C {\n  static { int z = 1; }\n  C() { }\n  void m() { Runnable r = () -> { int w = 0; }; Object o = new Object() { public String toString() { return \"\"; } }; Object p = new Object(); }\n}\n",
        walk: |source, check| {
            for_each_node_with_chain::<big_code_analysis_ast::JavaCode>(source, check);
        },
        opens_space: big_code_analysis_ast::roles::java::opens_space,
    },
    // Every function form, bound and positional, a class in both
    // spellings and ES2022 `static { … }`.
    #[cfg(feature = "javascript")]
    Row {
        lang: LANG::Javascript,
        source: JS_SOURCE,
        walk: |source, check| {
            for_each_node_with_chain::<big_code_analysis_ast::JavascriptCode>(source, check);
        },
        opens_space: big_code_analysis_ast::roles::javascript::opens_space,
    },
    #[cfg(feature = "mozjs")]
    Row {
        lang: LANG::Mozjs,
        source: JS_SOURCE,
        walk: |source, check| {
            for_each_node_with_chain::<big_code_analysis_ast::MozjsCode>(source, check);
        },
        opens_space: big_code_analysis_ast::roles::mozjs::opens_space,
    },
    // The same plus an abstract class, an interface and a namespace
    // (which opens nothing).
    #[cfg(feature = "typescript")]
    Row {
        lang: LANG::Typescript,
        source: TS_SOURCE,
        walk: |source, check| {
            for_each_node_with_chain::<big_code_analysis_ast::TypescriptCode>(source, check);
        },
        opens_space: big_code_analysis_ast::roles::typescript::opens_space,
    },
    #[cfg(feature = "typescript")]
    Row {
        lang: LANG::Tsx,
        source: TS_SOURCE,
        walk: |source, check| {
            for_each_node_with_chain::<big_code_analysis_ast::TsxCode>(source, check);
        },
        opens_space: big_code_analysis_ast::roles::tsx::opens_space,
    },
];

#[cfg(any(feature = "javascript", feature = "mozjs"))]
const JS_SOURCE: &[u8] = b"function f(a) { const g = (b) => b; return g(a); }\nfunction* h(n) { yield n; }\nconst i = function () {};\nclass K { static { let z = 1; } m() { run(x => x); } fld = () => 1; }\nconst L = class {};\n(function () {})();\n";

#[cfg(feature = "typescript")]
const TS_SOURCE: &[u8] = b"function f(a: number): number { const g = (b: number) => b; return g(a); }\nabstract class A { abstract am(): void; m(): void { run((x: number) => x); } fld = () => 1; static { } }\ninterface I { m(): void; }\nconst L = class {};\nnamespace NS { export function nf() {} }\n";

// The gate is the union of the rows' features; `assert_fixtures_present`
// fails rather than skips when a build enables none of them.
#[cfg(any(
    feature = "java",
    feature = "javascript",
    feature = "mozjs",
    feature = "typescript"
))]
#[test]
fn the_walk_promotes_exactly_what_opens_space_labels() {
    assert_fixtures_present(ROWS);
    for row in ROWS {
        let tree = analyze(Source::new(row.lang, row.source), MetricsOptions::default())
            .unwrap_or_else(|e| panic!("{:?}: analyze failed: {e}", row.lang));
        let mut walked = Vec::new();
        opened(&tree, &mut walked);
        walked.sort_unstable();

        let mut labelled = Vec::new();
        (row.walk)(row.source, &mut |node, chain| {
            if chain.is_empty() {
                return; // the root's Unit is the synthetic top-level space
            }
            let kind = (row.opens_space)(node, row.source, Ancestors::known(chain));
            if kind != SpaceKind::Unknown {
                labelled.push((node.start_row() + 1, node.end_line(), format!("{kind:?}")));
            }
        });
        labelled.sort_unstable();

        assert!(
            !walked.is_empty(),
            "{:?}: the fixture opens no nested space, so the check is vacuous",
            row.lang
        );
        assert_eq!(walked, labelled, "{:?}: promotion vs opens_space", row.lang);
    }
}
