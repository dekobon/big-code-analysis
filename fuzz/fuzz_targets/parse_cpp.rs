//! Fuzz C++: parse, then every `Ast` walk.
//!
//! The highest-prior target of the per-language set. tree-sitter-cpp's
//! raw-string external scanner carried a real unbounded `memcpy` in
//! `deserialize` (bca #1058, upstream tree-sitter/tree-sitter-cpp#368),
//! and a sanitizer build is what surfaced it. That scanner is stateful,
//! which is the class a coverage-guided mutator explores far better than
//! a fixture suite.
//!
//! The C-family macro-masking lexer is *not* reached from here — it needs
//! preprocessor results attached to the `Source`. See `preproc_macro`.

#![no_main]

use big_code_analysis::LANG;
use big_code_analysis_fuzz::walk_all;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    walk_all(LANG::Cpp, data.to_vec());
});
