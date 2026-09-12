#![allow(
    clippy::cast_precision_loss,
    clippy::float_cmp,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]

//! Cross-language parity test for **cognitive complexity** on a 2-arm
//! conditional with a wildcard / default arm.
//!
//! Per lesson 11 in `docs/development/lessons_learned.md`, the same
//! logical construct must produce the same metric value across every
//! language we support — per-language snapshot suites cannot detect
//! disagreement between languages. This is the cognitive-complexity
//! companion to the standard-CCN parity tests in
//! `tests/parity/cyclomatic_cross_language_parity.rs`.
//!
//! The fixture is the canonical lesson-11 trigger: a function whose
//! body is a single switch/match with one explicit arm plus a
//! wildcard/default arm. Bug #106 (Rust over-counting wildcard
//! `_ =>` in CCN) is the historical precedent on the cyclomatic
//! side; this test ensures the cognitive side never quietly drifts
//! along the same axis.
//!
//! The fixture table is an exhaustive `match` on [`LANG`], so adding a
//! language variant fails to compile until someone decides whether its
//! grammar models a switch-like construct; a language that does not says
//! so with `None`, and the reason lives at the arm. Before #1281 the
//! list was hand-maintained and had fallen behind the roster — Ruby and
//! Elixir were absent, and the row labelled `c` parsed `LANG::Cpp`.
//!
//! The test uses `cognitive_max()` (space-stacking-independent) so
//! Java's mandatory wrapping class does not skew the comparison —
//! no per-language offset is required.

use big_code_analysis::{LANG, MetricsOptions, Source, analyze};

/// Cognitive max for the single function in `source`.
fn cognitive_max(lang: LANG, source: &str, ext: &str) -> f64 {
    let name = format!("parity.{ext}");
    let space = analyze(
        Source::new(lang, source.as_bytes()).with_name(Some(name)),
        MetricsOptions::default(),
    )
    .unwrap_or_else(|e| panic!("{lang:?}: analyze failed: {e}"));
    space.metrics.cognitive.cognitive_max() as f64
}

/// Returns `(source, extension)` for a language whose grammar models a
/// switch-like construct with a wildcard / `default` arm, or `None` for
/// one that does not.
///
/// Every `Some` row spells the same shape: a function whose whole body
/// is a two-arm switch — one explicit arm plus a fallback.
fn fixture(lang: LANG) -> Option<(&'static str, &'static str)> {
    // Exhaustive per-language dispatch table: one arm per LANG variant
    // is the point of this function, so a new language cannot be added
    // without deciding whether it has a switch-like construct. The
    // repo's own `.bcaignore` excludes `./tests/**`, so this marker is
    // for the per-edit `bca check` hook rather than for the self-scan
    // gate.
    // bca: suppress(cyclomatic)
    let row = match lang {
        LANG::Rust => (
            "fn f(x: u8) -> &'static str {\n    match x {\n        1 => \"one\",\n        \
             _ => \"other\",\n    }\n}\n",
            "rs",
        ),
        // One arm for the whole C family: the fixture is plain C, which
        // all four grammars accept unchanged. Objective-C reaches it
        // through a free C `function_definition`, valid in a `.m` file.
        LANG::C | LANG::Cpp | LANG::Mozcpp | LANG::Objc => (
            "void f(int x) {\n    switch (x) {\n        case 1: break;\n        \
             default: break;\n    }\n}\n",
            "c",
        ),
        LANG::Java => (
            "class Parity {\n    static void f(int x) {\n        switch (x) {\n            \
             case 1: break;\n            default: break;\n        }\n    }\n}\n",
            "java",
        ),
        LANG::Csharp => (
            "class Parity {\n    static void F(int x) {\n        switch (x) {\n            \
             case 1: break;\n            default: break;\n        }\n    }\n}\n",
            "cs",
        ),
        LANG::Javascript | LANG::Mozjs => (
            "function f(x) {\n    switch (x) {\n        case 1: break;\n        \
             default: break;\n    }\n}\n",
            "js",
        ),
        LANG::Typescript => (
            "function f(x: number) {\n    switch (x) {\n        case 1: break;\n        \
             default: break;\n    }\n}\n",
            "ts",
        ),
        LANG::Tsx => (
            "function f(x: number) {\n    switch (x) {\n        case 1: break;\n        \
             default: break;\n    }\n}\n",
            "tsx",
        ),
        LANG::Php => (
            "<?php\nfunction f($x) {\n    switch ($x) {\n        case 1: break;\n        \
             default: break;\n    }\n}\n",
            "php",
        ),
        LANG::Groovy => (
            "def f(x) {\n    switch (x) {\n        case 1: break\n        default: break\n    }\n}\n",
            "groovy",
        ),
        // Kotlin spells the same construct `when`.
        LANG::Kotlin => (
            "fun f(x: Int): String {\n    return when (x) {\n        1 -> \"one\"\n        \
             else -> \"other\"\n    }\n}\n",
            "kt",
        ),
        LANG::Go => (
            "package p\nfunc f(x int) string {\n    switch x {\n    case 1:\n        \
             return \"one\"\n    default:\n        return \"other\"\n    }\n}\n",
            "go",
        ),
        // Python's structural-pattern `match`; `case _` is the wildcard.
        LANG::Python => (
            "def f(x):\n    match x:\n        case 1:\n            return 'one'\n        \
             case _:\n            return 'other'\n",
            "py",
        ),
        // Ruby spells the construct `case`/`when`, with `else` as the
        // fallback arm.
        LANG::Ruby => (
            "def f(x)\n  case x\n  when 1 then \"one\"\n  else \"other\"\n  end\nend\n",
            "rb",
        ),
        // Elixir's `case` with a bare `_ ->` catch-all (#1272). `def`
        // must live inside a `defmodule`, but `cognitive_max()` reads
        // the function space, so the module adds no offset.
        LANG::Elixir => (
            "defmodule Parity do\n  def f(x) do\n    case x do\n      1 -> :one\n      \
             _ -> :other\n    end\n  end\nend\n",
            "ex",
        ),
        LANG::Bash => (
            "f() {\n  case \"$1\" in\n    one) echo one ;;\n    *) echo other ;;\n  esac\n}\n",
            "sh",
        ),
        // Tcl spells the construct as a generic `switch` command and
        // iRules as a dedicated node (#467); both contribute one
        // decision point with the `default` arm free, so one fixture
        // text serves both.
        LANG::Tcl => (
            "proc f {x} {\n    switch $x {\n        1 { return one }\n        \
             default { return other }\n    }\n}\n",
            "tcl",
        ),
        LANG::Irules => (
            "proc f {x} {\n    switch $x {\n        1 { return one }\n        \
             default { return other }\n    }\n}\n",
            "irule",
        ),
        // No switch-like construct to score. Lua has none at all — the
        // idiomatic form is an `if`/`elseif` chain. Perl's `given`/`when`
        // was always experimental and was removed from the language in
        // 5.42; the pinned grammar does not model it (verified: the
        // construct parses to ERROR nodes), so a fixture using it would
        // pin the parse failure rather than the metric. The two
        // C-family helper grammars parse fragments and have no
        // statements at all.
        LANG::Lua | LANG::Perl | LANG::Ccomment | LANG::Preproc => return None,
    };
    Some(row)
}

#[test]
fn two_arm_wildcard_switch_cognitive_parity() {
    // expected: a two-arm switch/match with one explicit arm plus a
    // wildcard/`default` contributes exactly one cognitive decision
    // point — the switch itself. Hand-derived: the construct is +1 at
    // nesting depth 0, the explicit arm adds no nesting of its own, and
    // the fallback is silent.
    let expected = 1.0;

    let mut checked = 0;
    for lang in LANG::into_enum_iter() {
        if !lang.is_enabled() {
            continue;
        }
        let Some((source, ext)) = fixture(lang) else {
            continue;
        };
        checked += 1;
        let got = cognitive_max(lang, source, ext);
        assert_eq!(
            got, expected,
            "{lang:?}: cognitive_max {got} != expected {expected}",
        );
    }

    // A table that lost its rows is a real, feature-independent
    // regression, so assert it directly. A build whose enabled
    // languages have no switch-like construct legitimately checks
    // nothing — the state a `#[cfg]`-gated-out module would be in — so
    // `checked > 0` is deliberately not asserted: making it safe would
    // need a hand-maintained feature union naming every `Some` arm,
    // which is the drift this restructure removed (#1281).
    assert!(
        LANG::into_enum_iter().any(|lang| fixture(lang).is_some()),
        "the fixture table has no rows at all, so this test cannot fail",
    );
    // Re-derive the count independently of the loop, so a `continue`
    // added above cannot quietly drop rows.
    let eligible = LANG::into_enum_iter()
        .filter(|lang| lang.is_enabled() && fixture(*lang).is_some())
        .count();
    assert_eq!(
        checked, eligible,
        "the loop checked {checked} languages but {eligible} enabled languages have a fixture",
    );
}

/// A function *declared inside a closure* must score the same as the
/// same function declared outside one (#1187).
///
/// The lambda surcharge is a property of where the closure's *body* sits,
/// not of a new function boundary opened inside it. Until #1187 only the
/// JS macro reset `nesting.lambda` at a boundary, so every other language
/// carried the enclosing closure's surcharge into the nested declaration
/// and the same body scored 3 or 2 depending on whether something two
/// levels up happened to be a closure.
///
/// The paired fixtures are what make this discriminate: an absolute
/// value alone would move with any unrelated re-tuning, whereas the
/// *difference* between the two is exactly the defect. Both halves are
/// asserted so a regression that changed both equally still fails.
///
/// Python is deliberately absent: a `def` is a statement and a lambda
/// body is a single expression, so the shape is unconstructible.
#[test]
fn a_function_declared_inside_a_closure_scores_the_same_as_outside() {
    /// The innermost `g`'s own cognitive score.
    fn inner_g(lang: LANG, source: &str, ext: &str) -> f64 {
        fn find(space: &big_code_analysis::FuncSpace) -> Option<f64> {
            if space.name.as_deref() == Some("g") {
                return Some(space.metrics.cognitive.cognitive() as f64);
            }
            space.spaces.iter().find_map(find)
        }
        let name = format!("parity.{ext}");
        let space = analyze(
            Source::new(lang, source.as_bytes()).with_name(Some(name)),
            MetricsOptions::default(),
        )
        .expect("parser produced no FuncSpace for parity fixture");
        find(&space).unwrap_or_else(|| panic!("{lang:?}: fixture has no function named `g`"))
    }

    // (language, extension, inside a closure, outside one). The inner
    // body is byte-identical between the two halves of each pair.
    let cases: &[(LANG, &str, &str, &str)] = &[
        (
            LANG::Rust,
            "rs",
            "fn outer(a: bool, b: bool) { let f = || { if a { fn g(b: bool) { if b { println!(\"x\"); } } g(b); } }; f(); }",
            "fn outer(a: bool, b: bool) { if a { fn g(b: bool) { if b { println!(\"x\"); } } g(b); } }",
        ),
        (
            LANG::Java,
            "java",
            "class K { void outer(boolean a, boolean b){ Runnable r = () -> { if(a){ class L { void g(boolean b){ if(b){ System.out.print(1); } } } } }; } }",
            "class K { void outer(boolean a, boolean b){ if(a){ class L { void g(boolean b){ if(b){ System.out.print(1); } } } } } }",
        ),
        (
            LANG::Cpp,
            "cpp",
            "void outer(bool a, bool b){ auto f = [&]{ if(a){ struct L { void g(bool b){ if(b){ } } }; } }; f(); }",
            "void outer(bool a, bool b){ if(a){ struct L { void g(bool b){ if(b){ } } }; } }",
        ),
        (
            LANG::Php,
            "php",
            "<?php function outer($a,$b){ $f = function() use ($a,$b) { if($a){ function g($b){ if($b){ echo 1; } } } }; $f(); }",
            "<?php function outer($a,$b){ if($a){ function g($b){ if($b){ echo 1; } } } }",
        ),
        (
            LANG::Csharp,
            "cs",
            "class K{ void outer(bool a, bool b){ Action f = () => { if(a){ void g(bool b){ if(b){ } } } }; f(); } }",
            "class K{ void outer(bool a, bool b){ if(a){ void g(bool b){ if(b){ } } } } }",
        ),
    ];

    let mut checked = 0;
    for &(lang, ext, inside, outside) in cases {
        if !lang.is_enabled() {
            continue;
        }
        checked += 1;
        let (inside_score, outside_score) =
            (inner_g(lang, inside, ext), inner_g(lang, outside, ext));
        assert_eq!(
            inside_score, outside_score,
            "{lang:?}: `g` scored {inside_score} inside a closure and {outside_score} outside it",
        );
        // expected: `g`'s own `if` is +1 base, plus +1 for `g` being
        // lexically nested in another function. The enclosing closure
        // must contribute nothing.
        assert_eq!(
            outside_score, 2.0,
            "{lang:?}: the baseline itself moved, so the equality above proves nothing",
        );
    }
    // An empty `cases` table is a real, feature-independent regression;
    // a build enabling only languages absent from it legitimately checks
    // nothing. Asserting `checked > 0` conflated the two and failed
    // spuriously under, for example, `--no-default-features --features
    // go` (#1281).
    assert!(!cases.is_empty(), "the case table has no rows at all");
    let eligible = cases.iter().filter(|case| case.0.is_enabled()).count();
    assert_eq!(
        checked, eligible,
        "the loop checked {checked} languages but {eligible} enabled languages have a case",
    );
}
