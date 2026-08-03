//! Cross-language parity test for the **space tree** produced by the
//! two AST walks.
//!
//! `spaces::compute::metrics_inner` (behind [`analyze`]) and
//! `ops::ops_inner` (behind [`Ast::ops`]) are separate walks that must
//! open a function space on exactly the same nodes and label each with
//! the same [`SpaceKind`]. Nothing forces them to: each carries its own
//! copy of the promote-and-classify decision, and a divergence is
//! invisible from either side alone — `bca ops` simply reports fewer
//! spaces, with no error and no wrong metric value anywhere.
//!
//! That is exactly how #1130 survived: `ops_inner` opened on the
//! byte-less `is_func || is_func_space`, which cannot see Elixir's
//! macro-shaped `defmodule` / `def` declarations (they are plain `Call`
//! nodes distinguished only by their target identifier text, #275), so
//! `bca ops` returned a bare file-level space for every Elixir input
//! while `bca metrics` returned a full module/function tree.
//!
//! The fixture table is an exhaustive `match` on [`LANG`], so adding a
//! language variant fails to compile until a fixture is supplied — the
//! `add-lang` workflow trips over this test rather than discovering the
//! divergence in the field.

use big_code_analysis::{Ast, FuncSpace, LANG, MetricsOptions, Ops, Source, SpaceKind, analyze};

/// One fixture per language, chosen to open at least one *nested*
/// space: a walk that opens no space below the file root would agree
/// with any other walk vacuously.
///
/// Returns `(source, extension)`. The extension only reaches the space
/// name of the file-level `Unit`, which both walks take from
/// `Source::name` rather than from the AST, so it is cosmetic — but it
/// keeps a failure message readable.
///
/// Shared with [`super::space_span_containment`] and
/// [`super::functions_metrics_parity`], which assert different
/// properties over the same trees: one exhaustive `LANG` table serves
/// all three, so a new language cannot be added to one check and missed
/// by the others.
pub(super) fn fixture(lang: LANG) -> (&'static str, &'static str) {
    // Exhaustive per-language dispatch table: one arm per LANG variant
    // is the point of this function, and splitting it would break the
    // compile-time completeness check the test relies on. The repo's own
    // `.bcaignore` excludes `./tests/**`, so this marker is for the
    // per-edit `bca check` hook rather than for the self-scan gate.
    // bca: suppress(cyclomatic)
    match lang {
        // The generator is deliberate: `function* h()` opened a
        // `SpaceKind::Function` space named `h` while `is_func` said
        // false, so `functions()` did not report it — a live violation of
        // `functions_metrics_parity`'s coverage claim that no fixture
        // reached. #1186 fixed the classification; this makes the fixture
        // able to catch a regression of it.
        LANG::Javascript | LANG::Mozjs => (
            "function f(a) {\n  const g = (b) => b * 2;\n  return g(a) + 1;\n}\n\
             function* h(n) {\n  yield n;\n}\n",
            "js",
        ),
        LANG::Typescript => (
            "function f(a: number): number {\n  const g = (b: number) => b * 2;\n  \
             return g(a) + 1;\n}\n",
            "ts",
        ),
        LANG::Tsx => (
            "function f(a: number): number {\n  const g = (b: number) => b * 2;\n  \
             return g(a) + 1;\n}\n",
            "tsx",
        ),
        LANG::Java => (
            "import java.util.function.IntUnaryOperator;\n\nclass Parity {\n  \
             int f(int a) {\n    IntUnaryOperator g = b -> b * 2;\n    \
             return g.applyAsInt(a) + 1;\n  }\n}\n",
            "java",
        ),
        LANG::Go => (
            "package p\n\nfunc f(a int) int {\n  g := func(b int) int { return b * 2 }\n  \
             return g(a) + 1\n}\n",
            "go",
        ),
        LANG::Kotlin => (
            "fun f(a: Int): Int {\n  val g = { b: Int -> b * 2 }\n  return g(a) + 1\n}\n",
            "kt",
        ),
        LANG::Lua => (
            "function f(a)\n  local g = function(b) return b * 2 end\n  return g(a) + 1\nend\n",
            "lua",
        ),
        LANG::Rust => (
            "fn f(a: i32) -> i32 {\n    let g = |b: i32| b * 2;\n    g(a) + 1\n}\n",
            "rs",
        ),
        LANG::Tcl => ("proc f {a} {\n  return $a\n}\n", "tcl"),
        // `when <EVENT> { … }` is the dominant real iRules shape and a
        // separate `Checker` arm from `proc`; exercise both.
        LANG::Irules => (
            "proc f { a } {\n  return $a\n}\n\nwhen HTTP_REQUEST {\n  log local0. \"hi\"\n}\n",
            "irule",
        ),
        LANG::C => ("int f(int a) {\n  return a + 1;\n}\n", "c"),
        LANG::Cpp | LANG::Mozcpp => (
            "int f(int a) {\n  auto g = [](int b) { return b * 2; };\n  return g(a) + 1;\n}\n",
            "cpp",
        ),
        // A real Objective-C method, not the plain C function the
        // grammar would also accept — the `MethodDefinition` arm is
        // Objc-specific and nothing else here reaches it.
        LANG::Objc => (
            "@implementation Parity\n- (int)f:(int)a {\n    return a + 1;\n}\n@end\n",
            "m",
        ),
        // An expression-bodied property alongside an ordinary method:
        // per `.claude/rules/grammar-dispatch.md` §6, `is_func`,
        // `is_func_space`, and `get_space_kind` must stay gated by the
        // same child-presence predicate for these, and a mismatch shows
        // up here as an ops/metrics divergence.
        LANG::Csharp => (
            "class Parity {\n  int _w;\n  int W => _w;\n  int F(int a) {\n    \
             return a + 1;\n  }\n}\n",
            "cs",
        ),
        // The reason this file exists: every space below the root is a
        // `Call` node the byte-less predicates cannot recognise.
        LANG::Elixir => (
            "defmodule Foo do\n  def bar(x) do\n    x + 1\n  end\nend\n",
            "ex",
        ),
        LANG::Python => (
            "def f(a):\n    def g(b):\n        return b * 2\n    return g(a) + 1\n",
            "py",
        ),
        LANG::Bash => ("f() {\n  echo \"$1\"\n}\n", "sh"),
        LANG::Perl => (
            "sub f {\n    my ($a) = @_;\n    my $g = sub { return $_[0] * 2; };\n    \
             return $g->($a) + 1;\n}\n",
            "pl",
        ),
        LANG::Php => (
            "<?php\nfunction f($a) {\n    $g = function ($b) { return $b * 2; };\n    \
             return $g($a) + 1;\n}\n",
            "php",
        ),
        LANG::Ruby => (
            "def f(a)\n  g = lambda { |b| b * 2 }\n  g.call(a) + 1\nend\n",
            "rb",
        ),
        LANG::Groovy => (
            "def f(int a) {\n  def g = { int b -> b * 2 }\n  return g(a) + 1\n}\n",
            "groovy",
        ),
        // `Ccomment` and `Preproc` are internal C-family helper grammars
        // with no function-space concept at all: both walks open the file
        // root and nothing else. Included so the `match` stays exhaustive
        // — the parity claim still holds, it is just a one-node tree.
        LANG::Ccomment => ("/* a comment */\n", "c"),
        LANG::Preproc => ("#define A 1\n", "h"),
    }
}

/// The fields the two walks are supposed to agree on.
///
/// One trait rather than two renderers: a second renderer would be free
/// to drift from the first — printing a field one side does not — and
/// two renderers that disagree is the exact failure mode this file
/// exists to catch, one level up.
pub(super) trait SpaceTree: Sized {
    fn describe(&self) -> (Option<&str>, SpaceKind, usize, usize);
    fn children(&self) -> &[Self];
}

impl SpaceTree for FuncSpace {
    fn describe(&self) -> (Option<&str>, SpaceKind, usize, usize) {
        (
            self.name.as_deref(),
            self.kind,
            self.start_line,
            self.end_line,
        )
    }
    fn children(&self) -> &[Self] {
        &self.spaces
    }
}

impl SpaceTree for Ops {
    fn describe(&self) -> (Option<&str>, SpaceKind, usize, usize) {
        (
            self.name.as_deref(),
            self.kind,
            self.start_line,
            self.end_line,
        )
    }
    fn children(&self) -> &[Self] {
        &self.spaces
    }
}

/// Renders a space tree as one line per space, depth encoded as
/// indentation.
///
/// Comparing rendered trees rather than walking two structures in
/// lockstep means a mismatch anywhere prints both trees in full, so the
/// reader sees *which* space diverged and what its neighbours were —
/// the failure mode here is a missing subtree, which a pairwise
/// recursion reports as a confusing count mismatch at the parent.
fn render<T: SpaceTree>(node: &T, depth: usize, out: &mut String) {
    use std::fmt::Write as _;

    let (name, kind, start, end) = node.describe();
    let _ = writeln!(
        out,
        "{:indent$}{kind:?} {name:?} lines {start}..{end}",
        "",
        indent = depth * 2,
    );
    for child in node.children() {
        render(child, depth + 1, out);
    }
}

fn rendered<T: SpaceTree>(node: &T) -> String {
    let mut out = String::new();
    render(node, 0, &mut out);
    out
}

/// First line that differs between the two renderings, for a failure
/// message that names the diverging space instead of dumping a diff the
/// reader has to align by eye.
fn first_divergence(metrics: &str, ops: &str) -> String {
    let mut metrics_lines = metrics.lines();
    let mut ops_lines = ops.lines();
    loop {
        match (metrics_lines.next(), ops_lines.next()) {
            (Some(m), Some(o)) if m == o => {}
            (Some(m), Some(o)) => return format!("metrics has `{m}`, ops has `{o}`"),
            (Some(m), None) => return format!("metrics has `{m}`, ops has no space there"),
            (None, Some(o)) => return format!("ops has `{o}`, metrics has no space there"),
            (None, None) => return "trees are identical".to_owned(),
        }
    }
}

#[test]
fn ops_and_metrics_agree_on_the_space_tree() {
    let mut checked = 0;

    for lang in LANG::into_enum_iter() {
        if !lang.is_enabled() {
            continue;
        }
        checked += 1;

        let (source, ext) = fixture(lang);
        let name = format!("parity.{ext}");

        let space = analyze(
            Source::new(lang, source.as_bytes()).with_name(Some(name.clone())),
            MetricsOptions::default(),
        )
        .unwrap_or_else(|e| panic!("{lang:?}: analyze failed: {e}"));
        let ops = Ast::parse(Source::new(lang, source.as_bytes()).with_name(Some(name)))
            .unwrap_or_else(|e| panic!("{lang:?}: parse failed: {e}"))
            .ops()
            .unwrap_or_else(|e| panic!("{lang:?}: ops failed: {e}"));

        let from_metrics = rendered(&space);
        let from_ops = rendered(&ops);

        assert_eq!(
            from_metrics,
            from_ops,
            "{lang:?}: ops and metrics space trees diverge — {}\n\
             metrics tree:\n{from_metrics}\nops tree:\n{from_ops}",
            first_divergence(&from_metrics, &from_ops),
        );

        // A fixture whose only space is the file root would satisfy the
        // assertion above no matter how badly the two walks disagreed
        // about nested spaces — which is precisely the #1130 shape. The
        // two helper grammars genuinely have no nested spaces to open.
        if !matches!(lang, LANG::Ccomment | LANG::Preproc) {
            assert!(
                !space.spaces.is_empty(),
                "{lang:?}: fixture opens no nested space, so the parity \
                 assertion above cannot detect a divergence",
            );
        }
    }

    // Every language is feature-gated, so a build with none enabled
    // leaves a zero-iteration loop and a test that reports green while
    // asserting nothing.
    assert!(
        checked > 0,
        "at least one language feature must be enabled for this test to mean anything"
    );
}
