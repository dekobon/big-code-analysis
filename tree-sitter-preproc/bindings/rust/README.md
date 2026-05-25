# bca-tree-sitter-preproc

This crate is the `big-code-analysis` fork of `tree-sitter-preproc`,
published under the `bca-tree-sitter-*` namespace so it does not
collide with the original Mozilla `tree-sitter-preproc` on
crates.io. The Rust import path is preserved as
`tree_sitter_preproc`, so existing code does not change.

To use this crate, add it to the `[dependencies]` section of your
`Cargo.toml` file. (You will probably also need to depend on the
[`tree-sitter`][tree-sitter crate] crate to use the parsed result.)

``` toml
[dependencies]
tree-sitter = "0.26"
bca-tree-sitter-preproc = "1.0"
```

Typically, you will use the [LANGUAGE][] function to add this
grammar to a tree-sitter [Parser][], and then use the parser to parse some code:

``` rust
let code = r#"
    int double(int x) {
        return x * 2;
    }
"#;
let mut parser = Parser::new();
let language = tree_sitter_preproc::LANGUAGE;
parser
    .set_language(&language.into())
    .expect("Error loading Preproc parser");
let tree = parser.parse(code, None).unwrap();
assert!(!tree.root_node().has_error());
```

If you have any questions, please reach out to us in the [tree-sitter
discussions] page.

[Language]: https://docs.rs/tree-sitter/*/tree_sitter/struct.Language.html
[Parser]: https://docs.rs/tree-sitter/*/tree_sitter/struct.Parser.html
[tree-sitter]: https://tree-sitter.github.io/
[tree-sitter crate]: https://crates.io/crates/tree-sitter
[tree-sitter discussions]: https://github.com/tree-sitter/tree-sitter/discussions
