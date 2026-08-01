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

mod ast_seam_test;
#[cfg(feature = "rust")]
mod book_ast_traversal_examples;
#[cfg(feature = "rust")]
mod book_library_examples;
mod derive_eq_hash_ord;
// The feature gate moved here from the `mod parser_reuse` wrapper the
// file used to carry: as a module it can be gated at the declaration,
// and keeping the wrapper would have nested `parser_reuse` inside
// itself.
#[cfg(all(feature = "rust", feature = "typescript"))]
mod parser_reuse;
mod suppression_test;
