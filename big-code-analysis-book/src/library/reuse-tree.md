# Reusing an existing tree-sitter Tree

A common pain point is that callers who already drive
[tree-sitter] for syntax highlighting, code folding, or queries
end up parsing every file twice: once for their own tree, once
inside the metric walker. The parse seam (issue [#251]) lets you
hand `big-code-analysis` an already-parsed `tree_sitter::Tree` and
get the same `FuncSpace` back without re-parsing.

> **Use [`Ast::from_tree_sitter`][aft].** It adopts a caller-built
> `tree_sitter::Tree` and lets you run the metric walker more than
> once against the same parse (different
> [`MetricsOptions::with_only`][with_only] selections, custom
> tree-sitter walks interleaved with metrics, `Ast::ops` for
> operator/operand extraction, etc.). See
> [Parse once, run metrics many times](parse-once.md). It carries an
> explicit `name: Option<String>` rather than deriving the top-level
> [`FuncSpace::name`][fsn] from a path via lossy UTF-8 conversion.
>
> [with_only]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.MetricsOptions.html#method.with_only
> [aft]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Ast.html#method.from_tree_sitter
> [fsn]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.FuncSpace.html#structfield.name

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

Use the byte-based entry point [`analyze`][analyze] (with a
[`Source`][source]) if you do not already have a tree — it
constructs the parser internally and owns the parse end to end.

[analyze]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.analyze.html
[source]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Source.html

## Working example

```rust,no_run
use big_code_analysis::{analyze, tree_sitter, Ast, LANG, MetricsOptions, Source};

let source_code = "fn main() { if true { 1 } else { 2 }; }";
let source = source_code.as_bytes().to_vec();

// Step 1: build a tree with the *re-exported* tree-sitter crate.
// Using `big_code_analysis::tree_sitter` (rather than a direct
// `tree-sitter` dependency on your side) guarantees the version
// matches the one the metric walker was compiled against.
let mut parser = tree_sitter::Parser::new();
parser
    .set_language(&LANG::Rust.tree_sitter_language())
    .expect("rust grammar pinned to a compatible version");
let tree = parser
    .parse(&source, None)
    .expect("parser has a language set");

// Step 2: adopt the tree with an explicit display name.
let from_tree = Ast::from_tree_sitter(
    LANG::Rust,
    tree,
    source.clone(),
    Some("foo.rs".to_owned()),
)
.expect("rust feature enabled")
.metrics(MetricsOptions::default())
.expect("non-empty input");

// Step 3 (optional): confirm the values match the byte-based path.
let from_bytes = analyze(
    Source::new(LANG::Rust, &source).with_name(Some("foo.rs".to_owned())),
    MetricsOptions::default(),
)
.expect("non-empty input");

assert_eq!(
    from_tree.metrics.cyclomatic.cyclomatic_sum(),
    from_bytes.metrics.cyclomatic.cyclomatic_sum(),
);
```

The same shape works for any [`LANG`][lang] variant — pass the
matching grammar to `tree_sitter::Parser::set_language` (via
[`LANG::tree_sitter_language`][lang_grammar]) and the metric
walker will produce the same `FuncSpace` it would have produced
from bytes.

[lang]: https://docs.rs/big-code-analysis/*/big_code_analysis/enum.LANG.html
[lang_grammar]: https://docs.rs/big-code-analysis/*/big_code_analysis/enum.LANG.html#method.tree_sitter_language

## The single tree-adoption seam

[`Ast::from_tree_sitter`][aft] is *the* entry point for tree reuse —
it dispatches on a `LANG` at runtime and hides the parser plumbing
entirely. The former lower-level path (the generic `Parser<T>` /
`ParserTrait` and the per-language `*Parser` / `*Code` tag types)
is now crate-private (`pub(crate)`) and is no longer part of the
public surface; see [STABILITY.md][stab]. Library consumers should
adopt a tree through `Ast::from_tree_sitter`, which does not expose
any per-language tag types or trait bounds.

[stab]: https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md

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
