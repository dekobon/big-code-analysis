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
//! The fixture table is an exhaustive `match` on [`LANG`], so the
//! coverage claim is structural: adding a language variant fails to
//! compile until someone decides whether it has an `Exit` impl to
//! exercise, and a variant that has none says so with `None`. Before
//! #1281 the claim was a prose list ("all 23 languages …") with nothing
//! holding it to the roster.
//!
//! The per-language exit set (see `src/metrics/nexits.rs`) lives at each
//! arm. In summary:
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
    .unwrap_or_else(|e| panic!("{lang:?}: analyze failed: {e}"));
    space.metrics.nexits.nexits_sum() as f64
}

/// Returns `(source, extension)` for a language with an `Exit` impl, or
/// `None` for one that has no function concept to exit from.
///
/// Every `Some` row spells exactly two counted exits: a plain `return`
/// plus the language's abrupt-exit construct, or two `return`s where
/// that is the only modelled exit.
fn fixture(lang: LANG) -> Option<(&'static str, &'static str)> {
    // Exhaustive per-language dispatch table: one arm per LANG variant
    // is the point of this function, so a new language cannot be added
    // without deciding what its exit set is. The repo's own `.bcaignore`
    // excludes `./tests/**`, so this marker is for the per-edit
    // `bca check` hook rather than for the self-scan gate.
    // bca: suppress(cyclomatic)
    let row = match lang {
        // Rust: explicit `return` + the `?` operator (TryExpression) are
        // the two counted exits; the implicit final expression is not an
        // exit.
        LANG::Rust => (
            "fn f(x: bool) -> Result<i32, ()> {\n    if x {\n        return Ok(0);\n    }\n    \
             Ok(g()?)\n}\n",
            "rs",
        ),
        // C has no exceptions: two `return`s are the only exit form.
        LANG::C => (
            "int f(int x) {\n    if (x) {\n        return 1;\n    }\n    return 0;\n}\n",
            "c",
        ),
        // C++ counts `return` + `throw`; Mozcpp mirrors its set.
        LANG::Cpp | LANG::Mozcpp => (
            "int f(int x) {\n    if (x) {\n        throw 1;\n    }\n    return 0;\n}\n",
            "cpp",
        ),
        // Objective-C adds `@throw` on top of C's `return`.
        LANG::Objc => (
            "int f(int x) {\n    if (x) {\n        @throw @\"boom\";\n    }\n    return 0;\n}\n",
            "m",
        ),
        // Java counts `return` + `throw`.
        LANG::Java => (
            "class Parity {\n    static int f(boolean x) {\n        if (x) {\n            \
             throw new RuntimeException();\n        }\n        return 0;\n    }\n}\n",
            "java",
        ),
        // Groovy mirrors Java's `return` + `throw` set.
        LANG::Groovy => (
            "def f(x) {\n    if (x) {\n        throw new RuntimeException()\n    }\n    \
             return 0\n}\n",
            "groovy",
        ),
        // C# counts `return` + `throw` (statement form).
        LANG::Csharp => (
            "class Parity {\n    static int F(bool x) {\n        if (x) {\n            \
             throw new System.Exception();\n        }\n        return 0;\n    }\n}\n",
            "cs",
        ),
        // Kotlin counts `return` + `throw`.
        LANG::Kotlin => (
            "fun f(x: Boolean): Int {\n    if (x) {\n        throw RuntimeException(\"boom\")\n    \
             }\n    return 0\n}\n",
            "kt",
        ),
        // JavaScript counts `return` + `throw`; Mozjs mirrors its set.
        LANG::Javascript | LANG::Mozjs => (
            "function f(x) {\n    if (x) {\n        throw new Error(\"boom\");\n    }\n    \
             return 0;\n}\n",
            "js",
        ),
        // TypeScript counts `return` + `throw`; Tsx mirrors its set.
        LANG::Typescript | LANG::Tsx => (
            "function f(x: boolean): number {\n    if (x) {\n        \
             throw new Error(\"boom\");\n    }\n    return 0;\n}\n",
            "ts",
        ),
        // Python counts `return` + `raise`.
        LANG::Python => (
            "def f(x):\n    if x:\n        raise ValueError()\n    return 0\n",
            "py",
        ),
        // PHP counts `return` + `throw` (throw expression in statement
        // position).
        LANG::Php => (
            "<?php\nfunction f($x) {\n    if ($x) {\n        throw new \\Exception('boom');\n    \
             }\n    return 0;\n}\n",
            "php",
        ),
        // Bash has no `return_statement` node: `return` and `exit` are
        // builtins matched by command-name text.
        LANG::Bash => (
            "#!/bin/bash\nf() {\n    if [ \"$1\" -eq 1 ]; then\n        exit 1\n    fi\n    \
             return 0\n}\n",
            "sh",
        ),
        // Go has no `throw`: `panic(...)` is the built-in abrupt-exit
        // call (#779), matched by callee text alongside `return`.
        LANG::Go => (
            "package p\nfunc f(x bool) int {\n    if x {\n        panic(\"boom\")\n    }\n    \
             return 0\n}\n",
            "go",
        ),
        // Lua has no `throw`: `error(...)` raises and unwinds the stack
        // (#779), matched by callee text alongside `return`.
        LANG::Lua => (
            "local function f(x)\n    if x then\n        error(\"boom\")\n    end\n    \
             return 0\nend\n",
            "lua",
        ),
        // Elixir has no `return`: both exits are abrupt-exit `Call`s
        // whose target text spells `raise` / `throw`.
        LANG::Elixir => (
            "defmodule Foo do\n  def f(x) do\n    if x do\n      raise \"boom\"\n    end\n    \
             throw(:done)\n  end\nend\n",
            "ex",
        ),
        // Perl has no `throw`: `die` raises and unwinds to the nearest
        // `eval` (#1270), matched by bareword-callee text alongside
        // `return`.
        LANG::Perl => (
            "sub f {\n    if ($_[0]) {\n        die \"boom\";\n    }\n    return 0;\n}\n",
            "pl",
        ),
        // Ruby counts `return` + `raise`: `raise` has no grammar node of
        // its own, so it is matched as a receiver-less `call` whose
        // method identifier spells the builtin (#1270).
        LANG::Ruby => (
            "def f(x)\n    if x\n        raise ArgumentError, \"boom\"\n    end\n    \
             return 0\nend\n",
            "rb",
        ),
        // Tcl has no `return` keyword node and no dedicated `error` rule
        // either: both are generic Commands told apart by their leading
        // word (#1270).
        LANG::Tcl => (
            "proc f {x} {\n    if {$x > 0} {\n        error \"boom\"\n    }\n    \
             return nonpositive\n}\n",
            "tcl",
        ),
        // iRules mirrors Tcl for `return` + `error` (both generic
        // Commands). Tcl 8.6's `throw` is deliberately absent from the
        // iRules exit set — TMOS runs a Tcl 8.4-derived interpreter that
        // has no such builtin — so the fixture uses `error`.
        LANG::Irules => (
            "proc f {x} {\n    if {$x > 0} {\n        error \"boom\"\n    }\n    \
             return nonpositive\n}\n",
            "irul",
        ),
        // The two C-family helper grammars parse fragments — a comment
        // body, a preprocessor directive — with no function to exit
        // from and no `Exit` impl in `src/metrics/nexits.rs`.
        LANG::Ccomment | LANG::Preproc => return None,
    };
    Some(row)
}

#[test]
fn return_plus_abrupt_exit_parity() {
    // expected: every fixture has exactly two counted exits — one
    // `return` plus one abrupt-exit construct (or two `return`s where
    // that is the only modelled exit). Hand-derived by reading each
    // fixture: a regression that stops counting a language's
    // abrupt-exit construct drops it to 1 and fails here.
    let expected = 2.0;

    let mut checked = 0;
    for lang in LANG::into_enum_iter() {
        if !lang.is_enabled() {
            continue;
        }
        let Some((source, ext)) = fixture(lang) else {
            continue;
        };
        checked += 1;
        let got = nexits_sum(lang, source, ext);
        assert_eq!(
            got, expected,
            "{lang:?}: nexits {got} != expected {expected}"
        );
    }

    // A table that lost its rows is a real, feature-independent
    // regression, so assert it directly. A build enabling only the two
    // helper grammars legitimately checks nothing — the state a
    // `#[cfg]`-gated-out module would be in — so `checked > 0` is
    // deliberately not asserted: making it safe would need a
    // hand-maintained feature union naming every `Some` arm, which is
    // the drift this restructure removed (#1281).
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
