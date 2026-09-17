//! Per-grammar metric driver: suites that pin one tree-sitter grammar's
//! parse trees against the metric impls written for it, plus the
//! alterator's string-flattening rules.
//!
//! Grouped into one binary by #1124 — see `tests/api/main.rs`.

// Per-language test gating (#1472) makes "is this import live" a
// function of the enabled feature set, which no `cfg` on the import
// itself can express. Partial builds only — the build CI gates on and
// the one a contributor runs still police every unused import. Dead
// *items* are not relaxed: unlike the two library roots this test crate
// carries no `allow(dead_code)`, so every helper, `const`, macro and
// test here still needs its own gate. See `.claude/rules/testing.md`,
// "Why the import lint is off on a partial build".
#![cfg_attr(not(feature = "all-languages"), allow(unused_imports))]
mod alterator_string_flattening;
mod c_grammar_metrics;
mod mozcpp_grammar_metrics;
