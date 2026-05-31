use tree_sitter::Language;

// Layout is deliberately one variant per line so the per-variant
// "// -> <crate>" comments stay attached to the right Lang. The
// grammar crate that backs each variant is resolved by hand in
// `mk_get_language!` (see macros.rs); only non-obvious mappings
// are annotated.
#[rustfmt::skip]
mk_langs!(
    Kotlin, // -> tree_sitter_kotlin_ng
    Lua,
    Java,
    Go,
    Rust,
    Tcl,  // -> bca-tree-sitter-tcl (vendored fork, see Cargo.toml)
    Irules, // -> tree_sitter_irules (F5 iRules, a Tcl dialect)
    Cpp,  // -> bca-tree-sitter-mozcpp (vendored Mozilla C++ grammar)
    Python,
    Tsx,        // -> tree_sitter_typescript::LANGUAGE_TSX
    Typescript, // -> tree_sitter_typescript::LANGUAGE_TYPESCRIPT
    Bash,
    Csharp,   // -> tree_sitter_c_sharp
    Elixir,
    Ccomment, // -> bca-tree-sitter-ccomment (vendored fork)
    Preproc,  // -> bca-tree-sitter-preproc (vendored fork)
    Mozjs,    // -> bca-tree-sitter-mozjs (vendored Mozilla JS grammar)
    Javascript,
    Perl,
    Php, // -> tree_sitter_php::LANGUAGE_PHP
    Ruby,
    Groovy // -> dekobon_tree_sitter_groovy
);
