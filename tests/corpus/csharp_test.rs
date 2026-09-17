#![allow(missing_docs)]
use crate::common;

use std::path::Path;

use common::compare_rca_output_with_files_under;

// Hand-written, not derived: the corpus walk picks a language per
// file at run time from its extension, so nothing in this body
// names the grammar it needs. The glob list (`*.cs`) is what
// decides it, and a build without that grammar scores every file
// zero rather than matching the snapshot (#1472).
// test-lang-gates: hand-written(csharp) — the corpus walk picks a
//     language per file from its extension, so the glob list decides it
//     and nothing in the body names it
#[cfg(feature = "csharp")]
#[test]
fn test_csharp() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("repositories")
        .join("big-code-analysis-output");

    compare_rca_output_with_files_under(&source_root, "csharp", &["*.cs"], &[], 6);
}
