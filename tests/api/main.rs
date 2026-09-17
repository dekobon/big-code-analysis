//! Public-API integration driver: the `analyze` / `Ast` seam, the
//! book's runnable library examples, derived trait contracts, parser
//! reuse across languages, and in-source suppression markers.
//!
//! Each module below was its own `tests/*.rs` crate root until #1124.
//! One integration binary statically links the tree-sitter runtime and
//! every grammar, so thirty-one of them made linking — not compilation
//! — the tail of every incremental `cargo test`. Grouping by theme
//! keeps the crate roots readable while paying that link cost six times
//! instead of thirty-one. Test bodies are unchanged apart from two
//! mechanical edits: `mod common;` becomes `use crate::common;` where a
//! module needs the shared corpus harness, and a module that gated
//! itself with a crate-level `#![cfg]` carries the gate on its `mod`
//! declaration here instead, so this file's `//!` doc stays ungated for
//! the no-default-features and minimal-langs CI legs.

// Per-language test gating (#1472) makes "is this import live" a
// function of the enabled feature set, which no `cfg` on the import
// itself can express. Partial builds only — the build CI gates on and
// the one a contributor runs still police every unused import. Dead
// *items* are not relaxed: unlike the two library roots this test crate
// carries no `allow(dead_code)`, so every helper, `const`, macro and
// test here still needs its own gate. See `.claude/rules/testing.md`,
// "Why the import lint is off on a partial build".
#![cfg_attr(not(feature = "all-languages"), allow(unused_imports))]
mod ast_seam_test;
// test-lang-gates: hand-written(rust) — a `mod` declaration has no body
//     to read; the grammar is named inside the file it points at
#[cfg(feature = "rust")]
mod book_ast_traversal_examples;
// test-lang-gates: hand-written(rust) — a `mod` declaration has no body
//     to read; the grammar is named inside the file it points at
#[cfg(feature = "rust")]
mod book_library_examples;
mod derive_eq_hash_ord;
// The feature gate moved here from the `mod parser_reuse` wrapper the
// file used to carry: as a module it can be gated at the declaration,
// and keeping the wrapper would have nested `parser_reuse` inside
// itself.
// `rust` alone. Every test in the file needs the Rust grammar — three
// of them need TypeScript as well and say so individually — so
// conjoining TypeScript here dropped the other two from every Rust-only
// build. That is the over-gating #1478 is about, and it had been in the
// tree the whole time. `any(rust, typescript)` would be wrong the other
// way: under TypeScript alone the module has no tests at all and its
// shared fixtures go dead.
// test-lang-gates: hand-written(rust) — a `mod` declaration has no body
//     to read; the grammar is named inside the file it points at
#[cfg(feature = "rust")]
mod parser_reuse;
mod suppression_test;
