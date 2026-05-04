use tree_sitter::Language;

mk_langs!(
    // 1) Name for enum
    // 2) tree-sitter function to call to get a Language
    (Kotlin, tree_sitter_kotlin_ng),
    (Lua, tree_sitter_lua),
    (Java, tree_sitter_java),
    (Go, tree_sitter_go),
    (Rust, tree_sitter_rust),
    (Tcl, tree_sitter_tcl),
    (Cpp, tree_sitter_cpp),
    (Python, tree_sitter_python),
    (Tsx, tree_sitter_tsx),
    (Typescript, tree_sitter_typescript),
    (Bash, tree_sitter_bash),
    (Ccomment, tree_sitter_ccomment),
    (Preproc, tree_sitter_preproc),
    (Mozjs, tree_sitter_mozjs),
    (Javascript, tree_sitter_javascript),
    (Perl, tree_sitter_perl),
    (Php, tree_sitter_php)
);
