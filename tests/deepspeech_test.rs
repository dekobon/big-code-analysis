#![allow(missing_docs)]
mod common;

use common::compare_rca_output_with_files;

#[test]
fn test_deepspeech() {
    // FIXME: Ignoring these files temporarily due to parsing errors (originally https://github.com/dekobon/big-code-analysis/issues/83,
    // now tracked in the open issue https://github.com/dekobon/big-code-analysis/issues/86), to keep CI green until they parse cleanly.
    //
    // #721 note: the upstream-grammar flip (#720) fixed two of the
    // originally-skipped files — `fast-dtoa.cc` (tree-sitter-cpp#311) and
    // `left_test.cc` (tree-sitter-cpp#312) now parse without errors under
    // upstream `tree-sitter-cpp`, where the Mozilla fork's
    // `class_specifier` override had triggered an ERROR cascade — so they
    // were un-skipped here and given snapshots. The remaining five are
    // shared-base defects (present in both grammars) and stay skipped.
    let exclude = &[
        "**/DeepSpeech/native_client/deepspeech.cc",
        "**/DeepSpeech/native_client/kenlm/util/mmap.cc",
        "**/DeepSpeech/native_client/deepspeech.h",
        "**/DeepSpeech/native_client/ctcdecode/third_party/openfst-1.6.7/src/test/fst_test.h",
        "**/DeepSpeech/native_client/ctcdecode/third_party/openfst-1.6.9-win/src/include/fst/test/fst_test.h",
        // Vendored third-party directories with no snapshot coverage (8500+ files).
        // Only native_client/ has accepted snapshots in the big-code-analysis-output submodule.
        "**/DeepSpeech/tensorflow/**",
        "**/DeepSpeech/kenlm/**",
    ];

    compare_rca_output_with_files(
        "DeepSpeech",
        &["*.cc", "*.cpp", "*.h", "*.hh"],
        exclude,
        1042,
    );
}
