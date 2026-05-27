fn main() {
    let src_dir = std::path::Path::new("src");

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
