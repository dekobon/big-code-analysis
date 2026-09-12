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
//! Each construct family below is an exhaustive `match` on [`LANG`]
//! returning a [`Fixture`]: one arm per variant, so adding a language
//! fails to compile until someone decides whether it spells the family's
//! construct, and a language that does not says so with `None` and a
//! reason at the arm. Before #1281 the families were hand-maintained
//! `BTreeMap` inserts that had fallen behind the roster — Ruby, Lua,
//! Perl and Go were absent from families they can express, and every row
//! labelled `"c"` actually parsed [`LANG::Cpp`].
//!
//! [`check_family`] then builds a `BTreeMap<&str, f64>` keyed by
//! language with its standard-CCN sum and asserts every value is equal
//! **after subtracting a small set of explicitly-documented per-language
//! offsets** that account for intrinsic, language-mandated structural
//! differences (e.g. Java's requirement that every function live inside
//! a class, which adds a `FuncSpace` and therefore +1 to every sum). On
//! failure the full before/after maps are printed so any drift and the
//! language that drifted are immediately visible.
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
    .unwrap_or_else(|e| panic!("{lang:?}: analyze failed: {e}"));
    space.metrics.cyclomatic.cyclomatic_sum() as f64
}

/// Some languages cannot spell a free function: Java and C# require a
/// wrapping class, Elixir a wrapping `defmodule`. The `FuncSpace`
/// traversal counts that container as an extra space and so adds +1 to
/// every CCN sum. This is a language-mandated structural difference, not
/// a metric bug, and it is the only offset any family uses.
const CONTAINER_SPACE_OFFSET: f64 = 1.0;

/// One language's fixture for a construct family.
struct Fixture {
    /// Source spelling the family's construct in this language.
    source: &'static str,
    /// Extension used to name the parsed unit. It reaches the file-level
    /// space name and no metric, so it is cosmetic — but it keeps a
    /// failure message readable.
    ext: &'static str,
    /// Structural spaces the language forces around the fixture, removed
    /// before comparing. Living on the fixture rather than in a separate
    /// table keeps the offset next to the source that causes it.
    offset: f64,
}

/// A fixture whose function sits at file scope, so nothing is subtracted.
fn flat(source: &'static str, ext: &'static str) -> Fixture {
    Fixture {
        source,
        ext,
        offset: 0.0,
    }
}

/// A fixture whose language requires the function to live inside a class
/// or module, worth [`CONTAINER_SPACE_OFFSET`].
fn in_container(source: &'static str, ext: &'static str) -> Fixture {
    Fixture {
        source,
        ext,
        offset: CONTAINER_SPACE_OFFSET,
    }
}

/// Asserts every language in `sums` reports the same CCN value after
/// subtracting `offsets[lang]` (default 0). Both maps and the
/// normalised values are printed on failure so the drift is obvious.
///
/// This alone is **not** a sufficient check: a build with one enabled
/// language reaches it with a single-entry map, which has one unique
/// value by construction. [`assert_normalised`] is what pins the
/// magnitude, and every family calls both.
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

/// Measures every enabled language that has a row in `fixture`, then
/// runs both halves of the family's claim: mutual agreement
/// ([`assert_parity`]) and absolute magnitude ([`assert_normalised`]).
///
/// One driver for all eight families is what makes "every family still
/// pins its magnitude" structural rather than something a reviewer has
/// to re-check per family.
#[track_caller]
fn check_family(family: &str, expected: f64, fixture: fn(LANG) -> Option<Fixture>) {
    let mut sums = BTreeMap::new();
    let mut offsets = BTreeMap::new();

    for lang in LANG::into_enum_iter() {
        if !lang.is_enabled() {
            continue;
        }
        let Some(row) = fixture(lang) else {
            continue;
        };
        sums.insert(lang.name(), ccn_sum(lang, row.source, row.ext));
        if row.offset != 0.0 {
            offsets.insert(lang.name(), row.offset);
        }
    }

    // A table that lost its rows is a real, feature-independent
    // regression, so assert it directly. A build whose enabled languages
    // cannot all spell this family's construct legitimately checks
    // nothing — the state a `#[cfg]`-gated-out module would be in — so
    // a non-empty `sums` is deliberately not asserted: making that safe
    // would need a hand-maintained feature union naming every `Some`
    // arm, which is the drift this restructure removed (#1281).
    assert!(
        LANG::into_enum_iter().any(|lang| fixture(lang).is_some()),
        "{family}: the fixture table has no rows at all, so it cannot fail",
    );
    // Re-derive the count independently of the loop above, so a
    // `continue` added there cannot quietly drop rows.
    let eligible = LANG::into_enum_iter()
        .filter(|lang| lang.is_enabled() && fixture(*lang).is_some())
        .count();
    assert_eq!(
        sums.len(),
        eligible,
        "{family}: measured {} languages but {eligible} enabled languages have a fixture",
        sums.len(),
    );
    if sums.is_empty() {
        // Nothing enabled can spell this construct. `assert_parity`
        // would see zero distinct values and report drift, so a build
        // that measured nothing would fail as though it had found a
        // disagreement.
        return;
    }

    assert_parity(family, &sums, &offsets);
    assert_normalised(&sums, &offsets, expected, family);
}

// --- Family 1: switch / match with fallback arm ----------------------------
//
// Semantically equivalent fixtures, each defining a single function that
// switches an integer against three explicit arms plus a fallback
// (`default:` in C-family, `_ =>` in Rust, `else` in Ruby/Kotlin).
// Standard CCN counts the three explicit arms and skips the fallback
// (Rust treats bare `_ =>` like C-family `default:` after #106 /
// `a54b073`).
//
// Per-language post-offset expectation: unit(1) + fn(1) + 3 explicit arms = 5.

fn switch_with_default(lang: LANG) -> Option<Fixture> {
    // Exhaustive per-language dispatch table: one arm per LANG variant
    // is the point of this function, so a new language cannot be added
    // without deciding whether it spells the construct. The repo's own
    // `.bcaignore` excludes `./tests/**`, so this marker is for the
    // per-edit `bca check` hook rather than for the self-scan gate.
    // bca: suppress(cyclomatic)
    let row = match lang {
        LANG::Rust => flat(
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
        // One arm for the whole C family: the fixture is plain C, which
        // all four grammars accept unchanged. Objective-C reaches it
        // through a free C `function_definition`, valid in a `.m` file,
        // so it needs no container offset either.
        LANG::C | LANG::Cpp | LANG::Mozcpp | LANG::Objc => flat(
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
        LANG::Java => in_container(
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
        // C# switch-expression with discard arm `_ =>` (issue #282).
        LANG::Csharp => in_container(
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
        // Kotlin `when` with `else ->` arm (issue #282).
        LANG::Kotlin => flat(
            r#"fun f(x: Int): String = when (x) {
    1 -> "one"
    2 -> "two"
    3 -> "three"
    else -> "other"
}
"#,
            "kt",
        ),
        LANG::Javascript | LANG::Mozjs => flat(
            r"function f(x) {
    switch (x) {
        case 1: break;
        case 2: break;
        case 3: break;
        default: break;
    }
}
",
            "js",
        ),
        LANG::Typescript => flat(
            r"function f(x: number) {
    switch (x) {
        case 1: break;
        case 2: break;
        case 3: break;
        default: break;
    }
}
",
            "ts",
        ),
        LANG::Tsx => flat(
            r"function f(x: number) {
    switch (x) {
        case 1: break;
        case 2: break;
        case 3: break;
        default: break;
    }
}
",
            "tsx",
        ),
        LANG::Php => flat(
            r"<?php
function f($x) {
    switch ($x) {
        case 1: break;
        case 2: break;
        case 3: break;
        default: break;
    }
}
",
            "php",
        ),
        // Python's structural-pattern `match`; `case _` is the wildcard.
        LANG::Python => flat(
            r"def f(x):
    match x:
        case 1:
            return 'one'
        case 2:
            return 'two'
        case 3:
            return 'three'
        case _:
            return 'other'
",
            "py",
        ),
        LANG::Go => flat(
            r#"package p
func f(x int) string {
    switch x {
    case 1:
        return "one"
    case 2:
        return "two"
    case 3:
        return "three"
    default:
        return "other"
    }
}
"#,
            "go",
        ),
        // Ruby spells the construct `case`/`when` with `else` as the
        // fallback arm, and `def` is top-level, so no container offset.
        LANG::Ruby => flat(
            r#"def f(x)
  case x
  when 1 then "one"
  when 2 then "two"
  when 3 then "three"
  else "other"
  end
end
"#,
            "rb",
        ),
        LANG::Groovy => flat(
            r"def f(x) {
    switch (x) {
        case 1: break
        case 2: break
        case 3: break
        default: break
    }
}
",
            "groovy",
        ),
        // Bash's `*)` is the catch-all arm and is free, matching the
        // C-family `default:`; each explicit arm counts exactly once
        // after the #107 / `e668f14` fix.
        LANG::Bash => flat(
            r#"#!/bin/bash
f() {
    case "$1" in
        one)   echo 1 ;;
        two)   echo 2 ;;
        three) echo 3 ;;
        *)     echo 0 ;;
    esac
}
"#,
            "sh",
        ),
        // Tcl `switch` is a generic command whose non-`default` arms are
        // the decision points; `default` is free, matching the C-family
        // `default:` (issue #467). Procs are top-level, so no offset.
        LANG::Tcl => flat(
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
        // iRules `switch` is a dedicated node (unlike Tcl's command
        // form), but counts the same: 3 non-`default` arms, fallback
        // free.
        LANG::Irules => flat(
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
        // Elixir `case` with a bare `_ ->` catch-all: the wildcard is
        // the construct's default arm and is skipped, like Rust's
        // `_ =>` (issue #1272).
        LANG::Elixir => in_container(
            r"defmodule Parity do
  def f(x) do
    case x do
      1 -> :one
      2 -> :two
      3 -> :three
      _ -> :other
    end
  end
end
",
            "ex",
        ),
        // No switch-like construct to count. Lua has none at all — the
        // idiomatic multi-way form is an `if`/`elseif` chain, covered by
        // `if_else_if_else_chain`. Perl's `given`/`when` was always
        // experimental and was removed from the language in 5.42; the
        // pinned grammar does not model it (verified: the construct
        // parses to ERROR nodes), so a fixture using it would pin the
        // parse failure rather than the metric. The two C-family helper
        // grammars parse fragments — a comment body, a preprocessor
        // directive — and have no statements at all.
        LANG::Lua | LANG::Perl | LANG::Ccomment | LANG::Preproc => return None,
    };
    Some(row)
}

#[test]
fn switch_with_default_parity() {
    // Anchor the absolute magnitude, not just mutual agreement (lessons
    // #6 / #23 / #468): a symmetric regression that dropped or added +1
    // across every language would still satisfy `assert_parity`. Pin the
    // hand-derived spec value: unit(1) + fn(1) + 3 explicit arms = 5
    // (fallback skipped post-#106).
    check_family("switch_with_default", 5.0, switch_with_default);
}

// --- Family 2: switch / case / match without fallback ----------------------
//
// Three explicit arms, no semantic fallback.
//
// Rust requires exhaustive matching, so its fixture still includes a
// bare `_ => {}` arm — but post-#106 (`a54b073`) bare wildcards are
// skipped in standard CCN, so the match still counts only the three
// explicit arms (semantically equivalent to a C `switch` whose
// unmatched values do nothing).
//
// Per-language post-offset expectation: unit(1) + fn(1) + 3 arms = 5.

fn switch_without_default(lang: LANG) -> Option<Fixture> {
    // Exhaustive per-language dispatch table; see `switch_with_default`.
    // bca: suppress(cyclomatic)
    let row = match lang {
        LANG::Rust => flat(
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
        LANG::C | LANG::Cpp | LANG::Mozcpp | LANG::Objc => flat(
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
        LANG::Java => in_container(
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
        // C# switch *statement* (not expression — switch expressions
        // require exhaustiveness so cannot express "no default").
        // Issue #282.
        LANG::Csharp => in_container(
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
        // Kotlin `when` used as a statement does not require an `else`
        // arm (only `when` as an expression with sealed types does).
        // Issue #282.
        LANG::Kotlin => flat(
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
        LANG::Javascript | LANG::Mozjs => flat(
            r"function f(x) {
    switch (x) {
        case 1: break;
        case 2: break;
        case 3: break;
    }
}
",
            "js",
        ),
        LANG::Typescript => flat(
            r"function f(x: number) {
    switch (x) {
        case 1: break;
        case 2: break;
        case 3: break;
    }
}
",
            "ts",
        ),
        LANG::Tsx => flat(
            r"function f(x: number) {
    switch (x) {
        case 1: break;
        case 2: break;
        case 3: break;
    }
}
",
            "tsx",
        ),
        LANG::Php => flat(
            r"<?php
function f($x) {
    switch ($x) {
        case 1: break;
        case 2: break;
        case 3: break;
    }
}
",
            "php",
        ),
        // Python's `match` is not required to be exhaustive, so the
        // wildcard arm can simply be omitted.
        LANG::Python => flat(
            r"def f(x):
    match x:
        case 1:
            return 'one'
        case 2:
            return 'two'
        case 3:
            return 'three'
",
            "py",
        ),
        LANG::Go => flat(
            r"package p
func f(x int) {
    switch x {
    case 1:
    case 2:
    case 3:
    }
}
",
            "go",
        ),
        LANG::Ruby => flat(
            r#"def f(x)
  case x
  when 1 then "one"
  when 2 then "two"
  when 3 then "three"
  end
end
"#,
            "rb",
        ),
        LANG::Groovy => flat(
            r"def f(x) {
    switch (x) {
        case 1: break
        case 2: break
        case 3: break
    }
}
",
            "groovy",
        ),
        // Bash joins this family because `case…esac` semantics count
        // each arm exactly once after the #107 / `e668f14` fix; with no
        // `*)` arm there is no asymmetry between Bash and the C family.
        LANG::Bash => flat(
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
        // Tcl / iRules `switch` with no `default` arm: all three arms
        // are decision points (issue #467).
        LANG::Tcl => flat(
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
        LANG::Irules => flat(
            r"proc f {x} {
    switch $x {
        1 { return one }
        2 { return two }
        3 { return three }
    }
}
",
            "irule",
        ),
        // Elixir `case` with no `_ ->` fallback: every arm is a real
        // decision and counts (issue #1272 skips only the bare
        // wildcard).
        LANG::Elixir => in_container(
            r"defmodule Parity do
  def f(x) do
    case x do
      1 -> :one
      2 -> :two
      3 -> :three
    end
  end
end
",
            "ex",
        ),
        // No switch-like construct; see `switch_with_default` for why.
        LANG::Lua | LANG::Perl | LANG::Ccomment | LANG::Preproc => return None,
    };
    Some(row)
}

#[test]
fn switch_without_default_parity() {
    // Anchor the absolute magnitude (lessons #6 / #23 / #468):
    // unit(1) + fn(1) + 3 arms = 5 (no fallback to count; Rust's bare
    // `_ => {}` skipped post-#106, Bash arms count once post-#107).
    check_family("switch_without_default", 5.0, switch_without_default);
}

// --- Family 3: if / else if / else chain -----------------------------------
//
// A three-condition chain (one `if`, two `else if`, one `else`). Each
// condition contributes one decision point; the trailing `else` is free.
//
// Per-language post-offset expectation: unit(1) + fn(1) + 3 conditions = 5.

fn if_else_if_else_chain(lang: LANG) -> Option<Fixture> {
    // Exhaustive per-language dispatch table; see `switch_with_default`.
    // bca: suppress(cyclomatic)
    let row = match lang {
        LANG::Rust => flat(
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
        LANG::C | LANG::Cpp | LANG::Mozcpp | LANG::Objc => flat(
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
        LANG::Java => in_container(
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
        LANG::Csharp => in_container(
            r"class Parity {
    static int F(int x) {
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
            "cs",
        ),
        LANG::Kotlin => flat(
            r"fun f(x: Int): Int {
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
            "kt",
        ),
        LANG::Javascript | LANG::Mozjs => flat(
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
        LANG::Typescript => flat(
            r"function f(x: number): number {
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
            "ts",
        ),
        LANG::Tsx => flat(
            r"function f(x: number): number {
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
            "tsx",
        ),
        // PHP spells the middle links `elseif` (a dedicated node rather
        // than a nested `if` inside an else clause).
        LANG::Php => flat(
            r"<?php
function f($x) {
    if ($x == 1) {
        return 10;
    } elseif ($x == 2) {
        return 20;
    } elseif ($x == 3) {
        return 30;
    } else {
        return 0;
    }
}
",
            "php",
        ),
        LANG::Python => flat(
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
        LANG::Go => flat(
            r"package p
func f(x int) int {
    if x == 1 {
        return 10
    } else if x == 2 {
        return 20
    } else if x == 3 {
        return 30
    } else {
        return 0
    }
}
",
            "go",
        ),
        // Ruby spells the middle links `elsif`.
        LANG::Ruby => flat(
            r"def f(x)
  if x == 1
    10
  elsif x == 2
    20
  elsif x == 3
    30
  else
    0
  end
end
",
            "rb",
        ),
        // Lua spells them `elseif`, one keyword.
        LANG::Lua => flat(
            r"function f(x)
    if x == 1 then
        return 10
    elseif x == 2 then
        return 20
    elseif x == 3 then
        return 30
    else
        return 0
    end
end
",
            "lua",
        ),
        // Perl spells them `elsif`, like Ruby.
        LANG::Perl => flat(
            r"sub f {
    my ($x) = @_;
    if ($x == 1) {
        return 10;
    } elsif ($x == 2) {
        return 20;
    } elsif ($x == 3) {
        return 30;
    } else {
        return 0;
    }
}
",
            "pl",
        ),
        LANG::Groovy => flat(
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
        LANG::Bash => flat(
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
        // Tcl and iRules both have a dedicated `elseif`; each is a
        // decision and the trailing `else` is free.
        LANG::Tcl => flat(
            r"proc f {x} {
    if { $x == 1 } { return 10 } elseif { $x == 2 } { return 20 } elseif { $x == 3 } { return 30 } else { return 0 }
}
",
            "tcl",
        ),
        LANG::Irules => flat(
            r"proc f {x} {
    if { $x == 1 } { return 10 } elseif { $x == 2 } { return 20 } elseif { $x == 3 } { return 30 } else { return 0 }
}
",
            "irule",
        ),
        // Elixir has no `else if` chain keyword; `cond` is the idiomatic
        // multi-way conditional, and its final `true ->` arm is the
        // designated default — the analogue of the free `else`, skipped
        // post-#1272. Three condition arms count.
        LANG::Elixir => in_container(
            r"defmodule Parity do
  def f(x) do
    cond do
      x == 1 -> 10
      x == 2 -> 20
      x == 3 -> 30
      true -> 0
    end
  end
end
",
            "ex",
        ),
        // The two C-family helper grammars parse fragments and have no
        // statements at all.
        LANG::Ccomment | LANG::Preproc => return None,
    };
    Some(row)
}

#[test]
fn if_else_if_else_chain_parity() {
    // Anchor the absolute magnitude (lessons #6 / #23 / #468):
    // unit(1) + fn(1) + 3 conditions = 5 (one `if`, two
    // `else if`/`elif`/`elsif`/`elseif`; the trailing `else` is not a
    // decision point). This family caught the #229 Python over-count, so
    // pinning the value also guards the `parent_grandparent_match` fix
    // from a symmetric regression.
    check_family("if_else_if_else_chain", 5.0, if_else_if_else_chain);
}

// --- Family 4: single `if` with no `else` ----------------------------------
//
// The simplest decision form: one branching condition, no else clause.
// No else clause means none of the per-language `else`/elif
// idiosyncrasies apply, so every language with an `if` belongs here.
//
// Per-language post-offset expectation: unit(1) + fn(1) + 1 if = 3.

fn single_if_no_else(lang: LANG) -> Option<Fixture> {
    // Exhaustive per-language dispatch table; see `switch_with_default`.
    // bca: suppress(cyclomatic)
    let row = match lang {
        LANG::Rust => flat(
            r"fn f(x: i32) {
    if x == 1 {
        let _ = x;
    }
}
",
            "rs",
        ),
        LANG::C | LANG::Cpp | LANG::Mozcpp | LANG::Objc => flat(
            r"void f(int x) {
    if (x == 1) {
        x = x;
    }
}
",
            "c",
        ),
        LANG::Java => in_container(
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
        LANG::Csharp => in_container(
            r"class Parity {
    static void F(int x) {
        if (x == 1) {
            x = x;
        }
    }
}
",
            "cs",
        ),
        LANG::Kotlin => flat(
            r"fun f(x: Int) {
    if (x == 1) {
        println(x)
    }
}
",
            "kt",
        ),
        LANG::Javascript | LANG::Mozjs => flat(
            r"function f(x) {
    if (x === 1) {
        return x;
    }
}
",
            "js",
        ),
        LANG::Typescript => flat(
            r"function f(x: number) {
    if (x === 1) {
        return x;
    }
}
",
            "ts",
        ),
        LANG::Tsx => flat(
            r"function f(x: number) {
    if (x === 1) {
        return x;
    }
}
",
            "tsx",
        ),
        LANG::Php => flat(
            r"<?php
function f($x) {
    if ($x == 1) {
        echo $x;
    }
}
",
            "php",
        ),
        LANG::Python => flat(
            r"def f(x):
    if x == 1:
        return x
",
            "py",
        ),
        LANG::Go => flat(
            r"package p
func f(x int) {
    if x == 1 {
        g(x)
    }
}
",
            "go",
        ),
        LANG::Ruby => flat(
            r"def f(x)
  if x == 1
    x
  end
end
",
            "rb",
        ),
        LANG::Lua => flat(
            r"function f(x)
    if x == 1 then
        return x
    end
end
",
            "lua",
        ),
        LANG::Perl => flat(
            r"sub f {
    my ($x) = @_;
    if ($x == 1) {
        return $x;
    }
}
",
            "pl",
        ),
        LANG::Groovy => flat(
            r"def f(x) {
    if (x == 1) {
        println x
    }
}
",
            "groovy",
        ),
        LANG::Bash => flat(
            r#"#!/bin/bash
f() {
    if [ "$1" -eq 1 ]; then
        echo "$1"
    fi
}
"#,
            "sh",
        ),
        LANG::Tcl => flat(
            r"proc f {x} {
    if { $x == 1 } { return $x }
}
",
            "tcl",
        ),
        LANG::Irules => flat(
            r"proc f {x} {
    if { $x == 1 } { return $x }
}
",
            "irule",
        ),
        // Elixir's `if` is a macro-shaped `Call` rather than a keyword
        // statement (#275), so this row is also the one that would
        // notice the `Exit`-style callee-text match falling out of the
        // cyclomatic impl.
        LANG::Elixir => in_container(
            r"defmodule Parity do
  def f(x) do
    if x == 1 do
      x
    end
  end
end
",
            "ex",
        ),
        // The two C-family helper grammars parse fragments and have no
        // statements at all.
        LANG::Ccomment | LANG::Preproc => return None,
    };
    Some(row)
}

#[test]
fn single_if_no_else_parity() {
    // Anchor the absolute magnitude (lessons #6 / #23 / #468):
    // unit(1) + fn(1) + 1 if = 3. No else clause, so no per-language
    // else/elif idiosyncrasy contributes.
    check_family("single_if_no_else", 3.0, single_if_no_else);
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

fn two_arm_switch_with_wildcard(lang: LANG) -> Option<Fixture> {
    // Exhaustive per-language dispatch table; see `switch_with_default`.
    // bca: suppress(cyclomatic)
    let row = match lang {
        LANG::Rust => flat(
            r#"fn f(x: u8) -> &'static str {
    match x {
        1 => "one",
        _ => "other",
    }
}
"#,
            "rs",
        ),
        LANG::C | LANG::Cpp | LANG::Mozcpp | LANG::Objc => flat(
            r"void f(int x) {
    switch (x) {
        case 1: break;
        default: break;
    }
}
",
            "c",
        ),
        LANG::Java => in_container(
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
        // C# switch-expression with discard arm (issue #282).
        LANG::Csharp => in_container(
            r#"class Parity {
    static string f(int x) => x switch {
        1 => "one",
        _ => "other"
    };
}
"#,
            "cs",
        ),
        // Kotlin `when` with `else ->` arm (issue #282).
        LANG::Kotlin => flat(
            r#"fun f(x: Int): String = when (x) {
    1 -> "one"
    else -> "other"
}
"#,
            "kt",
        ),
        LANG::Javascript | LANG::Mozjs => flat(
            r"function f(x) {
    switch (x) {
        case 1: break;
        default: break;
    }
}
",
            "js",
        ),
        LANG::Typescript => flat(
            r"function f(x: number) {
    switch (x) {
        case 1: break;
        default: break;
    }
}
",
            "ts",
        ),
        LANG::Tsx => flat(
            r"function f(x: number) {
    switch (x) {
        case 1: break;
        default: break;
    }
}
",
            "tsx",
        ),
        LANG::Php => flat(
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
        LANG::Python => flat(
            "def f(x):\n    match x:\n        case 1:\n            return 'one'\n        case _:\n            return 'other'\n",
            "py",
        ),
        LANG::Go => flat(
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
        ),
        LANG::Ruby => flat(
            r#"def f(x)
  case x
  when 1 then "one"
  else "other"
  end
end
"#,
            "rb",
        ),
        LANG::Groovy => flat(
            r"def f(x) {
    switch (x) {
        case 1: break
        default: break
    }
}
",
            "groovy",
        ),
        LANG::Bash => flat(
            "#!/bin/bash\nf() {\n    case \"$1\" in\n        one) echo 1 ;;\n        *)   echo 0 ;;\n    esac\n}\n",
            "sh",
        ),
        LANG::Tcl => flat(
            r"proc f {x} {
    switch $x {
        1 { return one }
        default { return other }
    }
}
",
            "tcl",
        ),
        LANG::Irules => flat(
            r"proc f {x} {
    switch $x {
        1 { return one }
        default { return other }
    }
}
",
            "irule",
        ),
        // Elixir 2-arm `case` with a bare `_ ->` catch-all (issue
        // #1272): the minimal form that catches both an over-count of
        // the wildcard and an under-count of the explicit arm.
        LANG::Elixir => in_container(
            r"defmodule Parity do
  def f(x) do
    case x do
      1 -> :one
      _ -> :other
    end
  end
end
",
            "ex",
        ),
        // No switch-like construct; see `switch_with_default` for why.
        LANG::Lua | LANG::Perl | LANG::Ccomment | LANG::Preproc => return None,
    };
    Some(row)
}

#[test]
fn two_arm_switch_with_wildcard_parity() {
    // Anchor the absolute magnitude (lessons #6 / #23 / #468):
    // unit(1) + fn(1) + 1 explicit arm = 3 (the wildcard/`default` is
    // skipped). This is the canonical lesson-11 trigger (#106): pinning
    // the value guards against both an over-count of the wildcard and an
    // under-count of the explicit arm shifting symmetrically.
    check_family(
        "two_arm_switch_with_wildcard",
        3.0,
        two_arm_switch_with_wildcard,
    );
}

// --- Family 6: trailing-condition loop -----------------------------------
//
// A loop whose body runs before its condition is tested — `do { … }
// while (cond)` in the C family, `repeat … until` in Lua, `begin … end
// while` in Ruby — must contribute exactly one decision point. In the
// C-family cyclomatic impls this fires via the inner `while` keyword
// token (`Cpp::While`, `Java::While`, `Groovy::While`), which is the
// same token used for a standalone `while` loop — listing the
// `DoStatement` node itself would double-count (regression for issue
// #284's incorrect fix proposal).
//
// Per-language post-offset expectation: unit(1) + fn(1) + loop(1) = 3.

fn do_while_loop(lang: LANG) -> Option<Fixture> {
    // Exhaustive per-language dispatch table; see `switch_with_default`.
    // bca: suppress(cyclomatic)
    let row = match lang {
        LANG::C | LANG::Cpp | LANG::Mozcpp | LANG::Objc => flat(
            r"void f() {
    int i = 0;
    do {
        ++i;
    } while (i < 10);
}
",
            "c",
        ),
        LANG::Java => in_container(
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
        LANG::Csharp => in_container(
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
        LANG::Groovy => flat(
            r"def f() {
    int i = 0
    do {
        ++i
    } while (i < 10)
}
",
            "groovy",
        ),
        LANG::Javascript | LANG::Mozjs => flat(
            r"function f() {
    let i = 0;
    do {
        ++i;
    } while (i < 10);
}
",
            "js",
        ),
        LANG::Typescript => flat(
            r"function f() {
    let i: number = 0;
    do {
        ++i;
    } while (i < 10);
}
",
            "ts",
        ),
        LANG::Tsx => flat(
            r"function f() {
    let i: number = 0;
    do {
        ++i;
    } while (i < 10);
}
",
            "tsx",
        ),
        LANG::Php => flat(
            r"<?php
function f() {
    $i = 0;
    do {
        $i++;
    } while ($i < 10);
}
",
            "php",
        ),
        LANG::Kotlin => flat(
            r"fun f() {
    var i = 0
    do {
        i++
    } while (i < 10)
}
",
            "kt",
        ),
        // Perl spells it as a `do` block with a `while` statement
        // modifier, which is the same trailing-condition loop.
        LANG::Perl => flat(
            r"sub f {
    my $i = 0;
    do {
        $i++;
    } while ($i < 10);
}
",
            "pl",
        ),
        // Ruby spells it `begin … end while cond`.
        LANG::Ruby => flat(
            r"def f
  i = 0
  begin
    i += 1
  end while i < 10
end
",
            "rb",
        ),
        // Lua spells it `repeat … until cond`, the same loop with the
        // condition inverted. It is a distinct keyword, so this row is
        // what would notice `RepeatStatement` dropping out of the Lua
        // impl.
        LANG::Lua => flat(
            r"function f()
    local i = 0
    repeat
        i = i + 1
    until i >= 10
end
",
            "lua",
        ),
        // No trailing-condition loop. Rust, Go, Python, Bash, Tcl and
        // iRules offer only head-tested loops (`loop`/`while`, `for`,
        // `while`, `while`/`until`, `while`); the idiom is a head-tested
        // loop with an explicit first iteration, which is a different
        // construct with a different count. Elixir has no loop
        // statement at all — iteration is recursion or a comprehension.
        // The two C-family helper grammars parse fragments.
        LANG::Rust
        | LANG::Go
        | LANG::Python
        | LANG::Bash
        | LANG::Tcl
        | LANG::Irules
        | LANG::Elixir
        | LANG::Ccomment
        | LANG::Preproc => return None,
    };
    Some(row)
}

#[test]
fn do_while_loop_parity() {
    // Anchor the absolute magnitude (lessons #6 / #23 / #468):
    // unit(1) + fn(1) + loop(1) = 3. The loop fires once via the inner
    // `while` keyword token; pinning the value catches a regression that
    // additionally listed `DoStatement` (double-count, #284) symmetrically.
    check_family("do_while_loop", 3.0, do_while_loop);
}

// --- Family 7: iterate-over-a-collection loop ----------------------------
//
// The form that binds each element of a collection in turn — C++
// `for (auto x : xs)` (`ForRangeLoop`), Java / Groovy enhanced-for
// (`EnhancedForStatement`), C# `foreach` (`ForeachStatement`), Kotlin /
// Python / Rust / Ruby `for x in xs`, JS `for…of`, PHP / Perl / Tcl
// `foreach`, Go `range`, Lua's generic `for … in`, Bash `for x in …`,
// and Elixir's `for` comprehension — each must contribute exactly one
// decision point.
//
// In C++/Java/Groovy the `for` keyword token fires inside the
// grammar-specific loop node, so the existing keyword-token arm
// catches them; listing the statement node would double-count
// (regression for issue #284's incorrect fix proposal).
//
// Per-language post-offset expectation: unit(1) + fn(1) + for(1) = 3.

fn range_for_loop(lang: LANG) -> Option<Fixture> {
    // Exhaustive per-language dispatch table; see `switch_with_default`.
    // bca: suppress(cyclomatic)
    let row = match lang {
        LANG::Cpp | LANG::Mozcpp => flat(
            r"void f(std::vector<int> xs) {
    for (auto x : xs) {
        g(x);
    }
}
",
            "cpp",
        ),
        // Objective-C spells it as fast enumeration over an object, a
        // different node from the C++ range-for.
        LANG::Objc => flat(
            r"void f(NSArray *xs) {
    for (id x in xs) {
        g(x);
    }
}
",
            "m",
        ),
        LANG::Java => in_container(
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
        LANG::Groovy => flat(
            r"def f(int[] xs) {
    for (int x : xs) {
        println x
    }
}
",
            "groovy",
        ),
        LANG::Csharp => in_container(
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
        LANG::Kotlin => flat(
            r"fun f(xs: IntArray) {
    for (x in xs) {
        g(x)
    }
}
",
            "kt",
        ),
        LANG::Rust => flat(
            r"fn f(xs: Vec<i32>) {
    for x in xs {
        g(x);
    }
}
",
            "rs",
        ),
        LANG::Javascript | LANG::Mozjs => flat(
            r"function f(xs) {
    for (const x of xs) {
        g(x);
    }
}
",
            "js",
        ),
        LANG::Typescript => flat(
            r"function f(xs: number[]) {
    for (const x of xs) {
        g(x);
    }
}
",
            "ts",
        ),
        LANG::Tsx => flat(
            r"function f(xs: number[]) {
    for (const x of xs) {
        g(x);
    }
}
",
            "tsx",
        ),
        LANG::Php => flat(
            r"<?php
function f($xs) {
    foreach ($xs as $x) {
        g($x);
    }
}
",
            "php",
        ),
        LANG::Python => flat(
            r"def f(xs):
    for x in xs:
        g(x)
",
            "py",
        ),
        LANG::Go => flat(
            r"package p
func f(xs []int) {
    for _, x := range xs {
        g(x)
    }
}
",
            "go",
        ),
        LANG::Ruby => flat(
            r"def f(xs)
  for x in xs
    g(x)
  end
end
",
            "rb",
        ),
        // Lua's generic `for … in <iterator>`, distinct from its
        // numeric `for i = 1, n` form.
        LANG::Lua => flat(
            r"function f(xs)
    for _, x in ipairs(xs) do
        g(x)
    end
end
",
            "lua",
        ),
        LANG::Perl => flat(
            r"sub f {
    my (@xs) = @_;
    foreach my $x (@xs) {
        g($x);
    }
}
",
            "pl",
        ),
        LANG::Bash => flat(
            r#"#!/bin/bash
f() {
    for x in "$@"; do
        echo "$x"
    done
}
"#,
            "sh",
        ),
        LANG::Tcl => flat(
            r"proc f {xs} {
    foreach x $xs {
        puts $x
    }
}
",
            "tcl",
        ),
        LANG::Irules => flat(
            r"proc f {xs} {
    foreach x $xs {
        log local0. $x
    }
}
",
            "irule",
        ),
        // Elixir has no loop statement; `for x <- xs` is a
        // comprehension, and it is the language's iterate-over-a-
        // collection form.
        LANG::Elixir => in_container(
            r"defmodule Parity do
  def f(xs) do
    for x <- xs do
      g(x)
    end
  end
end
",
            "ex",
        ),
        // C has no way to bind each element of a collection: its only
        // loop forms are the three-clause `for`, `while` and `do`. The
        // C++ range-for above is not valid C, so `LANG::C` cannot share
        // that arm. The two C-family helper grammars parse fragments.
        LANG::C | LANG::Ccomment | LANG::Preproc => return None,
    };
    Some(row)
}

#[test]
fn range_for_loop_parity() {
    // Anchor the absolute magnitude (lessons #6 / #23 / #468):
    // unit(1) + fn(1) + for(1) = 3. Each range/enhanced-for fires exactly
    // one decision point; pinning the value catches a regression that
    // additionally listed the statement node (double-count, #284) across
    // all languages symmetrically.
    check_family("range_for_loop", 3.0, range_for_loop);
}

// --- Family 8: safe-navigation operator chains ---------------------------
//
// A two-link safe-navigation chain — Kotlin `a?.b?.c`, Groovy `a?.b?.c`,
// PHP `$a?->b?->c`, Ruby `a&.b&.c`, JS/TS `a?.b?.c`, C# `a?.b?.c` — must
// each contribute exactly two decision points (one short-circuit per
// operator), matching the inconsistency #281 / c8b7d93 set out to remove
// and the Ruby/Groovy fix in #452.
//
// Each operator is short-circuit (it skips the member access/call when the
// LHS is null/nil), so standard CCN counts it like `&&` / `||`. Kotlin /
// JS / TS / Groovy match the `?.` token (`QMARKDOT`), PHP matches `?->`
// (`QMARKDASHGT`), Ruby matches `&.` (`AMPDOT`), and C# matches the
// `ConditionalAccessExpression` node — every granularity fires once per
// textual operator, so the chain is +2 everywhere.
//
// Per-language post-offset expectation: unit(1) + fn(1) + 2 ops = 4.

fn safe_navigation_chain(lang: LANG) -> Option<Fixture> {
    // Exhaustive per-language dispatch table; see `switch_with_default`.
    // bca: suppress(cyclomatic)
    let row = match lang {
        LANG::Kotlin => flat("fun f(a: A?): C? { return a?.b?.c }\n", "kt"),
        LANG::Groovy => flat("def f(a){ return a?.b?.c }\n", "groovy"),
        LANG::Php => flat("<?php function f($a){ return $a?->b?->c; }\n", "php"),
        LANG::Ruby => flat("def f(a); a&.b&.c; end\n", "rb"),
        LANG::Javascript | LANG::Mozjs => flat("function f(a){ return a?.b?.c; }\n", "js"),
        LANG::Typescript => flat("function f(a: any): any { return a?.b?.c; }\n", "ts"),
        LANG::Tsx => flat("function f(a: any): any { return a?.b?.c; }\n", "tsx"),
        LANG::Csharp => in_container(
            "public class Parity { static object F(Parity a){ return a?.b?.c; } }\n",
            "cs",
        ),
        // No safe-navigation operator. Rust models absence in the type
        // system (`?` on an `Option` is an early exit, not a member
        // access); the C family, Java, Go, Python, Lua, Perl, Tcl,
        // iRules and Elixir each spell the guard as an explicit
        // conditional, which family 4 already covers. Bash has no member
        // access at all, and the two C-family helper grammars parse
        // fragments.
        LANG::Rust
        | LANG::C
        | LANG::Cpp
        | LANG::Mozcpp
        | LANG::Objc
        | LANG::Java
        | LANG::Go
        | LANG::Python
        | LANG::Lua
        | LANG::Perl
        | LANG::Bash
        | LANG::Tcl
        | LANG::Irules
        | LANG::Elixir
        | LANG::Ccomment
        | LANG::Preproc => return None,
    };
    Some(row)
}

#[test]
fn safe_navigation_chain_parity() {
    // Anchor the absolute magnitude, not just mutual agreement: a shared
    // regression that dropped both `?.` links symmetrically across every
    // language would still satisfy `assert_parity` (all would agree on the
    // wrong value). Pin the hand-derived spec value so the +2 magnitude
    // itself is guarded (lessons #6 / #23): unit(1) + fn(1) + 2 ops = 4.
    check_family("safe_navigation_chain", 4.0, safe_navigation_chain);
}
