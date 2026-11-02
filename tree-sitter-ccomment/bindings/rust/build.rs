// Adapted from tree-sitter-python bindings
use std::path::Path;
extern crate cc;

fn main() {
    let src_dir = Path::new("src");

    // The parser and scanner include the headers under `src/tree_sitter/`
    // (`parser.h`, and `alloc.h` / `array.h` where present). Watching only
    // the two translation units below left a header-only edit with a stale
    // object while the CLI's build-inputs digest (#1773) moved on — a
    // binary advertising a tree it did not compile.
    if let Ok(entries) = std::fs::read_dir(src_dir.join("tree_sitter")) {
        for entry in entries.flatten() {
            println!("cargo:rerun-if-changed={}", entry.path().display());
        }
    }

    let mut c_config = cc::Build::new();
    c_config.include(src_dir);
    c_config
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-unused-but-set-variable")
        .flag_if_supported("-Wno-trigraphs");
    let parser_path = src_dir.join("parser.c");
    c_config.file(&parser_path);
    println!("cargo:rerun-if-changed={}", parser_path.to_str().unwrap());
    c_config.compile("parser");

    let mut cpp_config = cc::Build::new();
    cpp_config.cpp(true);
    cpp_config.include(src_dir);
    // -Wno-sign-compare: scanner.cc compares `lexer->lookahead` (int32_t)
    // against `wchar_t` containers. The pattern is inherited from upstream
    // tree-sitter scanner conventions and is wiped by recreate-grammars
    // regeneration, so the suppression must live in this build.rs rather
    // than in the C++ source. See #399.
    cpp_config
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-unused-but-set-variable")
        .flag_if_supported("-Wno-sign-compare");
    let scanner_path = src_dir.join("scanner.cc");
    cpp_config.file(&scanner_path);
    println!("cargo:rerun-if-changed={}", scanner_path.to_str().unwrap());
    cpp_config.compile("scanner");
}
