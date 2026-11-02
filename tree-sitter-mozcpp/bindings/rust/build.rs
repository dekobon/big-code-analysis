fn main() {
    let src_dir = std::path::Path::new("src");

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
    c_config.std("c11").include(src_dir);

    // -Wno-sign-compare: scanner.c compares `lexer->lookahead` (int32_t)
    // against `wchar_t` containers. The pattern is inherited from upstream
    // tree-sitter scanner conventions and is wiped by recreate-grammars
    // regeneration, so the suppression must live in this build.rs rather
    // than in the C source. See #399.
    c_config.flag_if_supported("-Wno-sign-compare");

    #[cfg(target_env = "msvc")]
    c_config.flag("-utf-8");

    let parser_path = src_dir.join("parser.c");
    c_config.file(&parser_path);
    println!("cargo:rerun-if-changed={}", parser_path.to_str().unwrap());

    let scanner_path = src_dir.join("scanner.c");
    c_config.file(&scanner_path);
    println!("cargo:rerun-if-changed={}", scanner_path.to_str().unwrap());

    c_config.compile("tree-sitter-mozcpp");
}
