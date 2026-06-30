//! Behavioural coverage for the per-language `Alterator::alterate`
//! string-flattening arms in `src/alterator.rs`.
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

use big_code_analysis::{Ast, AstCfg, AstNode, LANG, Source};

/// Dumps `code` for `lang` and returns the root AST node. Runs only under
/// builds where `lang`'s grammar feature is enabled (the `--all-features`
/// test leg); the `feature-matrix` legs merely `cargo check` this file.
fn dump_root(lang: LANG, code: &str, file_name: &str) -> AstNode {
    let cfg = AstCfg {
        id: String::new(),
        language: String::new(),
        comment: false,
        span: false,
    };
    Ast::parse(Source::new(lang, code.as_bytes()).with_name(Some(file_name.to_owned())))
        .expect("language feature enabled under --all-features")
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

#[test]
fn objc_flattens_string_literal() {
    assert_flattened(
        LANG::Objc,
        "int f(void) { const char *s = \"hi\"; return 0; }",
        "f.m",
        "\"hi\"",
    );
}

#[test]
fn mozcpp_flattens_string_literal() {
    assert_flattened(
        LANG::Mozcpp,
        "int f() { const char *s = \"hi\"; return 0; }",
        "f.cpp",
        "\"hi\"",
    );
}

#[test]
fn csharp_flattens_string_literal() {
    assert_flattened(
        LANG::Csharp,
        "class C { void M() { string s = \"hi\"; } }",
        "f.cs",
        "\"hi\"",
    );
}

#[test]
fn lua_flattens_string_literal() {
    assert_flattened(LANG::Lua, "local s = \"hi\"", "f.lua", "\"hi\"");
}

#[test]
fn tcl_flattens_quoted_word() {
    assert_flattened(LANG::Tcl, "set s \"hi\"", "f.tcl", "\"hi\"");
}

#[test]
fn irules_flattens_braced_word() {
    // In valid iRules a quoted word only appears inside an event handler's
    // `{ … }` body, which `alterate` flattens as a single `braced_word`
    // leaf (the same match arm that handles `quoted_word`), so the verbatim
    // text to look for is the whole brace block.
    assert_flattened(
        LANG::Irules,
        "when HTTP_REQUEST { set s \"hi\" }",
        "f.irul",
        "{ set s \"hi\" }",
    );
}

#[test]
fn ruby_flattens_string_literal() {
    assert_flattened(LANG::Ruby, "s = \"hi\"\n", "f.rb", "\"hi\"");
}

#[test]
fn elixir_flattens_string_literal() {
    assert_flattened(LANG::Elixir, "s = \"hi\"\n", "f.ex", "\"hi\"");
}
