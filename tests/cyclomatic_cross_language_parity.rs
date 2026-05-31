#![allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]

//! Cross-language parity tests for **standard** cyclomatic complexity
//! (CCN).
//!
//! Per-language snapshot tests pin behaviour against each language's own
//! history, but cannot detect that two languages disagree about the same
//! logical construct — exactly the failure mode behind bugs #106 (Rust
//! wildcard `_ =>` counted while C-family `default:` was not) and #107
//! (Bash `case…esac` double-counted container plus arms).
//!
//! Lesson 11 in `docs/development/lessons_learned.md` ("The same metric
//! across languages must agree on the same logical construct")
//! prescribes a cross-language parity test for this metric. That is
//! what this file is.
//!
//! For every construct family below the test builds a
//! `BTreeMap<&str, f64>` keyed by language with its standard-CCN sum, then
//! asserts every value is equal **after subtracting a small set of
//! explicitly-documented per-language offsets** that account for
//! intrinsic, language-mandated structural differences (e.g. Java's
//! requirement that every function live inside a class, which adds a
//! `FuncSpace` and therefore +1 to every sum). On failure the full
//! before/after maps are printed so any drift and the language that
//! drifted are immediately visible.
//!
//! Each offset must be either (a) a structural language requirement
//! that cannot be removed by rewriting the fixture, or (b) a
//! standard-CCN bug whose root cause is documented inline next to the
//! offset definition. Adding an offset *without* one of those is a sign
//! the parity test is being papered over rather than enforcing the
//! lesson — review carefully.
//!
//! The tests deliberately exercise the **standard** CCN variant
//! (`metrics.cyclomatic.cyclomatic_sum()`), not the modified variant
//! introduced in `16cd610`. The modified variant was added in part to
//! mask per-language quirks; running parity against it would defeat the
//! point.

use std::collections::BTreeMap;

use big_code_analysis::{LANG, MetricsOptions, Source, analyze};

/// Parses `source` as `lang` and returns the file-level standard CCN sum.
fn ccn_sum(lang: LANG, source: &str, ext: &str) -> f64 {
    let name = format!("parity.{ext}");
    let space = analyze(
        Source::new(lang, source.as_bytes()).with_name(Some(name)),
        MetricsOptions::default(),
    )
    .expect("parser produced no FuncSpace for parity fixture");
    space.metrics.cyclomatic.cyclomatic_sum()
}

/// Java requires every function to live inside a class, which the
/// FuncSpace traversal counts as an extra space and so adds +1 to every
/// CCN sum. This is a language-mandated structural difference, not a
/// metric bug.
const JAVA_CLASS_OFFSET: f64 = 1.0;

/// Asserts every language in `sums` reports the same CCN value after
/// subtracting `offsets[lang]` (default 0). Both maps and the
/// normalised values are printed on failure so the drift is obvious.
#[track_caller]
fn assert_parity(family: &str, sums: &BTreeMap<&str, f64>, offsets: &BTreeMap<&str, f64>) {
    let normalised: BTreeMap<&str, f64> = sums
        .iter()
        .map(|(k, v)| (*k, v - offsets.get(k).copied().unwrap_or(0.0)))
        .collect();
    let unique: std::collections::BTreeSet<_> = normalised.values().map(|v| v.to_bits()).collect();
    assert!(
        unique.len() == 1,
        "{family}: cross-language CCN drift detected\n  raw sums:    {sums:#?}\n  offsets:     {offsets:#?}\n  normalised:  {normalised:#?}",
    );
}

/// Asserts every language's normalised CCN (`sums[lang] - offsets[lang]`)
/// equals `expected`. This pins the *absolute* magnitude a family is
/// built around, so a regression that shifts every language by the same
/// amount — passing `assert_parity` because they still agree — is caught.
/// The per-family `expected` value and its derivation live at the call
/// site (lessons #6 / #23 / #468).
#[track_caller]
fn assert_normalised(
    sums: &BTreeMap<&str, f64>,
    offsets: &BTreeMap<&str, f64>,
    expected: f64,
    family: &str,
) {
    for (lang, sum) in sums {
        let normalised = sum - offsets.get(lang).copied().unwrap_or(0.0);
        assert_eq!(
            normalised, expected,
            "{family}/{lang}: normalised CCN {normalised} != expected {expected}",
        );
    }
}

// --- Family 1: switch / match with fallback arm ----------------------------
//
// Three semantically equivalent fixtures, each defining a single function
// that switches an integer against three explicit arms plus a fallback
// (`default:` in C/Java, `_ =>` in Rust). Standard CCN counts the three
// explicit arms and skips the fallback (Rust treats bare `_ =>` like
// C-family `default:` after #106 / `a54b073`).
//
// Per-language post-offset expectation: unit(1) + fn(1) + 3 explicit arms = 5.

#[test]
fn switch_with_default_parity() {
    let mut sums = BTreeMap::new();
    sums.insert(
        "rust",
        ccn_sum(
            LANG::Rust,
            r#"fn f(x: u8) -> &'static str {
    match x {
        1 => "one",
        2 => "two",
        3 => "three",
        _ => "other",
    }
}
"#,
            "rs",
        ),
    );
    sums.insert(
        "c",
        ccn_sum(
            LANG::Cpp,
            r"void f(int x) {
    switch (x) {
        case 1: break;
        case 2: break;
        case 3: break;
        default: break;
    }
}
",
            "c",
        ),
    );
    sums.insert(
        "java",
        ccn_sum(
            LANG::Java,
            r"class Parity {
    static void f(int x) {
        switch (x) {
            case 1: break;
            case 2: break;
            case 3: break;
            default: break;
        }
    }
}
",
            "java",
        ),
    );
    // C# switch-expression with discard arm `_ =>` (issue #282).
    sums.insert(
        "csharp",
        ccn_sum(
            LANG::Csharp,
            r#"class Parity {
    static string f(int x) => x switch {
        1 => "one",
        2 => "two",
        3 => "three",
        _ => "other"
    };
}
"#,
            "cs",
        ),
    );
    // Kotlin `when` with `else ->` arm (issue #282).
    sums.insert(
        "kotlin",
        ccn_sum(
            LANG::Kotlin,
            r#"fun f(x: Int): String = when (x) {
    1 -> "one"
    2 -> "two"
    3 -> "three"
    else -> "other"
}
"#,
            "kt",
        ),
    );
    // Tcl `switch` is a generic command whose non-`default` arms are the
    // decision points; `default` is free, matching the C-family `default:`
    // (issue #467). Procs are top-level, so no class offset.
    sums.insert(
        "tcl",
        ccn_sum(
            LANG::Tcl,
            r"proc f {x} {
    switch $x {
        1 { return one }
        2 { return two }
        3 { return three }
        default { return other }
    }
}
",
            "tcl",
        ),
    );
    // iRules `switch` is a dedicated node (unlike Tcl's command form), but
    // counts the same: 3 non-`default` arms, fallback free. Procs are
    // top-level, so no class offset.
    sums.insert(
        "irules",
        ccn_sum(
            LANG::Irules,
            r"proc f {x} {
    switch $x {
        1 { return one }
        2 { return two }
        3 { return three }
        default { return other }
    }
}
",
            "irule",
        ),
    );
    let offsets = BTreeMap::from([
        ("java", JAVA_CLASS_OFFSET),
        // C# requires every function to live inside a class, same as
        // Java — this is a language-mandated structural difference.
        ("csharp", JAVA_CLASS_OFFSET),
    ]);
    assert_parity("switch_with_default", &sums, &offsets);

    // Anchor the absolute magnitude, not just mutual agreement (lessons
    // #6 / #23 / #468): a symmetric regression that dropped or added +1
    // across every language would still satisfy `assert_parity`. Pin the
    // hand-derived spec value: unit(1) + fn(1) + 3 explicit arms = 5
    // (fallback skipped post-#106), Java/C# normalised by JAVA_CLASS_OFFSET.
    assert_normalised(&sums, &offsets, 5.0, "switch_with_default");
}

// --- Family 2: switch / case / match without fallback ----------------------
//
// Three explicit arms, no semantic fallback. Bash joins this family
// because `case…esac` semantics count each arm exactly once after the
// #107 / `e668f14` fix; with no `*)` arm there is no asymmetry between
// Bash and C-family.
//
// Rust requires exhaustive matching, so its fixture still includes a
// bare `_ => {}` arm — but post-#106 (`a54b073`) bare wildcards are
// skipped in standard CCN, so the match still counts only the three
// explicit arms (semantically equivalent to a C `switch` whose
// unmatched values do nothing).
//
// Per-language post-offset expectation: unit(1) + fn(1) + 3 arms = 5.

#[test]
fn switch_without_default_parity() {
    let mut sums = BTreeMap::new();
    sums.insert(
        "rust",
        ccn_sum(
            LANG::Rust,
            r"fn f(x: u8) {
    match x {
        1 => {}
        2 => {}
        3 => {}
        _ => {}
    }
}
",
            "rs",
        ),
    );
    sums.insert(
        "c",
        ccn_sum(
            LANG::Cpp,
            r"void f(int x) {
    switch (x) {
        case 1: break;
        case 2: break;
        case 3: break;
    }
}
",
            "c",
        ),
    );
    sums.insert(
        "java",
        ccn_sum(
            LANG::Java,
            r"class Parity {
    static void f(int x) {
        switch (x) {
            case 1: break;
            case 2: break;
            case 3: break;
        }
    }
}
",
            "java",
        ),
    );
    sums.insert(
        "bash",
        ccn_sum(
            LANG::Bash,
            r#"#!/bin/bash
f() {
    case "$1" in
        one)   echo 1 ;;
        two)   echo 2 ;;
        three) echo 3 ;;
    esac
}
"#,
            "sh",
        ),
    );
    // C# switch *statement* (not expression — switch expressions require
    // exhaustiveness so cannot express "no default"). Issue #282.
    sums.insert(
        "csharp",
        ccn_sum(
            LANG::Csharp,
            r"class Parity {
    static void f(int x) {
        switch (x) {
            case 1: break;
            case 2: break;
            case 3: break;
        }
    }
}
",
            "cs",
        ),
    );
    // Kotlin `when` used as a statement does not require an `else` arm
    // (only `when` as an expression with sealed types does). Issue #282.
    sums.insert(
        "kotlin",
        ccn_sum(
            LANG::Kotlin,
            r"fun f(x: Int) {
    when (x) {
        1 -> { }
        2 -> { }
        3 -> { }
    }
}
",
            "kt",
        ),
    );
    // Tcl `switch` with no `default` arm: all three arms are decision
    // points (issue #467). Procs are top-level, so no class offset.
    sums.insert(
        "tcl",
        ccn_sum(
            LANG::Tcl,
            r"proc f {x} {
    switch $x {
        1 { return one }
        2 { return two }
        3 { return three }
    }
}
",
            "tcl",
        ),
    );
    let offsets = BTreeMap::from([("java", JAVA_CLASS_OFFSET), ("csharp", JAVA_CLASS_OFFSET)]);
    assert_parity("switch_without_default", &sums, &offsets);

    // Anchor the absolute magnitude (lessons #6 / #23 / #468):
    // unit(1) + fn(1) + 3 arms = 5 (no fallback to count; Rust's bare
    // `_ => {}` skipped post-#106, Bash arms count once post-#107).
    assert_normalised(&sums, &offsets, 5.0, "switch_without_default");
}

// --- Family 3: if / else if / else chain -----------------------------------
//
// A three-condition chain (one `if`, two `else if`, one `else`). Each
// condition contributes one decision point. JavaScript and Python join
// here; Bash uses `elif` which is counted via `ElifClause`.
//
// Per-language expectation: unit(1) + fn(1) + 3 conditions = 5.

#[test]
fn if_else_if_else_chain_parity() {
    let mut sums = BTreeMap::new();
    sums.insert(
        "rust",
        ccn_sum(
            LANG::Rust,
            r"fn f(x: i32) -> i32 {
    if x == 1 {
        10
    } else if x == 2 {
        20
    } else if x == 3 {
        30
    } else {
        0
    }
}
",
            "rs",
        ),
    );
    sums.insert(
        "c",
        ccn_sum(
            LANG::Cpp,
            r"int f(int x) {
    if (x == 1) {
        return 10;
    } else if (x == 2) {
        return 20;
    } else if (x == 3) {
        return 30;
    } else {
        return 0;
    }
}
",
            "c",
        ),
    );
    sums.insert(
        "java",
        ccn_sum(
            LANG::Java,
            r"class Parity {
    static int f(int x) {
        if (x == 1) {
            return 10;
        } else if (x == 2) {
            return 20;
        } else if (x == 3) {
            return 30;
        } else {
            return 0;
        }
    }
}
",
            "java",
        ),
    );
    sums.insert(
        "javascript",
        ccn_sum(
            LANG::Javascript,
            r"function f(x) {
    if (x === 1) {
        return 10;
    } else if (x === 2) {
        return 20;
    } else if (x === 3) {
        return 30;
    } else {
        return 0;
    }
}
",
            "js",
        ),
    );
    sums.insert(
        "python",
        ccn_sum(
            LANG::Python,
            r"def f(x):
    if x == 1:
        return 10
    elif x == 2:
        return 20
    elif x == 3:
        return 30
    else:
        return 0
",
            "py",
        ),
    );
    sums.insert(
        "bash",
        ccn_sum(
            LANG::Bash,
            r#"#!/bin/bash
f() {
    if [ "$1" -eq 1 ]; then
        echo 10
    elif [ "$1" -eq 2 ]; then
        echo 20
    elif [ "$1" -eq 3 ]; then
        echo 30
    else
        echo 0
    fi
}
"#,
            "sh",
        ),
    );
    sums.insert(
        "groovy",
        ccn_sum(
            LANG::Groovy,
            r"def f(x) {
    if (x == 1) {
        return 10
    } else if (x == 2) {
        return 20
    } else if (x == 3) {
        return 30
    } else {
        return 0
    }
}
",
            "groovy",
        ),
    );
    // iRules has a dedicated `elseif` node; each `elseif` is a decision and
    // the trailing `else` is free, matching the family. Proc is top-level,
    // so no class offset.
    sums.insert(
        "irules",
        ccn_sum(
            LANG::Irules,
            r"proc f {x} {
    if { $x == 1 } { return 10 } elseif { $x == 2 } { return 20 } elseif { $x == 3 } { return 30 } else { return 0 }
}
",
            "irule",
        ),
    );
    let offsets = BTreeMap::from([("java", JAVA_CLASS_OFFSET)]);
    assert_parity("if_else_if_else_chain", &sums, &offsets);

    // Anchor the absolute magnitude (lessons #6 / #23 / #468):
    // unit(1) + fn(1) + 3 conditions = 5 (one `if`, two `else if`/`elif`;
    // the trailing `else` is not a decision point). This family caught
    // the #229 Python over-count, so pinning the value also guards the
    // `parent_grandparent_match` fix from a symmetric regression.
    assert_normalised(&sums, &offsets, 5.0, "if_else_if_else_chain");
}

// --- Family 4: single `if` with no `else` ----------------------------------
//
// The simplest decision form: one branching condition, no else clause.
// No else clause means none of the per-language `else`/elif
// idiosyncrasies apply.
//
// Per-language post-offset expectation: unit(1) + fn(1) + 1 if = 3.

#[test]
fn single_if_no_else_parity() {
    let mut sums = BTreeMap::new();
    sums.insert(
        "rust",
        ccn_sum(
            LANG::Rust,
            r"fn f(x: i32) {
    if x == 1 {
        let _ = x;
    }
}
",
            "rs",
        ),
    );
    sums.insert(
        "c",
        ccn_sum(
            LANG::Cpp,
            r"void f(int x) {
    if (x == 1) {
        x = x;
    }
}
",
            "c",
        ),
    );
    sums.insert(
        "java",
        ccn_sum(
            LANG::Java,
            r"class Parity {
    static void f(int x) {
        if (x == 1) {
            x = x;
        }
    }
}
",
            "java",
        ),
    );
    sums.insert(
        "javascript",
        ccn_sum(
            LANG::Javascript,
            r"function f(x) {
    if (x === 1) {
        return x;
    }
}
",
            "js",
        ),
    );
    sums.insert(
        "python",
        ccn_sum(
            LANG::Python,
            r"def f(x):
    if x == 1:
        return x
",
            "py",
        ),
    );
    sums.insert(
        "bash",
        ccn_sum(
            LANG::Bash,
            r#"#!/bin/bash
f() {
    if [ "$1" -eq 1 ]; then
        echo "$1"
    fi
}
"#,
            "sh",
        ),
    );
    sums.insert(
        "groovy",
        ccn_sum(
            LANG::Groovy,
            r"def f(x) {
    if (x == 1) {
        println x
    }
}
",
            "groovy",
        ),
    );
    let offsets = BTreeMap::from([("java", JAVA_CLASS_OFFSET)]);
    assert_parity("single_if_no_else", &sums, &offsets);

    // Anchor the absolute magnitude (lessons #6 / #23 / #468):
    // unit(1) + fn(1) + 1 if = 3. No else clause, so no per-language
    // else/elif idiosyncrasy contributes.
    assert_normalised(&sums, &offsets, 3.0, "single_if_no_else");
}

// --- Family 5: 2-arm switch / match with wildcard / default ---------------
//
// One explicit arm plus a fallback (`default:` in C-family, `_ =>` in
// Rust). This is the canonical lesson-11 trigger: bug #106 had Rust
// counting the bare `_ =>` while C-family `default:` was correctly
// skipped, and the divergence was invisible until a parity test
// existed. Family 1 already covers the 3-arm variant; this family
// adds the minimal 2-arm form so the test catches both an
// over-count of the wildcard *and* an under-count of the explicit
// arm.
//
// Per-language post-offset expectation: unit(1) + fn(1) + 1 arm = 3.

#[test]
fn two_arm_switch_with_wildcard_parity() {
    let mut sums = BTreeMap::new();
    sums.insert(
        "rust",
        ccn_sum(
            LANG::Rust,
            r#"fn f(x: u8) -> &'static str {
    match x {
        1 => "one",
        _ => "other",
    }
}
"#,
            "rs",
        ),
    );
    sums.insert(
        "c",
        ccn_sum(
            LANG::Cpp,
            r"void f(int x) {
    switch (x) {
        case 1: break;
        default: break;
    }
}
",
            "c",
        ),
    );
    sums.insert(
        "java",
        ccn_sum(
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
        ),
    );
    sums.insert(
        "javascript",
        ccn_sum(
            LANG::Javascript,
            r"function f(x) {
    switch (x) {
        case 1: break;
        default: break;
    }
}
",
            "js",
        ),
    );
    sums.insert(
        "typescript",
        ccn_sum(
            LANG::Typescript,
            r"function f(x: number) {
    switch (x) {
        case 1: break;
        default: break;
    }
}
",
            "ts",
        ),
    );
    sums.insert(
        "php",
        ccn_sum(
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
        ),
    );
    sums.insert(
        "python",
        ccn_sum(
            LANG::Python,
            "def f(x):\n    match x:\n        case 1:\n            return 'one'\n        case _:\n            return 'other'\n",
            "py",
        ),
    );
    sums.insert(
        "bash",
        ccn_sum(
            LANG::Bash,
            "#!/bin/bash\nf() {\n    case \"$1\" in\n        one) echo 1 ;;\n        *)   echo 0 ;;\n    esac\n}\n",
            "sh",
        ),
    );
    // C# switch-expression with discard arm (issue #282).
    sums.insert(
        "csharp",
        ccn_sum(
            LANG::Csharp,
            r#"class Parity {
    static string f(int x) => x switch {
        1 => "one",
        _ => "other"
    };
}
"#,
            "cs",
        ),
    );
    // Kotlin `when` with `else ->` arm (issue #282).
    sums.insert(
        "kotlin",
        ccn_sum(
            LANG::Kotlin,
            r#"fun f(x: Int): String = when (x) {
    1 -> "one"
    else -> "other"
}
"#,
            "kt",
        ),
    );
    let offsets = BTreeMap::from([("java", JAVA_CLASS_OFFSET), ("csharp", JAVA_CLASS_OFFSET)]);
    assert_parity("two_arm_switch_with_wildcard", &sums, &offsets);

    // Anchor the absolute magnitude (lessons #6 / #23 / #468):
    // unit(1) + fn(1) + 1 explicit arm = 3 (the wildcard/`default` is
    // skipped). This is the canonical lesson-11 trigger (#106): pinning
    // the value guards against both an over-count of the wildcard and an
    // under-count of the explicit arm shifting symmetrically.
    assert_normalised(&sums, &offsets, 3.0, "two_arm_switch_with_wildcard");
}

// --- Family 6: do-while loop ---------------------------------------------
//
// A single `do { … } while (cond)` loop must contribute exactly one
// decision point in every language that exposes the construct. In the
// C-family cyclomatic impls this fires via the inner `while` keyword
// token (`Cpp::While`, `Java::While`, `Groovy::While`), which is the
// same token used for a standalone `while` loop — listing the
// `DoStatement` node itself would double-count (regression for issue
// #284's incorrect fix proposal).
//
// Per-language post-offset expectation: unit(1) + fn(1) + do(1) = 3.

#[test]
fn do_while_loop_parity() {
    let mut sums = BTreeMap::new();
    sums.insert(
        "c",
        ccn_sum(
            LANG::Cpp,
            r"void f() {
    int i = 0;
    do {
        ++i;
    } while (i < 10);
}
",
            "c",
        ),
    );
    sums.insert(
        "java",
        ccn_sum(
            LANG::Java,
            r"class Parity {
    static void f() {
        int i = 0;
        do {
            ++i;
        } while (i < 10);
    }
}
",
            "java",
        ),
    );
    sums.insert(
        "groovy",
        ccn_sum(
            LANG::Groovy,
            r"def f() {
    int i = 0
    do {
        ++i
    } while (i < 10)
}
",
            "groovy",
        ),
    );
    sums.insert(
        "javascript",
        ccn_sum(
            LANG::Javascript,
            r"function f() {
    let i = 0;
    do {
        ++i;
    } while (i < 10);
}
",
            "js",
        ),
    );
    sums.insert(
        "csharp",
        ccn_sum(
            LANG::Csharp,
            r"class Parity {
    static void f() {
        int i = 0;
        do {
            ++i;
        } while (i < 10);
    }
}
",
            "cs",
        ),
    );
    let offsets = BTreeMap::from([("java", JAVA_CLASS_OFFSET), ("csharp", JAVA_CLASS_OFFSET)]);
    assert_parity("do_while_loop", &sums, &offsets);

    // Anchor the absolute magnitude (lessons #6 / #23 / #468):
    // unit(1) + fn(1) + do(1) = 3. The loop fires once via the inner
    // `while` keyword token; pinning the value catches a regression that
    // additionally listed `DoStatement` (double-count, #284) symmetrically.
    assert_normalised(&sums, &offsets, 3.0, "do_while_loop");
}

// --- Family 7: range / enhanced for loop ---------------------------------
//
// Iterator-style loops — C++ `for (auto x : xs)` (`ForRangeLoop`),
// Java enhanced-for `for (T x : xs)` (`EnhancedForStatement`), Groovy
// enhanced-for `for (T x : xs)` (`EnhancedForStatement`), C#
// `foreach (var x in xs)` (`ForeachStatement`), Kotlin `for (x in xs)`
// — each must contribute exactly one decision point.
//
// In C++/Java/Groovy the `for` keyword token fires inside the
// grammar-specific loop node, so the existing keyword-token arm
// catches them; listing the statement node would double-count
// (regression for issue #284's incorrect fix proposal). C# has a
// dedicated `ForeachStatement` arm; Kotlin counts via `ForStatement`.
//
// Per-language post-offset expectation: unit(1) + fn(1) + for(1) = 3.

#[test]
fn range_for_loop_parity() {
    let mut sums = BTreeMap::new();
    sums.insert(
        "c",
        ccn_sum(
            LANG::Cpp,
            r"void f(std::vector<int> xs) {
    for (auto x : xs) {
        g(x);
    }
}
",
            "cpp",
        ),
    );
    sums.insert(
        "java",
        ccn_sum(
            LANG::Java,
            r"class Parity {
    static void f(int[] xs) {
        for (int x : xs) {
            g(x);
        }
    }
}
",
            "java",
        ),
    );
    sums.insert(
        "groovy",
        ccn_sum(
            LANG::Groovy,
            r"def f(int[] xs) {
    for (int x : xs) {
        println x
    }
}
",
            "groovy",
        ),
    );
    sums.insert(
        "csharp",
        ccn_sum(
            LANG::Csharp,
            r"class Parity {
    static void f(int[] xs) {
        foreach (var x in xs) {
            g(x);
        }
    }
}
",
            "cs",
        ),
    );
    sums.insert(
        "kotlin",
        ccn_sum(
            LANG::Kotlin,
            r"fun f(xs: IntArray) {
    for (x in xs) {
        g(x)
    }
}
",
            "kt",
        ),
    );
    let offsets = BTreeMap::from([("java", JAVA_CLASS_OFFSET), ("csharp", JAVA_CLASS_OFFSET)]);
    assert_parity("range_for_loop", &sums, &offsets);

    // Anchor the absolute magnitude (lessons #6 / #23 / #468):
    // unit(1) + fn(1) + for(1) = 3. Each range/enhanced-for fires exactly
    // one decision point; pinning the value catches a regression that
    // additionally listed the statement node (double-count, #284) across
    // all languages symmetrically.
    assert_normalised(&sums, &offsets, 3.0, "range_for_loop");
}

// --- Family 8: safe-navigation operator chains ---------------------------
//
// A two-link safe-navigation chain — Kotlin `a?.b?.c`, Groovy `a?.b?.c`,
// PHP `$a?->b?->c`, Ruby `a&.b&.c`, JS `a?.b?.c`, C# `a?.b?.c` — must each
// contribute exactly two decision points (one short-circuit per operator),
// matching the inconsistency #281 / c8b7d93 set out to remove and the
// Ruby/Groovy fix in #452.
//
// Each operator is short-circuit (it skips the member access/call when the
// LHS is null/nil), so standard CCN counts it like `&&` / `||`. Kotlin /
// JS / TS / Groovy match the `?.` token (`QMARKDOT`), PHP matches `?->`
// (`QMARKDASHGT`), Ruby matches `&.` (`AMPDOT`), and C# matches the
// `ConditionalAccessExpression` node — every granularity fires once per
// textual operator, so the chain is +2 everywhere.
//
// Per-language post-offset expectation: unit(1) + fn(1) + 2 ops = 4.

#[test]
fn safe_navigation_chain_parity() {
    let mut sums = BTreeMap::new();
    sums.insert(
        "kotlin",
        ccn_sum(LANG::Kotlin, "fun f(a: A?): C? { return a?.b?.c }\n", "kt"),
    );
    sums.insert(
        "groovy",
        ccn_sum(LANG::Groovy, "def f(a){ return a?.b?.c }\n", "groovy"),
    );
    sums.insert(
        "php",
        ccn_sum(
            LANG::Php,
            "<?php function f($a){ return $a?->b?->c; }\n",
            "php",
        ),
    );
    sums.insert(
        "ruby",
        ccn_sum(LANG::Ruby, "def f(a); a&.b&.c; end\n", "rb"),
    );
    sums.insert(
        "javascript",
        ccn_sum(LANG::Javascript, "function f(a){ return a?.b?.c; }\n", "js"),
    );
    sums.insert(
        "csharp",
        ccn_sum(
            LANG::Csharp,
            "public class Parity { static object F(Parity a){ return a?.b?.c; } }\n",
            "cs",
        ),
    );
    // C# requires the function inside a class, adding one FuncSpace (+1),
    // identical to the Java structural offset used elsewhere in this file.
    let offsets = BTreeMap::from([("csharp", JAVA_CLASS_OFFSET)]);
    assert_parity("safe_navigation_chain", &sums, &offsets);

    // Anchor the absolute magnitude, not just mutual agreement: a shared
    // regression that dropped both `?.` links symmetrically across every
    // language would still satisfy `assert_parity` (all would agree on the
    // wrong value). Pin the hand-derived spec value so the +2 magnitude
    // itself is guarded (lessons #6 / #23): unit(1) + fn(1) + 2 ops = 4
    // for the five flat fixtures; C#'s extra class FuncSpace is removed by
    // JAVA_CLASS_OFFSET, normalising it to 4 as well.
    assert_normalised(&sums, &offsets, 4.0, "safe_navigation_chain");
}
