#![allow(
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
//! to `tests/cyclomatic_cross_language_parity.rs`.
//!
//! The fixture is the simplest possible: a function with exactly
//! three formal parameters, body empty. Every supported language
//! whose grammar models a named parameter list reports
//! `fn_args_sum() == 3`.
//!
//! The test uses `fn_args_sum()` (matches the metric definition;
//! file-level sum over functions, but with one function the value
//! equals that function's nargs).
//!
//! **Excluded — Bash**: Bash functions take no formal parameters in
//! the grammar — arguments are positional (`$1`, `$2`, …), so
//! `fn_args_sum()` is structurally always `0`. This is a
//! language-mandated absence, not a metric drift; documented here
//! rather than asserted.

use big_code_analysis::{LANG, MetricsOptions, Source, analyze};

/// `fn_args` file-level sum for the single function in `source`.
fn fn_args_sum(lang: LANG, source: &str, ext: &str) -> f64 {
    let name = format!("parity.{ext}");
    let space = analyze(
        Source::new(lang, source.as_bytes()).with_name(Some(name)),
        MetricsOptions::default(),
    )
    .expect("parser produced no FuncSpace for parity fixture");
    space.metrics.nargs.fn_args_sum()
}

#[test]
fn three_parameter_function_parity() {
    let rust = fn_args_sum(
        LANG::Rust,
        r"fn f(a: i32, b: i32, c: i32) {}
",
        "rs",
    );
    let c = fn_args_sum(
        LANG::Cpp,
        r"void f(int a, int b, int c) {}
",
        "c",
    );
    let java = fn_args_sum(
        LANG::Java,
        r"class Parity {
    static void f(int a, int b, int c) {}
}
",
        "java",
    );
    let javascript = fn_args_sum(
        LANG::Javascript,
        r"function f(a, b, c) {}
",
        "js",
    );
    let typescript = fn_args_sum(
        LANG::Typescript,
        r"function f(a: number, b: number, c: number) {}
",
        "ts",
    );
    let python = fn_args_sum(
        LANG::Python,
        r"def f(a, b, c):
    pass
",
        "py",
    );
    let php = fn_args_sum(
        LANG::Php,
        r"<?php
function f($a, $b, $c) {}
",
        "php",
    );
    let csharp = fn_args_sum(
        LANG::Csharp,
        r"class Parity {
    static void F(int a, int b, int c) {}
}
",
        "cs",
    );
    let kotlin = fn_args_sum(
        LANG::Kotlin,
        r"fun f(a: Int, b: Int, c: Int) {}
",
        "kt",
    );
    let go = fn_args_sum(
        LANG::Go,
        r"package p
func f(a int, b int, c int) {}
",
        "go",
    );
    let groovy = fn_args_sum(
        LANG::Groovy,
        r"def f(int a, int b, int c) {}
",
        "groovy",
    );

    // expected: three formal parameters.
    let expected = 3.0;
    assert_eq!(rust, expected, "rust");
    assert_eq!(c, expected, "c");
    assert_eq!(java, expected, "java");
    assert_eq!(javascript, expected, "javascript");
    assert_eq!(typescript, expected, "typescript");
    assert_eq!(python, expected, "python");
    assert_eq!(php, expected, "php");
    assert_eq!(csharp, expected, "csharp");
    assert_eq!(kotlin, expected, "kotlin");
    assert_eq!(go, expected, "go");
    assert_eq!(groovy, expected, "groovy");
}
