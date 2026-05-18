#![allow(
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
//! `tests/cyclomatic_cross_language_parity.rs`.
//!
//! The fixture is the canonical lesson-11 trigger: a function whose
//! body is a single switch/match with one explicit arm plus a
//! wildcard/default arm. Bug #106 (Rust over-counting wildcard
//! `_ =>` in CCN) is the historical precedent on the cyclomatic
//! side; this test ensures the cognitive side never quietly drifts
//! along the same axis.
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
    .expect("parser produced no FuncSpace for parity fixture");
    space.metrics.cognitive.cognitive_max()
}

// A 2-arm switch/match with one explicit arm plus a wildcard /
// `default` arm contributes one decision point in cognitive
// complexity (the switch itself); the explicit arm adds no extra
// nesting and the fallback is silent. Expected `cognitive_max()` is
// therefore `1` for every language whose grammar models the
// construct as a `switch`/`match`.

#[test]
fn two_arm_wildcard_switch_cognitive_parity() {
    let rust = cognitive_max(
        LANG::Rust,
        r#"fn f(x: u8) -> &'static str {
    match x {
        1 => "one",
        _ => "other",
    }
}
"#,
        "rs",
    );
    let c = cognitive_max(
        LANG::Cpp,
        r"void f(int x) {
    switch (x) {
        case 1: break;
        default: break;
    }
}
",
        "c",
    );
    let java = cognitive_max(
        LANG::Java,
        r"class Parity {
    static void f(int x) {
        switch (x) {
            case 1: break;
            default: break;
        }
    }
}
",
        "java",
    );
    let javascript = cognitive_max(
        LANG::Javascript,
        r"function f(x) {
    switch (x) {
        case 1: break;
        default: break;
    }
}
",
        "js",
    );
    let typescript = cognitive_max(
        LANG::Typescript,
        r"function f(x: number) {
    switch (x) {
        case 1: break;
        default: break;
    }
}
",
        "ts",
    );
    let php = cognitive_max(
        LANG::Php,
        r"<?php
function f($x) {
    switch ($x) {
        case 1: break;
        default: break;
    }
}
",
        "php",
    );
    let csharp = cognitive_max(
        LANG::Csharp,
        r"class Parity {
    static void F(int x) {
        switch (x) {
            case 1: break;
            default: break;
        }
    }
}
",
        "cs",
    );
    // Kotlin spells the same construct `when`; Go's `switch` shares
    // the same node category.
    let kotlin = cognitive_max(
        LANG::Kotlin,
        r#"fun f(x: Int): String {
    return when (x) {
        1 -> "one"
        else -> "other"
    }
}
"#,
        "kt",
    );
    let go = cognitive_max(
        LANG::Go,
        r#"package p
func f(x int) string {
    switch x {
    case 1:
        return "one"
    default:
        return "other"
    }
}
"#,
        "go",
    );
    let bash = cognitive_max(
        LANG::Bash,
        "f() {\n  case \"$1\" in\n    one) echo one ;;\n    *) echo other ;;\n  esac\n}\n",
        "sh",
    );
    let python = cognitive_max(
        LANG::Python,
        "def f(x):\n    match x:\n        case 1:\n            return 'one'\n        case _:\n            return 'other'\n",
        "py",
    );
    let groovy = cognitive_max(
        LANG::Groovy,
        r"def f(x) {
    switch (x) {
        case 1: break
        default: break
    }
}
",
        "groovy",
    );

    // expected: one explicit arm + wildcard/default in a single
    // switch/match contributes one cognitive decision point.
    let expected = 1.0;
    assert_eq!(rust, expected, "rust");
    assert_eq!(c, expected, "c");
    assert_eq!(java, expected, "java");
    assert_eq!(javascript, expected, "javascript");
    assert_eq!(typescript, expected, "typescript");
    assert_eq!(php, expected, "php");
    assert_eq!(csharp, expected, "csharp");
    assert_eq!(kotlin, expected, "kotlin");
    assert_eq!(go, expected, "go");
    assert_eq!(bash, expected, "bash");
    assert_eq!(python, expected, "python");
    assert_eq!(groovy, expected, "groovy");
}
