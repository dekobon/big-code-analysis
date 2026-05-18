# Reusing an existing tree-sitter Tree

A common pain point is that callers who already drive
[tree-sitter] for syntax highlighting, code folding, or queries
end up parsing every file twice: once for their own tree, once
inside `get_function_spaces`. The parse seam (issue [#251]) lets you
hand `big-code-analysis` an already-parsed `tree_sitter::Tree` and
get the same `FuncSpace` back without re-parsing.

[tree-sitter]: https://tree-sitter.github.io/tree-sitter/
[#251]: https://github.com/dekobon/big-code-analysis/issues/251

## When to use this

Use the parse seam if you:

- Already keep a `tree_sitter::Tree` per open buffer (editor, LSP,
  language server, custom static-analysis pipeline) and want to
  reuse that parse for metrics rather than paying the byte-based
  cost again.
- Want to run multiple passes (metrics + AST dump + custom
  analysis) against one parse result.
- Intend to pin `tree-sitter` on your side without taking a
  separate dependency from this library. The re-exported
  `big_code_analysis::tree_sitter` module is the same crate we
  link against, so the types agree by definition.

Use the byte-based entry points
([`get_function_spaces`][gfs] / [`metrics_with_options`][mwo]) if
you do not already have a tree — they construct the parser
internally and own the parse end to end.

[gfs]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.get_function_spaces.html
[mwo]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.metrics_with_options.html

## Working example

```rust,no_run
use std::path::PathBuf;

use big_code_analysis::{
    get_function_spaces, metrics_from_tree, tree_sitter, LANG,
    MetricsOptions,
};

let source_code = "fn main() { if true { 1 } else { 2 }; }";
let path = PathBuf::from("foo.rs");
let source = source_code.as_bytes().to_vec();

// Step 1: build a tree with the *re-exported* tree-sitter crate.
// Using `big_code_analysis::tree_sitter` (rather than a direct
// `tree-sitter` dependency on your side) guarantees the version
// matches the one the metric walker was compiled against.
let mut parser = tree_sitter::Parser::new();
parser
    .set_language(&LANG::Rust.get_tree_sitter_language())
    .expect("rust grammar pinned to a compatible version");
let tree = parser
    .parse(&source, None)
    .expect("parser has a language set");

// Step 2: feed the tree into metrics_from_tree.
let from_tree = metrics_from_tree(
    &LANG::Rust,
    tree,
    source.clone(),
    &path,
    None,
    MetricsOptions::default(),
)
.expect("non-empty input");

// Step 3 (optional): confirm the values match the byte-based path.
let from_bytes =
    get_function_spaces(&LANG::Rust, source, &path, None)
        .expect("non-empty input");

assert_eq!(
    from_tree.metrics.cyclomatic.cyclomatic_sum(),
    from_bytes.metrics.cyclomatic.cyclomatic_sum(),
);
```

The same shape works for any [`LANG`][lang] variant — pass the
matching grammar to `tree_sitter::Parser::set_language` (via
[`LANG::get_tree_sitter_language`][lang_grammar]) and the metric
walker will produce the same `FuncSpace` it would have produced
from bytes.

[lang]: https://docs.rs/big-code-analysis/*/big_code_analysis/enum.LANG.html
[lang_grammar]: https://docs.rs/big-code-analysis/*/big_code_analysis/enum.LANG.html#method.get_tree_sitter_language

## Lower-level: `Parser::from_tree`

If you want to drive the metric walker by hand (e.g. to pass the
resulting `Parser` to `metrics_with_options`, `find`,
`operands_and_operators`, or `dump_root`), construct a `Parser`
directly with [`Parser::from_tree`][pft] instead of going through
`metrics_from_tree`:

```rust,no_run
use std::path::PathBuf;

use big_code_analysis::{
    metrics_with_options, tree_sitter, MetricsOptions, RustParser,
    ParserTrait, LANG,
};

let source = b"fn main() {}".to_vec();
let path = PathBuf::from("foo.rs");

let mut ts_parser = tree_sitter::Parser::new();
ts_parser
    .set_language(&LANG::Rust.get_tree_sitter_language())
    .expect("rust grammar pinned to a compatible version");
let tree = ts_parser.parse(&source, None).expect("parsed");

let parser = RustParser::from_tree(tree, source);
let _space = metrics_with_options(&parser, &path, MetricsOptions::default());
```

[pft]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Parser.html#method.from_tree

The per-language `*Parser` aliases (`RustParser`, `PythonParser`,
…) are emitted by the `mk_langs!` macro and are part of the
`#[doc(hidden)]` escape-hatch surface — see [STABILITY.md][stab]
for the caveat. For most callers, the higher-level
`metrics_from_tree` is the right entry point because it dispatches
on a `&LANG` at runtime.

[stab]: https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md#escape-hatches

## Out of scope

- **Incremental re-computation.** Applying a `tree_sitter::InputEdit`
  and re-querying only the changed spans is not supported yet —
  the metric walker still walks the entire tree on every call. The
  parse seam is the first step; making the walker itself
  incremental is a follow-up.
- **Promoting all of `Node`'s `pub(crate)` traversal methods.**
  `Node` still exposes its inner `tree_sitter::Node` through the
  public `.0` field for ad-hoc traversal; the wrapper helpers
  remain crate-private and are tracked under the `pub use`
  curation issue.
