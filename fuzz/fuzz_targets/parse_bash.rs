//! Fuzz Bash: parse, then every `Ast` walk.
//!
//! Heredocs make the Bash scanner hold a delimiter across lines, so an
//! unterminated one leaves the scanner in a state no single line
//! reveals. Unterminated constructs are the shape the adversarial byte
//! corpus covered only at file granularity; a mutator reaches the
//! interior states.

#![no_main]

use big_code_analysis::LANG;
use big_code_analysis_fuzz::walk_all;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    walk_all(LANG::Bash, data.to_vec());
});
