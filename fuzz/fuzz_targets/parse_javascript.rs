//! Fuzz JavaScript: parse, then every `Ast` walk.
//!
//! The regex-versus-division ambiguity makes JavaScript lexing depend on
//! parser state rather than on the token alone, and its external scanner
//! is correspondingly stateful. The grammar is also the largest in the
//! pin set by node-kind count, so it carries the most dispatch arms.
//!
//! `LANG::Mozjs` is not fuzzed separately: the Gecko fork was measured
//! metric-equivalent to upstream on real JavaScript when #507 made
//! upstream the default, and it owns no file extension but `.jsm`.

#![no_main]

use big_code_analysis::LANG;
use big_code_analysis_fuzz::walk_all;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    walk_all(LANG::Javascript, data.to_vec());
});
