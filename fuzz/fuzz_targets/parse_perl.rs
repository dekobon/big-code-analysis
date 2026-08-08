//! Fuzz Perl: parse, then every `Ast` walk.
//!
//! tree-sitter-perl carries the largest external scanner of the pinned
//! grammars — Perl's quote-like operators, heredocs, and POD sections
//! each need bespoke lexing — and it is the one external grammar that
//! depends on the `tree-sitter` runtime directly rather than through the
//! shared `tree-sitter-language` shim.

#![no_main]

use big_code_analysis::LANG;
use big_code_analysis_fuzz::walk_all;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    walk_all(LANG::Perl, data.to_vec());
});
