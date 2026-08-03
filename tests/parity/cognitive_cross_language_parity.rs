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
    space.metrics.cognitive.cognitive_max() as f64
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
    // Tcl spells the construct as a generic `switch` command; the structure
    // adds one cognitive decision point, the `default` arm is free (issue #467).
    let tcl = cognitive_max(
        LANG::Tcl,
        r"proc f {x} {
    switch $x {
        1 { return one }
        default { return other }
    }
}
",
        "tcl",
    );
    // iRules spells `switch` as a dedicated node, but the cognitive
    // contribution is the same: one decision point, the `default` arm free.
    let irules = cognitive_max(
        LANG::Irules,
        r"proc f {x} {
    switch $x {
        1 { return one }
        default { return other }
    }
}
",
        "irule",
    );
    // Objective-C uses a free C `function_definition` (valid in a `.m`
    // file); its `switch_statement` is the single decision point and the
    // `default:` arm is free, matching the family.
    let objc = cognitive_max(
        LANG::Objc,
        r"void f(int x) {
    switch (x) {
        case 1: break;
        default: break;
    }
}
",
        "m",
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
    assert_eq!(tcl, expected, "tcl");
    assert_eq!(irules, expected, "irules");
    assert_eq!(objc, expected, "objc");
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
    assert!(
        checked > 0,
        "at least one language feature must be enabled for this test to mean anything"
    );
}
