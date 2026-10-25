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
    // A closure opens a space in Rust.
    #[cfg(feature = "rust")]
    Row {
        lang: LANG::Rust,
        source: b"struct S;\ntrait T { fn tm(&self); }\nimpl T for S { fn tm(&self) { let c = |x: i32| x; c(1); } }\nfn f() {}\n",
        walk: |source, check| {
            for_each_node_with_chain::<big_code_analysis_ast::RustCode>(source, check);
        },
        opens_space: big_code_analysis_ast::roles::rust::opens_space,
    },
    #[cfg(feature = "go")]
    Row {
        lang: LANG::Go,
        source: b"package p\ntype S struct{}\nfunc (s *S) M() { f := func(x int) int { return x }; f(1) }\nfunc top() {}\n",
        walk: |source, check| {
            for_each_node_with_chain::<big_code_analysis_ast::GoCode>(source, check);
        },
        opens_space: big_code_analysis_ast::roles::go::opens_space,
    },
    #[cfg(feature = "lua")]
    Row {
        lang: LANG::Lua,
        source: b"function M.f(a) return a end\nlocal function g() local c = function(x) return x end; return c(1) end\n",
        walk: |source, check| {
            for_each_node_with_chain::<big_code_analysis_ast::LuaCode>(source, check);
        },
        opens_space: big_code_analysis_ast::roles::lua::opens_space,
    },
    #[cfg(feature = "bash")]
    Row {
        lang: LANG::Bash,
        source: b"f() { echo 1; }\nfunction g { f; }\n",
        walk: |source, check| {
            for_each_node_with_chain::<big_code_analysis_ast::BashCode>(source, check);
        },
        opens_space: big_code_analysis_ast::roles::bash::opens_space,
    },
    #[cfg(feature = "tcl")]
    Row {
        lang: LANG::Tcl,
        source: b"proc f {a} { return $a }\nproc g {} { f 1 }\n",
        walk: |source, check| {
            for_each_node_with_chain::<big_code_analysis_ast::TclCode>(source, check);
        },
        opens_space: big_code_analysis_ast::roles::tcl::opens_space,
    },
    #[cfg(feature = "irules")]
    Row {
        lang: LANG::Irules,
        source: b"when HTTP_REQUEST {\n  f 1\n}\nproc f {a} { return $a }\n",
        walk: |source, check| {
            for_each_node_with_chain::<big_code_analysis_ast::IrulesCode>(source, check);
        },
        opens_space: big_code_analysis_ast::roles::irules::opens_space,
    },
    #[cfg(feature = "perl")]
    Row {
        lang: LANG::Perl,
        source: b"package P;\nsub f { my $c = sub { 1 }; $c->(); }\nmethod m { 1 }\n1;\n",
        walk: |source, check| {
            for_each_node_with_chain::<big_code_analysis_ast::PerlCode>(source, check);
        },
        opens_space: big_code_analysis_ast::roles::perl::opens_space,
    },
    #[cfg(feature = "php")]
    Row {
        lang: LANG::Php,
        source: b"<?php\ninterface I { public function m(); }\ntrait T { function tm() {} }\nenum E { case A; function em() {} }\nclass C { function m() { $c = function ($x) { return $x; }; $a = fn($y) => $y; $o = new class { function z() {} }; } }\nfunction top() {}\n",
        walk: |source, check| {
            for_each_node_with_chain::<big_code_analysis_ast::PhpCode>(source, check);
        },
        opens_space: big_code_analysis_ast::roles::php::opens_space,
    },
    #[cfg(feature = "c")]
    Row {
        lang: LANG::C,
        source: b"static int helper(int x) { return x; }\nint main(void) { return helper(1); }\n",
        walk: |source, check| {
            for_each_node_with_chain::<big_code_analysis_ast::CCode>(source, check);
        },
        opens_space: big_code_analysis_ast::roles::c::opens_space,
    },
    // Bodied and bodiless specifiers (#1555), a lambda (closure, no
    // space), a namespace; Mozcpp reads the same source.
    #[cfg(feature = "cpp")]
    Row {
        lang: LANG::Cpp,
        source: CPP_SOURCE,
        walk: |source, check| {
            for_each_node_with_chain::<big_code_analysis_ast::CppCode>(source, check);
        },
        opens_space: big_code_analysis_ast::roles::cpp::opens_space,
    },
    #[cfg(feature = "mozcpp")]
    Row {
        lang: LANG::Mozcpp,
        source: CPP_SOURCE,
        walk: |source, check| {
            for_each_node_with_chain::<big_code_analysis_ast::MozcppCode>(source, check);
        },
        opens_space: big_code_analysis_ast::roles::mozcpp::opens_space,
    },
    #[cfg(feature = "objc")]
    Row {
        lang: LANG::Objc,
        source: b"@interface Foo : NSObject\n- (void)m:(int)x;\n@end\n@implementation Foo\n- (void)m:(int)x { void (^blk)(int) = ^(int y) { }; blk(1); }\nstatic void helper(void) {}\n@end\n@protocol P\n- (void)pm;\n@end\nint cfun(int a) { return a; }\n",
        walk: |source, check| {
            for_each_node_with_chain::<big_code_analysis_ast::ObjcCode>(source, check);
        },
        opens_space: big_code_analysis_ast::roles::objc::opens_space,
    },
    // The accessor-less indexer and property open a space, the bodied
    // ones through their accessors (#464, #472); a lambda and an
    // anonymous method open one each.
    #[cfg(feature = "csharp")]
    Row {
        lang: LANG::Csharp,
        source: b"namespace N { interface I { int P { get; set; } } class C { public int P { get; set; } public int W => 1; public int this[int i] => i; public int this[string k] { get { return 1; } } public void M() { int L(int z) { return z; } System.Func<int, int> f = y => y + 1; System.Action a = delegate { }; L(1); } } }\n",
        walk: |source, check| {
            for_each_node_with_chain::<big_code_analysis_ast::CsharpCode>(source, check);
        },
        opens_space: big_code_analysis_ast::roles::csharp::opens_space,
    },
    // Kotlin: accessors and `init` open a space without being callable;
    // a lambda and an anonymous `fun` open none; one `class_declaration`
    // kind is a class or an interface by its keyword.
    #[cfg(feature = "kotlin")]
    Row {
        lang: LANG::Kotlin,
        source: b"interface I {\n  fun m()\n}\nobject O {\n  val o = 1\n}\nclass C : I {\n  var q: Int = 1\n    get() = field\n    set(v) { field = v }\n  init { println(1) }\n  constructor(x: Int) : this()\n  override fun m() {\n    val l = { x: Int -> x }\n    val a = fun(y: Int) = y\n    val o = object : I {\n      override fun m() {}\n    }\n    l(1); a(2); o.m()\n  }\n}\n",
        walk: |source, check| {
            for_each_node_with_chain::<big_code_analysis_ast::KotlinCode>(source, check);
        },
        opens_space: big_code_analysis_ast::roles::kotlin::opens_space,
    },
    // Groovy: `static { }` opens a space, a closure none.
    #[cfg(feature = "groovy")]
    Row {
        lang: LANG::Groovy,
        source: b"interface I { void m() }\ntrait Tr { def tm() {} }\nclass C implements I {\n  static { int s = 1 }\n  C() { }\n  void m() { def c = { x -> x }; c(1) }\n}\n",
        walk: |source, check| {
            for_each_node_with_chain::<big_code_analysis_ast::GroovyCode>(source, check);
        },
        opens_space: big_code_analysis_ast::roles::groovy::opens_space,
    },
    // Ruby: both block spellings as iterator blocks (spaces) and as
    // stabby-lambda bodies (not).
    #[cfg(feature = "ruby")]
    Row {
        lang: LANG::Ruby,
        source: b"module M\n  class C\n    def m; l = ->(y) { y }; l2 = -> do 1 end; [1].each { |i| i }; [1].each do |i| i end; l.call(1); end\n    def self.sm; end\n    class << self; def eig; end; end\n  end\nend\n",
        walk: |source, check| {
            for_each_node_with_chain::<big_code_analysis_ast::RubyCode>(source, check);
        },
        opens_space: big_code_analysis_ast::roles::ruby::opens_space,
    },
];

#[cfg(any(feature = "cpp", feature = "mozcpp"))]
const CPP_SOURCE: &[u8] = b"namespace ns { int x; }\nstruct S;\nstruct S2 { int a; void m(); };\nclass C { int f; void m() { auto l = [](int x) { return x; }; l(1); } };\nvoid g(struct S *q) {}\nint main() { return 0; }\n";

#[cfg(any(feature = "javascript", feature = "mozjs"))]
const JS_SOURCE: &[u8] = b"function f(a) { const g = (b) => b; return g(a); }\nfunction* h(n) { yield n; }\nconst i = function () {};\nclass K { static { let z = 1; } m() { run(x => x); } fld = () => 1; }\nconst L = class {};\n(function () {})();\n";

#[cfg(feature = "typescript")]
const TS_SOURCE: &[u8] = b"function f(a: number): number { const g = (b: number) => b; return g(a); }\nabstract class A { abstract am(): void; m(): void { run((x: number) => x); } fld = () => 1; static { } }\ninterface I { m(): void; }\nconst L = class {};\nnamespace NS { export function nf() {} }\n";

// The gate is the union of the rows' features; `assert_fixtures_present`
// fails rather than skips when a build enables none of them.
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "cpp",
    feature = "csharp",
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
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
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
