mod common;

use std::path::Path;

use common::compare_rca_output_with_files_under;

#[test]
fn test_csharp() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("repositories")
        .join("big-code-analysis-output");

    compare_rca_output_with_files_under(&source_root, "csharp", &["*.cs"], &[]);
}
