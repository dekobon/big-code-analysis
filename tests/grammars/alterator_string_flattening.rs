//! Behavioural coverage for the per-language `Alterator::alterate`
//! string-flattening arms in `big-code-analysis-ast/src/alterator.rs`.
//!
//! `alterate` collapses a string-like literal into a single leaf
//! [`AstNode`] holding its verbatim source text, so the AST dump (and the
//! REST `/ast` endpoint that consumes it) renders `"hi"` as one node
//! rather than exposing the grammar's delimiter / `string_content`
//! children. Eight languages — Objective-C, Mozilla C++, C#, Lua, Tcl,
//! iRules, Ruby, and Elixir — had no dump test reaching this arm, leaving
//! their `alterate` impls entirely uncovered. Each case parses a minimal
//! snippet, dumps it through the public [`Ast::dump`] API, and asserts the
//! literal survives as a verbatim leaf.
//!
//! The assertion is non-vacuous: without flattening the literal's quotes
//! and body land in separate child leaves (`"`, `hi`, `"`), so no single
//! leaf carries the full quoted text — only the `alterate` arm produces a
//! childless node whose value is `"hi"`.
#![allow(missing_docs)]
// Every case below is gated on its own grammar feature, so a build that
// enables none of the eight leaves the helpers they share with no caller
// at all. The relaxation is scoped to exactly that configuration, so a
// genuinely dead helper still fails the `--all-features` leg.
#![cfg_attr(
    not(any(
        feature = "csharp",
        feature = "elixir",
        feature = "irules",
        feature = "lua",
        feature = "mozcpp",
        feature = "objc",
        feature = "ruby",
        feature = "tcl",
    )),
    allow(dead_code)
)]

use big_code_analysis::{Ast, AstCfg, AstNode, LANG, Source};

/// Dumps `code` for `lang` and returns the root AST node. Every caller is
/// a `#[cfg(feature = …)]`-gated case, so `lang`'s grammar is compiled in
/// whenever this runs and the parse cannot fail for want of a feature.
fn dump_root(lang: LANG, code: &str, file_name: &str) -> AstNode {
    let cfg = AstCfg {
        id: String::new(),
        language: String::new(),
        comment: false,
        span: false,
    };
    Ast::parse(Source::new(lang, code.as_bytes()).with_name(Some(file_name.to_owned())))
        .expect("case gated on this language's grammar feature")
        .dump(cfg)
        .root
        .expect("source parses to a root AST node")
}

/// True if the tree contains a leaf node (no children) whose verbatim
/// value equals `literal` — the signature of a flattened string arm.
fn has_verbatim_leaf(node: &AstNode, literal: &str) -> bool {
    (node.children.is_empty() && node.value == literal)
        || node.children.iter().any(|c| has_verbatim_leaf(c, literal))
}

#[track_caller]
fn assert_flattened(lang: LANG, code: &str, file_name: &str, literal: &str) {
    let root = dump_root(lang, code, file_name);
    assert!(
        has_verbatim_leaf(&root, literal),
        "{lang:?}: expected {literal:?} to survive as a single verbatim leaf in the AST dump"
    );
}

/// Emits one `#[test]` per `name: feature, lang, code, file, literal;`
/// case, each asserting the literal survives `alterate` as a single
/// verbatim leaf. Mirrors the `roundtrip_tests!` pattern in
/// `src/language_enum_roundtrip.rs`.
///
/// The feature literal is per row rather than one gate on the module
/// because the table spans eight grammars: a whole-module
/// `#[cfg(feature = …)]` — the shape the single-grammar siblings
/// `c_grammar_metrics` and `mozcpp_grammar_metrics` use — can name only
/// one of them. A row whose grammar is absent must be *absent* too, not
/// merely failing: without the gate every row panicked in `dump_root`
/// under any feature set short of the full eight, which reads as a
/// defect in whatever was being changed (`.claude/rules/testing.md`,
/// "Gate a feature-gated fixture table on the union of its rows").
macro_rules! flatten_cases {
    ($($name:ident: $feat:literal, $lang:expr, $code:expr, $file:expr, $lit:expr;)*) => {
        $(
            #[test]
            #[cfg(feature = $feat)]
            fn $name() {
                assert_flattened($lang, $code, $file, $lit);
            }
        )*
    };
}

flatten_cases! {
    objc_flattens_string_literal: "objc", LANG::Objc, "int f(void) { const char *s = \"hi\"; return 0; }", "f.m", "\"hi\"";
    mozcpp_flattens_string_literal: "mozcpp", LANG::Mozcpp, "int f() { const char *s = \"hi\"; return 0; }", "f.cpp", "\"hi\"";
    csharp_flattens_string_literal: "csharp", LANG::Csharp, "class C { void M() { string s = \"hi\"; } }", "f.cs", "\"hi\"";
    lua_flattens_string_literal: "lua", LANG::Lua, "local s = \"hi\"", "f.lua", "\"hi\"";
    tcl_flattens_quoted_word: "tcl", LANG::Tcl, "set s \"hi\"", "f.tcl", "\"hi\"";
    // In valid iRules a quoted word only appears inside an event handler's
    // `{ … }` body. Until #1381 `alterate` flattened that body as a single
    // `braced_word` leaf, so the only verbatim text in the whole dump was
    // the brace block and the `quoted_word` arm was unreachable from any
    // valid input. The handler body is a script now, so the arm this file
    // is about is finally what this row tests — the same claim its Tcl
    // twin above makes.
    irules_flattens_quoted_word: "irules", LANG::Irules, "when HTTP_REQUEST { set s \"hi\" }", "f.irule", "\"hi\"";
    // The Tcl half of the same claim. `tcl_flattens_quoted_word` above
    // reaches the literal at statement level, where the body guard has
    // nothing to do — verified by perturbation: deleting the Tcl guard
    // failed no test until this row existed, while the iRules twin failed
    // on its own because iRules has no statement level to test from.
    tcl_flattens_quoted_word_inside_a_proc_body: "tcl", LANG::Tcl, "proc p {} { puts \"hi\" }", "f.tcl", "\"hi\"";
    // The braced *value* half of the same rule: `lappend`'s argument is a
    // literal, so it keeps the flattening a script body gives up. Without
    // it the #1381 guard would read as "braced words are never flattened",
    // which is the opposite over-correction.
    tcl_flattens_braced_value: "tcl", LANG::Tcl, "lappend x {a b}\n", "f.tcl", "{a b}";
    irules_flattens_braced_value: "irules", LANG::Irules, "lappend b {x y}\n", "f.irule", "{x y}";
    // The value slots of a construct `is_value_braced_word` classifies
    // whole as script-taking. Without `is_braced_literal_slot` the dump
    // rendered `{my proc}` as a command named `my` and `{a b}` as a
    // command named `a` — the literal's text survived only inside a
    // subtree the source does not contain.
    tcl_flattens_braced_proc_name: "tcl", LANG::Tcl, "proc {my proc} {} {}\n", "f.tcl", "{my proc}";
    tcl_flattens_namespace_argument: "tcl", LANG::Tcl, "namespace export {a b}\n", "f.tcl", "{a b}";
    irules_flattens_braced_proc_name: "irules", LANG::Irules, "proc {my proc} {} {}\n", "f.irule", "{my proc}";
    irules_flattens_namespace_argument: "irules", LANG::Irules, "namespace export {a b}\n", "f.irule", "{a b}";
    // The *value* argument of a command whose other arguments are
    // scripts (#1381 review). Each of these was flattened before #1381
    // and became a nested `command` under it, so the dump grew a
    // subtree the source does not contain: `{100}` rendered as a
    // command named `100`, `{5}` as one named `5`. The script argument
    // beside each is the negative half, asserted through
    // `Checker::is_string_with_code` in `checker.rs` — this file can
    // only observe the positive, since a flattened leaf is what it
    // looks for.
    tcl_flattens_after_delay: "tcl", LANG::Tcl, "after {100} {puts a}\n", "f.tcl", "{100}";
    tcl_flattens_time_count: "tcl", LANG::Tcl, "time {puts b} {5}\n", "f.tcl", "{5}";
    tcl_flattens_uplevel_level: "tcl", LANG::Tcl, "uplevel {1} {puts c}\n", "f.tcl", "{1}";
    tcl_flattens_switch_subject: "tcl", LANG::Tcl, "switch {foo} {p {puts d}}\n", "f.tcl", "{foo}";
    tcl_flattens_namespace_eval_name: "tcl", LANG::Tcl, "namespace eval {my ns} {puts e}\n", "f.tcl", "{my ns}";
    irules_flattens_after_delay: "irules", LANG::Irules, "after {100} {log a}\n", "f.irule", "{100}";
    irules_flattens_time_count: "irules", LANG::Irules, "time {log b} {5}\n", "f.irule", "{5}";
    irules_flattens_uplevel_level: "irules", LANG::Irules, "uplevel {1} {log c}\n", "f.irule", "{1}";
    irules_flattens_namespace_eval_name: "irules", LANG::Irules, "namespace eval {my ns} {log d}\n", "f.irule", "{my ns}";
    ruby_flattens_string_literal: "ruby", LANG::Ruby, "s = \"hi\"\n", "f.rb", "\"hi\"";
    elixir_flattens_string_literal: "elixir", LANG::Elixir, "s = \"hi\"\n", "f.ex", "\"hi\"";
}
