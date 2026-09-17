#![allow(missing_docs)]
use crate::common;

use common::compare_rca_output_with_files;

// Hand-written, not derived: the corpus walk picks a language per
// file at run time from its extension, so nothing in this body
// names the grammar it needs. The glob list (`*.rs`) is what
// decides it, and a build without that grammar scores every file
// zero rather than matching the snapshot (#1472).
// test-lang-gates: hand-written(rust) — the corpus walk picks a
//     language per file from its extension, so the glob list decides it
//     and nothing in the body names it
#[cfg(feature = "rust")]
#[test]
fn test_serde() {
    compare_rca_output_with_files("serde", &["*.rs"], &[], 172);
}
