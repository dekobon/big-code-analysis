fn main() {
    let src_dir = std::path::Path::new("src");

    // The parser and scanner include the headers under `src/tree_sitter/`
    // (`parser.h`, and `alloc.h` / `array.h` where present). Watching only
    // the two translation units below left a header-only edit with a stale
    // object while the CLI's build-inputs digest (#1773) moved on — a
    // binary advertising a tree it did not compile.
    let headers = src_dir.join("tree_sitter");
    if let Ok(entries) = std::fs::read_dir(&headers) {
        for entry in entries.flatten() {
            match entry.path().to_str() {
                Some(path) => println!("cargo:rerun-if-changed={path}"),
                // A name cargo cannot be told exactly: watch the directory
                // instead, which changes whenever that file is replaced.
                None => println!("cargo:rerun-if-changed={}", headers.display()),
            }
        }
    }

    let mut c_config = cc::Build::new();
    c_config
        .std("c11")
        .include(src_dir)
        .flag_if_supported("-Wno-unused-parameter");

    #[cfg(target_env = "msvc")]
    c_config.flag("-utf-8");

    let parser_path = src_dir.join("parser.c");
    c_config.file(&parser_path);
    println!("cargo:rerun-if-changed={}", parser_path.to_str().unwrap());

    let scanner_path = src_dir.join("scanner.c");
    c_config.file(&scanner_path);
    println!("cargo:rerun-if-changed={}", scanner_path.to_str().unwrap());

    c_config.compile("tree-sitter-mozjs");
}
