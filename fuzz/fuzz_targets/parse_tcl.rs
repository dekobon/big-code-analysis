//! Fuzz Tcl: parse, then every `Ast` walk.
//!
//! The one grammar in this set we vendor and maintain ourselves
//! (`bca-tree-sitter-tcl`), so a scanner defect found here is ours to
//! fix rather than an upstream report. Tcl is also command-dispatched:
//! control flow has no dedicated node kind and is recognised by the
//! command's leading word, so the metric walk compares source bytes
//! where other languages compare `kind_id`.

#![no_main]

use big_code_analysis::LANG;
use big_code_analysis_fuzz::walk_all;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    walk_all(LANG::Tcl, data.to_vec());
});
