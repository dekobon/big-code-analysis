#![allow(
    clippy::cast_precision_loss,
    clippy::float_cmp,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]

//! Cross-language parity test for the **early-exit** (`nexits`) metric.
//!
//! Per lesson 11 in `docs/development/lessons_learned.md`, the same
//! logical construct must produce the same metric value across every
//! language we support — per-language snapshot suites cannot detect
//! disagreement between languages. This file is the exit-metric
//! companion to `tests/parity/cyclomatic_cross_language_parity.rs`.
//!
//! ## Why a single `return` fixture is not enough (#945)
//!
//! `return` is the one exit construct every language's `Exit` impl
//! counts identically, so a fixture that uses only `return` is
//! *trivially* in parity and cannot detect the divergences that are
//! the real cross-language inconsistencies (#779: Go `panic`, Lua
//! `error`/`os.exit`; and the `throw`/`raise`/process-`exit` family).
//! Each fixture below therefore exercises **two** exits: a plain
//! `return` *and* the language's abrupt-exit construct (`throw` /
//! `raise` / `panic` / `error` / process-`exit`), or — for languages
//! whose only modelled exit is `return` — two `return`s. Every fixture
//! asserts `nexits_sum() == 2`, so a regression that stops counting
//! any one language's abrupt-exit construct drops that language to `1`
//! and fails here.
//!
//! The test uses `nexits_sum()` (file-level sum over function spaces;
//! with one function per fixture it equals that function's exit count),
//! which counts every exit in the body rather than the per-function
//! `max`, so both exits in the body contribute.
//!
//! ## Coverage
//!
//! All 23 languages with an `Exit` impl in `src/metrics/nexits.rs` are
//! covered: Python, Mozjs, Javascript, Typescript, Tsx, Cpp, Mozcpp,
//! C, Objc, Java, Groovy, Rust, Csharp, Go, Perl, Kotlin, Lua, Bash,
//! Tcl, Irules, Php, Ruby, Elixir.
//!
//! Per-language exit set (see `src/metrics/nexits.rs`):
//!
//! - `return` + abrupt-exit (sum == 2): Rust (`return` + `?`),
//!   Cpp/Mozcpp/Objc/Java/Groovy/Csharp/Kotlin/JS-family (`return` +
//!   `throw`), Python (`return` + `raise`), Php (`return` + `throw`),
//!   Bash (`return` + `exit`), Go (`return` + `panic`),
//!   Lua (`return` + `error`), Ruby (`return` + `raise`),
//!   Perl (`return` + `die`), Tcl and Irules (`return` + `error`).
//! - Abrupt-exit only, no `return` node: Elixir (`raise` + `throw`,
//!   both `Call`-target text matches — Elixir has no `return`).
//! - `return`-only languages (two `return`s, sum == 2): C — no
//!   exceptions, so `return` is the whole exit set.
//!
//! Ruby, Perl, Tcl and Irules used to sit in that last bucket, with two
//! `return`s each. That was never a language property: their abrupt-exit
//! builtins have no dedicated grammar node, and their `Exit` impls
//! simply did not text-match the callee the way Go / Lua / Elixir
//! already did. #1270 fixed the impls; the fixtures moved with them, so
//! this file exercises the new arms instead of documenting their
//! absence.

use big_code_analysis::{LANG, MetricsOptions, Source, analyze};

/// Exit-count file-level sum for the single function in `source`.
fn nexits_sum(lang: LANG, source: &str, ext: &str) -> f64 {
    let name = format!("parity.{ext}");
    let space = analyze(
        Source::new(lang, source.as_bytes()).with_name(Some(name)),
        MetricsOptions::default(),
    )
    .expect("parser produced no FuncSpace for parity fixture");
    space.metrics.nexits.nexits_sum() as f64
}

#[test]
fn return_plus_abrupt_exit_parity() {
    // Rust: explicit `return` + the `?` operator (TryExpression) are the
    // two counted exits; the implicit final expression is not an exit.
    let rust = nexits_sum(
        LANG::Rust,
        r"fn f(x: bool) -> Result<i32, ()> {
    if x {
        return Ok(0);
    }
    Ok(g()?)
}
",
        "rs",
    );
    // C has no exceptions: two `return`s are the only exit form.
    let c = nexits_sum(
        LANG::C,
        r"int f(int x) {
    if (x) {
        return 1;
    }
    return 0;
}
",
        "c",
    );
    // C++ counts `return` + `throw`.
    let cpp = nexits_sum(
        LANG::Cpp,
        r"int f(int x) {
    if (x) {
        throw 1;
    }
    return 0;
}
",
        "cpp",
    );
    // Mozcpp mirrors Cpp's `return` + `throw` set.
    let mozcpp = nexits_sum(
        LANG::Mozcpp,
        r"int f(int x) {
    if (x) {
        throw 1;
    }
    return 0;
}
",
        "cpp",
    );
    // Objective-C adds `@throw` on top of C's `return`.
    let objc = nexits_sum(
        LANG::Objc,
        r#"int f(int x) {
    if (x) {
        @throw @"boom";
    }
    return 0;
}
"#,
        "m",
    );
    // Java counts `return` + `throw`.
    let java = nexits_sum(
        LANG::Java,
        r"class Parity {
    static int f(boolean x) {
        if (x) {
            throw new RuntimeException();
        }
        return 0;
    }
}
",
        "java",
    );
    // Groovy mirrors Java's `return` + `throw` set.
    let groovy = nexits_sum(
        LANG::Groovy,
        r"def f(x) {
    if (x) {
        throw new RuntimeException()
    }
    return 0
}
",
        "groovy",
    );
    // C# counts `return` + `throw` (statement form).
    let csharp = nexits_sum(
        LANG::Csharp,
        r"class Parity {
    static int F(bool x) {
        if (x) {
            throw new System.Exception();
        }
        return 0;
    }
}
",
        "cs",
    );
    // Kotlin counts `return` + `throw`.
    let kotlin = nexits_sum(
        LANG::Kotlin,
        r#"fun f(x: Boolean): Int {
    if (x) {
        throw RuntimeException("boom")
    }
    return 0
}
"#,
        "kt",
    );
    // JavaScript counts `return` + `throw`.
    let javascript = nexits_sum(
        LANG::Javascript,
        r#"function f(x) {
    if (x) {
        throw new Error("boom");
    }
    return 0;
}
"#,
        "js",
    );
    // Mozjs mirrors JavaScript's `return` + `throw` set.
    let mozjs = nexits_sum(
        LANG::Mozjs,
        r#"function f(x) {
    if (x) {
        throw new Error("boom");
    }
    return 0;
}
"#,
        "js",
    );
    // TypeScript counts `return` + `throw`.
    let typescript = nexits_sum(
        LANG::Typescript,
        r#"function f(x: boolean): number {
    if (x) {
        throw new Error("boom");
    }
    return 0;
}
"#,
        "ts",
    );
    // Tsx mirrors TypeScript's `return` + `throw` set.
    let tsx = nexits_sum(
        LANG::Tsx,
        r#"function f(x: boolean): number {
    if (x) {
        throw new Error("boom");
    }
    return 0;
}
"#,
        "tsx",
    );
    // Python counts `return` + `raise`.
    let python = nexits_sum(
        LANG::Python,
        r"def f(x):
    if x:
        raise ValueError()
    return 0
",
        "py",
    );
    // PHP counts `return` + `throw` (throw expression in statement
    // position).
    let php = nexits_sum(
        LANG::Php,
        r"<?php
function f($x) {
    if ($x) {
        throw new \Exception('boom');
    }
    return 0;
}
",
        "php",
    );
    // Bash has no `return_statement` node: `return` and `exit` are
    // builtins matched by command-name text.
    let bash = nexits_sum(
        LANG::Bash,
        r#"#!/bin/bash
f() {
    if [ "$1" -eq 1 ]; then
        exit 1
    fi
    return 0
}
"#,
        "sh",
    );
    // Go has no `throw`: `panic(...)` is the built-in abrupt-exit call
    // (#779), matched by callee text alongside `return`.
    let go = nexits_sum(
        LANG::Go,
        r#"package p
func f(x bool) int {
    if x {
        panic("boom")
    }
    return 0
}
"#,
        "go",
    );
    // Lua has no `throw`: `error(...)` raises and unwinds the stack
    // (#779), matched by callee text alongside `return`.
    let lua = nexits_sum(
        LANG::Lua,
        r#"local function f(x)
    if x then
        error("boom")
    end
    return 0
end
"#,
        "lua",
    );
    // Elixir has no `return`: both exits are abrupt-exit `Call`s whose
    // target text spells `raise` / `throw`.
    let elixir = nexits_sum(
        LANG::Elixir,
        "defmodule Foo do\n  def f(x) do\n    if x do\n      raise \"boom\"\n    end\n    throw(:done)\n  end\nend\n",
        "ex",
    );
    // Perl has no `throw`: `die` raises and unwinds to the nearest
    // `eval` (#1270), matched by bareword-callee text alongside
    // `return`.
    let perl = nexits_sum(
        LANG::Perl,
        r#"sub f {
    if ($_[0]) {
        die "boom";
    }
    return 0;
}
"#,
        "pl",
    );
    // Ruby counts `return` + `raise`: `raise` has no grammar node of
    // its own, so it is matched as a receiver-less `call` whose method
    // identifier spells the builtin (#1270).
    let ruby = nexits_sum(
        LANG::Ruby,
        r#"def f(x)
    if x
        raise ArgumentError, "boom"
    end
    return 0
end
"#,
        "rb",
    );
    // Tcl has no `return` keyword node and no dedicated `error` rule
    // either: both are generic Commands told apart by their leading
    // word (#1270).
    let tcl = nexits_sum(
        LANG::Tcl,
        r#"proc f {x} {
    if {$x > 0} {
        error "boom"
    }
    return nonpositive
}
"#,
        "tcl",
    );
    // iRules mirrors Tcl for `return` + `error` (both generic
    // Commands). Tcl 8.6's `throw` is deliberately absent from the
    // iRules exit set — TMOS runs a Tcl 8.4-derived interpreter that
    // has no such builtin — so the fixture uses `error`.
    let irules = nexits_sum(
        LANG::Irules,
        r#"proc f {x} {
    if {$x > 0} {
        error "boom"
    }
    return nonpositive
}
"#,
        "irul",
    );

    // expected: every fixture has exactly two counted exits — one
    // `return` plus one abrupt-exit construct (or two `return`s where
    // that is the only modelled exit). A regression that stops counting
    // a language's abrupt-exit construct drops it to 1 and fails here.
    let expected = 2.0;
    assert_eq!(rust, expected, "rust");
    assert_eq!(c, expected, "c");
    assert_eq!(cpp, expected, "cpp");
    assert_eq!(mozcpp, expected, "mozcpp");
    assert_eq!(objc, expected, "objc");
    assert_eq!(java, expected, "java");
    assert_eq!(groovy, expected, "groovy");
    assert_eq!(csharp, expected, "csharp");
    assert_eq!(kotlin, expected, "kotlin");
    assert_eq!(javascript, expected, "javascript");
    assert_eq!(mozjs, expected, "mozjs");
    assert_eq!(typescript, expected, "typescript");
    assert_eq!(tsx, expected, "tsx");
    assert_eq!(python, expected, "python");
    assert_eq!(php, expected, "php");
    assert_eq!(bash, expected, "bash");
    assert_eq!(go, expected, "go");
    assert_eq!(lua, expected, "lua");
    assert_eq!(elixir, expected, "elixir");
    assert_eq!(perl, expected, "perl");
    assert_eq!(ruby, expected, "ruby");
    assert_eq!(tcl, expected, "tcl");
    assert_eq!(irules, expected, "irules");
}
