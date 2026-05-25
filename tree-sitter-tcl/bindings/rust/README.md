# bca-tree-sitter-tcl

This crate is the `big-code-analysis` fork of
[`tree-sitter-tcl`](https://github.com/tree-sitter-grammars/tree-sitter-tcl)
by Lewis Russell, published under the `bca-tree-sitter-*` namespace so
it is unambiguously the version we ship inside `big-code-analysis`.
The Rust import path is preserved as `tree_sitter_tcl`, so existing
code does not change.

To use this crate, add it to the `[dependencies]` section of your
`Cargo.toml` file. (You will probably also need to depend on the
[`tree-sitter`][tree-sitter crate] crate to use the parsed result.)

``` toml
[dependencies]
tree-sitter = "0.26"
bca-tree-sitter-tcl = "1.0"
```

Typically, you will use the [LANGUAGE][] constant to add this grammar
to a tree-sitter [Parser][], and then use the parser to parse some
code:

``` rust
let mut parser = tree_sitter::Parser::new();
let language = tree_sitter_tcl::LANGUAGE;
parser
    .set_language(&language.into())
    .expect("Error loading Tcl parser");
```

The upstream grammar lives at
<https://github.com/tree-sitter-grammars/tree-sitter-tcl>; the
`LICENSE` shipped in this tarball preserves Lewis Russell's
copyright notice and adds an MIT-compatible modifications line for
the `big-code-analysis` changes.

[Language]: https://docs.rs/tree-sitter/*/tree_sitter/struct.Language.html
[Parser]: https://docs.rs/tree-sitter/*/tree_sitter/struct.Parser.html
[tree-sitter crate]: https://crates.io/crates/tree-sitter
