//! Fuzz Rust: parse, then every `Ast` walk.
//!
//! #1051 — a five-byte input that panicked the library in release — was
//! `loc/rust.rs` computing `end - 1` for a doc comment ending at EOF.
//! That is the bug the `clippy::arithmetic_side_effects` adoption in
//! #1152 was validated against, and Rust is the language whose `loc`
//! module carries the span arithmetic it was scoped to.
//!
//! Reaching the class at all depends on the harness not normalising: see
//! the module docs in `big_code_analysis_fuzz`.

#![no_main]

use big_code_analysis::LANG;
use big_code_analysis_fuzz::walk_all;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    walk_all(LANG::Rust, data.to_vec());
});
