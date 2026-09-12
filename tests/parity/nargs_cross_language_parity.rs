#![allow(
    clippy::cast_precision_loss,
    clippy::float_cmp,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]

//! Cross-language parity test for **nargs** (formal parameter count).
//!
//! Per lesson 11 in `docs/development/lessons_learned.md`, the same
//! logical construct must produce the same metric value across every
//! language we support — per-language snapshot suites cannot detect
//! disagreement between languages. This file is the nargs companion
//! to `tests/parity/cyclomatic_cross_language_parity.rs`.
//!
//! The fixture is the simplest possible: a function with exactly three
//! formal parameters, body empty.
//!
//! The fixture table is an exhaustive `match` on [`LANG`], so adding a
//! language variant fails to compile until someone decides whether its
//! grammar models a named parameter list. A language that does not says
//! so with `None`, and the reason lives at the arm — the coverage claim
//! is therefore structural rather than a prose list that can fall behind
//! the language roster (#1281; the previous hand-maintained list named
//! 15 languages and silently omitted six that do model one).
//!
//! The test uses `function_args_sum()` (matches the metric definition;
//! file-level sum over functions, but with one function the value
//! equals that function's nargs).

use big_code_analysis::{LANG, MetricsOptions, Source, analyze};

/// `function_args` file-level sum for the single function in `source`.
fn function_args_sum(lang: LANG, source: &str, ext: &str) -> f64 {
    let name = format!("parity.{ext}");
    let space = analyze(
        Source::new(lang, source.as_bytes()).with_name(Some(name)),
        MetricsOptions::default(),
    )
    .unwrap_or_else(|e| panic!("{lang:?}: analyze failed: {e}"));
    space.metrics.nargs.function_args_sum() as f64
}

/// Returns `(source, extension)` for a language whose grammar models a
/// named formal-parameter list, or `None` for one that does not.
///
/// Every `Some` row declares the same three parameters `a`, `b`, `c`.
/// The extension only names the parsed unit; it reaches no metric.
fn fixture(lang: LANG) -> Option<(&'static str, &'static str)> {
    // Exhaustive per-language dispatch table: one arm per LANG variant
    // is the point of this function, so a new language cannot be added
    // without deciding whether it has a formal parameter list. The
    // repo's own `.bcaignore` excludes `./tests/**`, so this marker is
    // for the per-edit `bca check` hook rather than for the self-scan
    // gate.
    // bca: suppress(cyclomatic)
    let row = match lang {
        LANG::Rust => ("fn f(a: i32, b: i32, c: i32) {}\n", "rs"),
        // One arm for the whole C family: the fixture is plain C, which
        // all four grammars accept unchanged.
        LANG::C | LANG::Cpp | LANG::Mozcpp | LANG::Objc => {
            ("void f(int a, int b, int c) {}\n", "c")
        }
        LANG::Java => (
            "class Parity {\n    static void f(int a, int b, int c) {}\n}\n",
            "java",
        ),
        LANG::Csharp => (
            "class Parity {\n    static void F(int a, int b, int c) {}\n}\n",
            "cs",
        ),
        LANG::Javascript | LANG::Mozjs => ("function f(a, b, c) {}\n", "js"),
        LANG::Typescript => ("function f(a: number, b: number, c: number) {}\n", "ts"),
        LANG::Tsx => ("function f(a: number, b: number, c: number) {}\n", "tsx"),
        LANG::Python => ("def f(a, b, c):\n    pass\n", "py"),
        LANG::Php => ("<?php\nfunction f($a, $b, $c) {}\n", "php"),
        LANG::Kotlin => ("fun f(a: Int, b: Int, c: Int) {}\n", "kt"),
        LANG::Go => ("package p\nfunc f(a int, b int, c int) {}\n", "go"),
        LANG::Groovy => ("def f(int a, int b, int c) {}\n", "groovy"),
        LANG::Ruby => ("def f(a, b, c)\n  a + b + c\nend\n", "rb"),
        LANG::Lua => ("function f(a, b, c)\nend\n", "lua"),
        // Elixir's `def` must live inside a `defmodule`; the module
        // space contributes no parameters, so no offset is needed.
        LANG::Elixir => (
            "defmodule Parity do\n  def f(a, b, c) do\n    a\n  end\nend\n",
            "ex",
        ),
        LANG::Tcl => ("proc f {a b c} { puts $a }\n", "tcl"),
        LANG::Irules => ("proc f { a b c } { return $a }\n", "irule"),
        // Bash and Perl functions take no formal parameters in the
        // grammar: arguments arrive positionally (`$1`, `$2`, … and
        // `@_`), so `function_args_sum()` is structurally always `0`.
        // Measured: both report 0 for a three-argument call shape. This
        // is a language-mandated absence, not a metric drift.
        //
        // The two C-family helper grammars parse fragments (a comment
        // body, a preprocessor directive) and have no function concept
        // at all.
        LANG::Bash | LANG::Perl | LANG::Ccomment | LANG::Preproc => return None,
    };
    Some(row)
}

#[test]
fn three_parameter_function_parity() {
    // expected: three formal parameters, hand-derived from each fixture
    // by counting the names in its parameter list.
    let expected = 3.0;

    let mut checked = 0;
    for lang in LANG::into_enum_iter() {
        if !lang.is_enabled() {
            continue;
        }
        let Some((source, ext)) = fixture(lang) else {
            continue;
        };
        checked += 1;
        let got = function_args_sum(lang, source, ext);
        assert_eq!(
            got, expected,
            "{lang:?}: nargs {got} != expected {expected}"
        );
    }

    // Two ways this loop can assert nothing, and only one is a defect.
    //
    // A table that lost its rows is a real regression, and it is
    // feature-independent, so assert it directly. A build whose enabled
    // languages all take positional arguments legitimately has nothing
    // to check — the same state a `#[cfg]`-gated-out module would be in
    // — so `checked > 0` is deliberately *not* asserted: making it safe
    // would need a hand-maintained feature union naming every `Some`
    // arm, which is the drift this restructure removed (#1281).
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
