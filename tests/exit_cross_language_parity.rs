#![allow(
    clippy::float_cmp,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]

//! Cross-language parity test for the **early-exit** metric.
//!
//! Per lesson 11 in `docs/development/lessons_learned.md`, the same
//! logical construct must produce the same metric value across every
//! language we support — per-language snapshot suites cannot detect
//! disagreement between languages. This file is the exit-metric
//! companion to `tests/cyclomatic_cross_language_parity.rs`.
//!
//! The fixture is a `while` loop whose body is an `if`-guarded
//! `return`: one and only one early exit. Every supported language
//! tested below reports `exit_max() == 1`.
//!
//! The test uses `exit_max()` (space-stacking-independent) so Java's
//! mandatory wrapping class does not skew the comparison — no
//! per-language offset is required.

use std::path::PathBuf;

use big_code_analysis::{LANG, get_function_spaces};

/// Exit max for the single function in `source`.
fn exit_max(lang: LANG, source: &str, ext: &str) -> f64 {
    let path = PathBuf::from(format!("parity.{ext}"));
    let space = get_function_spaces(&lang, source.as_bytes().to_vec(), &path, None)
        .expect("parser produced no FuncSpace for parity fixture");
    space.metrics.nexits.exit_max()
}

#[test]
fn early_exit_in_while_loop_parity() {
    let rust = exit_max(
        LANG::Rust,
        r"fn f(x: bool, y: bool) {
    while x {
        if y {
            return;
        }
    }
}
",
        "rs",
    );
    let c = exit_max(
        LANG::Cpp,
        r"void f(int x, int y) {
    while (x) {
        if (y) {
            return;
        }
    }
}
",
        "c",
    );
    let java = exit_max(
        LANG::Java,
        r"class Parity {
    static void f(boolean x, boolean y) {
        while (x) {
            if (y) {
                return;
            }
        }
    }
}
",
        "java",
    );
    let javascript = exit_max(
        LANG::Javascript,
        r"function f(x, y) {
    while (x) {
        if (y) {
            return;
        }
    }
}
",
        "js",
    );
    let typescript = exit_max(
        LANG::Typescript,
        r"function f(x: boolean, y: boolean) {
    while (x) {
        if (y) {
            return;
        }
    }
}
",
        "ts",
    );
    let csharp = exit_max(
        LANG::Csharp,
        r"class Parity {
    static void F(bool x, bool y) {
        while (x) {
            if (y) {
                return;
            }
        }
    }
}
",
        "cs",
    );
    let php = exit_max(
        LANG::Php,
        r"<?php
function f($x, $y) {
    while ($x) {
        if ($y) {
            return;
        }
    }
}
",
        "php",
    );
    let python = exit_max(
        LANG::Python,
        r"def f(x, y):
    while x:
        if y:
            return
",
        "py",
    );
    let bash = exit_max(
        LANG::Bash,
        r#"#!/bin/bash
f() {
    while [ "$1" -eq 1 ]; do
        if [ "$2" -eq 1 ]; then
            return
        fi
    done
}
"#,
        "sh",
    );
    let kotlin = exit_max(
        LANG::Kotlin,
        r"fun f(x: Boolean, y: Boolean) {
    while (x) {
        if (y) {
            return
        }
    }
}
",
        "kt",
    );
    // Go has no `while`; `for` with a single condition is the idiomatic
    // equivalent and parses to the same loop-body structure.
    let go = exit_max(
        LANG::Go,
        r"package p
func f(x bool, y bool) {
    for x {
        if y {
            return
        }
    }
}
",
        "go",
    );

    // expected: one `return` reachable along the while/if path is
    // a single early exit per function.
    let expected = 1.0;
    assert_eq!(rust, expected, "rust");
    assert_eq!(c, expected, "c");
    assert_eq!(java, expected, "java");
    assert_eq!(javascript, expected, "javascript");
    assert_eq!(typescript, expected, "typescript");
    assert_eq!(csharp, expected, "csharp");
    assert_eq!(php, expected, "php");
    assert_eq!(python, expected, "python");
    assert_eq!(bash, expected, "bash");
    assert_eq!(kotlin, expected, "kotlin");
    assert_eq!(go, expected, "go");
}
