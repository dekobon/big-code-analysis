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
use std::path::PathBuf;

use big_code_analysis::{LANG, get_function_spaces};

/// Parses `source` as `lang` and returns the file-level standard CCN sum.
fn ccn_sum(lang: LANG, source: &str, ext: &str) -> f64 {
    let path = PathBuf::from(format!("parity.{ext}"));
    let space = get_function_spaces(&lang, source.as_bytes().to_vec(), &path, None)
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
    let offsets = BTreeMap::from([("java", JAVA_CLASS_OFFSET)]);
    assert_parity("switch_with_default", &sums, &offsets);
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
    let offsets = BTreeMap::from([("java", JAVA_CLASS_OFFSET)]);
    assert_parity("switch_without_default", &sums, &offsets);
}

// --- Family 3: if / else if / else chain -----------------------------------
//
// A three-condition chain (one `if`, two `else if`, one `else`). Each
// condition contributes one decision point. JavaScript and Python join
// here; Bash uses `elif` which is counted via `ElifClause`.
//
// Per-language post-offset expectation: unit(1) + fn(1) + 3 conditions = 5.
//
// Python carries an additional +1 offset for this family. `impl
// Cyclomatic for PythonCode` (`src/metrics/cyclomatic.rs:195`) calls
// `node.has_ancestors(For|While, ElseClause)` from an `Else` token to
// detect Python's loop-`else` feature. `Node::has_ancestors`
// (`src/node.rs:175`) only walks up when the first predicate matches,
// so as wired here the check evaluates to "is the parent of `Else` an
// `else_clause`" — which is true for **every** `else:` in Python,
// including the `else` of an `if`/`elif`/`else` chain. This is a
// pre-existing standard-CCN bug surfaced by the parity test. Drop
// this offset once the Python impl is fixed.
const PYTHON_ELSE_BUG_OFFSET: f64 = 1.0;

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
    let offsets = BTreeMap::from([
        ("java", JAVA_CLASS_OFFSET),
        ("python", PYTHON_ELSE_BUG_OFFSET),
    ]);
    assert_parity("if_else_if_else_chain", &sums, &offsets);
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
    let offsets = BTreeMap::from([("java", JAVA_CLASS_OFFSET)]);
    assert_parity("single_if_no_else", &sums, &offsets);
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
    let offsets = BTreeMap::from([("java", JAVA_CLASS_OFFSET)]);
    assert_parity("two_arm_switch_with_wildcard", &sums, &offsets);
}
