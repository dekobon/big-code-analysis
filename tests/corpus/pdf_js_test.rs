#![allow(missing_docs)]
use crate::common;

use common::compare_rca_output_with_files;

#[test]
fn test_pdfjs() {
    // The 118-entry exclude list that used to live here (mozjs-era parse
    // failures, #84) was retired by #1282: every former entry parses
    // without ERROR nodes under the post-#507 upstream
    // tree-sitter-javascript grammar. Re-add an exclude only with a live
    // tracking issue, mirroring how deepspeech_test.rs points at #86.
    compare_rca_output_with_files("pdf.js", &["*.js"], &[], 384);
}
