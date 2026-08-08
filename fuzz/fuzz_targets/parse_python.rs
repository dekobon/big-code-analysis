//! Fuzz Python: parse, then every `Ast` walk.
//!
//! Python's external scanner tracks indentation across the whole file,
//! so its state machine is driven by every byte rather than by local
//! context — the shape a mutator explores well and a fixture suite
//! samples thinly. Python is also the language whose NPA metric reads
//! receiver bytes directly to tell `self.x` from `db.x`, so the metric
//! walk indexes into the source rather than only into the tree.

#![no_main]

use big_code_analysis::LANG;
use big_code_analysis_fuzz::walk_all;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    walk_all(LANG::Python, data.to_vec());
});
