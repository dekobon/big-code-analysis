#![allow(missing_docs)]
use crate::common;

use common::compare_rca_output_with_files;

// Hand-written, not derived: the corpus walk picks a language per
// file at run time from its extension, so nothing in this body
// names the grammar it needs. The glob list (`*.js`) is what
// decides it, and a build without that grammar scores every file
// zero rather than matching the snapshot (#1472).
// test-lang-gates: hand-written(javascript) — the corpus walk picks a
//     language per file from its extension, so the glob list decides it
//     and nothing in the body names it
#[cfg(feature = "javascript")]
#[test]
fn test_pdfjs() {
    // The 118-entry exclude list that used to live here (mozjs-era parse
    // failures, #84) was retired by #1282: every former entry parses
    // without ERROR nodes under the post-#507 upstream
    // tree-sitter-javascript grammar. Re-add an exclude only with a live
    // tracking issue, mirroring how deepspeech_test.rs points at #86.
    compare_rca_output_with_files("pdf.js", &["*.js"], &[], 384);
}
