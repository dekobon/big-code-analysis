//! Fuzz C: parse, then every `Ast` walk.
//!
//! `LANG::C` routes to upstream tree-sitter-c, a different grammar from
//! the C++ one, and shares the preprocessor macro-replacement path
//! (#721). Kept separate from `parse_cpp` because the two grammars have
//! independent external scanners.

#![no_main]

use big_code_analysis::LANG;
use big_code_analysis_fuzz::walk_all;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    walk_all(LANG::C, data.to_vec());
});
