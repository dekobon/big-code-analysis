use tree_sitter::Language;

mk_langs!(
    // Enum variants only. Grammar resolution lives in mk_get_language!.
    (Kotlin),
    (Lua),
    (Java),
    (Go),
    (Rust),
    (Tcl),
    // Cpp intentionally resolves to tree_sitter_mozcpp::LANGUAGE.
    (Cpp),
    (Python),
    (Tsx),
    (Typescript),
    (Bash),
    (Csharp),
    (Elixir),
    (Ccomment),
    (Preproc),
    (Mozjs),
    (Javascript),
    (Perl),
    (Php),
    (Ruby),
    (Groovy)
);
