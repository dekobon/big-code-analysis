# Walking the AST directly

[`Ast::parse`][ast_parse] gives you a parsed
[`tree_sitter::Tree`][ts_tree] together with the source bytes it was
parsed from; [`Ast::as_tree_sitter`][ast_as_ts] hands that tree out as a
borrowed reference. This chapter shows how to use it to drive your own
syntax-tree analysis — counting node kinds, finding constructs by name,
detecting parse errors, or pulling out a symbol table — without paying
for a second parse.

[ast_parse]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Ast.html#method.parse
[ts_tree]: https://docs.rs/tree-sitter/*/tree_sitter/struct.Tree.html
[ast_as_ts]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Ast.html#method.as_tree_sitter

## When to use this

Reach for direct AST traversal when:

- You want to **count or find syntactic constructs** in-process. The CLI
  equivalents (`bca count <kind>`, `bca find <kind>`,
  [recipe](../recipes/ast-queries.md)) shell out per file; the library
  path is one parse and one Rust loop.
- You want to **detect parse errors** programmatically. Tree-sitter
  emits a synthetic `ERROR` node anywhere the grammar could not match;
  [`Node::has_error`][node_has_error] is O(1) — tree-sitter caches the
  error bit on every node — so the check is free even on a multi-MB
  source file.
- You want to **mix metrics with custom analysis** in one parse — e.g.
  capture metric values *and* a list of function names for a coverage
  mapping, an IDE outline, or a code-owner report.

If you only need standard metrics, stay with [`analyze`][analyze] or
[`Ast::metrics`][ast_metrics] — they walk the tree for you. The direct
path is for things the metric walker does not already compute.

[node_has_error]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Node.html#method.has_error
[analyze]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.analyze.html
[ast_metrics]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Ast.html#method.metrics

## Use the re-exported `tree_sitter`

Import `tree_sitter` from
[`big_code_analysis::tree_sitter`][bca_ts] rather than adding a sibling
`tree-sitter` dependency. The re-export is pinned to the exact version
the metric walker was built against, so the `Tree` types agree by
definition. See [Reusing an existing tree-sitter
Tree](reuse-tree.md#working-example) and
[Stability and versioning](stability.md) for the value-not-stable
posture this re-export carries.

[bca_ts]: https://docs.rs/big-code-analysis/*/big_code_analysis/tree_sitter/index.html

## A reusable DFS walker

Most of the examples below need a depth-first traversal of every
descendant. Tree-sitter ships a [`TreeCursor`][ts_cursor] that does
this in O(1) per step (no allocations beyond the cursor itself). The
canonical walk is short enough to inline:

```rust
use big_code_analysis::tree_sitter;

/// Visit every node in `tree` in pre-order, root first, passing each
/// node to `visit`. Allocation-free apart from the cursor itself.
fn walk_preorder<F: FnMut(tree_sitter::Node<'_>)>(
    tree: &tree_sitter::Tree,
    mut visit: F,
) {
    let mut cursor = tree.walk();
    'walk: loop {
        visit(cursor.node());
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                continue 'walk;
            }
            if !cursor.goto_parent() {
                return;
            }
        }
    }
}
```

The pattern is: visit, descend, climb back up while there is no next
sibling, repeat. Every example in this chapter is a thin wrapper around
this walker — the code fences below are marked `ignore` because they
assume `walk_preorder` is already in scope; the matching set of tests
in [`tests/book_ast_traversal_examples.rs`][tests] keeps them
honest, so a refactor that broke an example would fail `cargo test`.

[tests]: https://github.com/dekobon/big-code-analysis/blob/main/tests/book_ast_traversal_examples.rs

[ts_cursor]: https://docs.rs/tree-sitter/*/tree_sitter/struct.TreeCursor.html

## Count nodes by kind

Library equivalent of `bca count if_expression for_expression
while_expression` from the
[AST-queries recipe](../recipes/ast-queries.md):

```rust,ignore
use big_code_analysis::{Ast, LANG, Source};
use std::collections::HashMap;

let ast = Ast::parse(Source::new(
    LANG::Rust,
    b"fn a() { if true { 1 } else { 2 } } fn b() { for _ in 0..10 {} }",
))
.expect("rust feature enabled");

let mut counts: HashMap<&str, usize> = HashMap::new();
walk_preorder(ast.as_tree_sitter(), |node| {
    *counts.entry(node.kind()).or_default() += 1;
});

assert_eq!(counts.get("if_expression").copied().unwrap_or(0), 1);
assert_eq!(counts.get("for_expression").copied().unwrap_or(0), 1);
```

The string keys (`"if_expression"`, `"for_expression"`, …) are the
tree-sitter grammar's node-type names. The fastest way to discover them
for a new language is `bca --paths sample.rs dump`, which prints the
full AST.

> **Anonymous tokens.** The walker visits every node tree-sitter emits,
> including anonymous tokens like `"{"`, `";"`, and keyword literals.
> The targeted `counts.get("if_expression")` lookups above are unaffected
> — anonymous tokens have different kind names — but `counts.values().sum()`
> would be much larger than the count of *named* grammar productions.
> Filter with [`tree_sitter::Node::is_named()`][ts_is_named] inside the
> visitor if you only want named nodes.

[ts_is_named]: https://docs.rs/tree-sitter/*/tree_sitter/struct.Node.html#method.is_named

## Find nodes by kind

Library equivalent of `bca find unsafe_block`:

```rust,ignore
use big_code_analysis::{Ast, LANG, Source};

let ast = Ast::parse(Source::new(
    LANG::Rust,
    b"fn safe() {} fn risky() { unsafe { } }",
))
.expect("rust feature enabled");

let source = ast.source();
// Captured slices borrow from `source` — no per-hit `String` allocation.
let mut hits: Vec<((usize, usize), &str)> = Vec::new();
walk_preorder(ast.as_tree_sitter(), |node| {
    if node.kind() == "unsafe_block" {
        let span = (node.start_position().row, node.end_position().row);
        let text = node
            .utf8_text(source)
            .expect("source is valid utf-8");
        hits.push((span, text));
    }
});

assert_eq!(hits.len(), 1);
```

`Node::utf8_text(&source[..])` slices the source bytes by the node's
byte range. Pair it with [`Ast::source`][ast_source] — for C++ with
preprocessor inputs supplied to [`Ast::parse`][ast_parse], `source` is
the *expanded* buffer the parser actually saw, not the original input
(see [the C++ preprocessor note](parse-once.md#c-preprocessor)).

[ast_source]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Ast.html#method.source

## Detect parse errors

Tree-sitter is lossless: even on malformed input it returns a tree, but
nodes that could not be matched are tagged as errors. The cheapest
check is on the root:

```rust
use big_code_analysis::{Ast, LANG, Source};

let ast = Ast::parse(Source::new(LANG::Rust, b"fn broken("))
    .expect("rust feature enabled");

// Walks far enough to confirm something went wrong, but does not
// enumerate every error site.
assert!(ast.as_tree_sitter().root_node().has_error());
```

To list the offending nodes, walk the tree and check each:

```rust,ignore
use big_code_analysis::{Ast, LANG, Source};

let ast = Ast::parse(Source::new(LANG::Rust, b"fn broken("))
    .expect("rust feature enabled");

let mut error_lines = Vec::new();
walk_preorder(ast.as_tree_sitter(), |node| {
    if node.is_error() || node.is_missing() {
        error_lines.push(node.start_position().row);
    }
});

assert!(!error_lines.is_empty());
```

`Node::is_error()` flags the synthetic `ERROR` node tree-sitter inserts
where it could not match the grammar; `Node::is_missing()` flags
phantom nodes the parser invented to recover from a missing token. The
CLI's `bca find ERROR` recipe uses the same nodes.

## Combine metrics with a custom walk

The whole point of [`Ast`][ast] is parse-once / compute-many. A
realistic pipeline computes metrics *and* extracts a symbol table from
the same parse:

```rust,ignore
use big_code_analysis::{Ast, LANG, MetricsOptions, Source};

let ast = Ast::parse(Source::new(
    LANG::Rust,
    b"fn outer() { fn inner() {} } fn alone() {}",
))
.expect("rust feature enabled");

// One parse: metrics walker uses it…
let space = ast
    .metrics(MetricsOptions::default())
    .expect("walker succeeds");

// …and so does the custom walk, against the very same tree. The
// captured names borrow from `source` rather than allocating a fresh
// `String` per function — the same pattern as `find_unsafe_blocks`
// above.
let source = ast.source();
let mut functions: Vec<&str> = Vec::new();
walk_preorder(ast.as_tree_sitter(), |node| {
    if node.kind() == "function_item"
        && let Some(name_node) = node.child_by_field_name("name")
    {
        let name = name_node
            .utf8_text(source)
            .expect("source is valid utf-8");
        functions.push(name);
    }
});

assert_eq!(space.metrics.nom.functions_sum(), 3.0);
assert_eq!(functions, ["outer", "inner", "alone"]);
```

`Node::child_by_field_name` walks the named grammar fields — the same
fields that show up in the `FieldName` column when you run
`bca --paths sample.rs dump`. Field-based lookup is more robust than
positional indexing because it does not depend on which children the
grammar emits for anonymous tokens (commas, parentheses, …).

[ast]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Ast.html

## Want a serializable JSON tree?

For pipelines that want a structured AST as data — diffing, queries on
the wire, language-agnostic schema work — the
[`AstCallback`][ast_callback] / [`AstNode`][astnode] family materializes
the tree as a `Serialize`-able struct. This is what the REST `/ast`
endpoint produces (`bca dump` uses a separate `Dump` callback that
writes a human-readable form to stdout). Library consumers can call
the JSON-shaped callback directly:

```rust,no_run
use std::path::PathBuf;

use big_code_analysis::{
    AstCallback, AstCfg, AstPayload, LANG, action,
};

let payload = AstPayload {
    id: "snippet".to_owned(),
    file_name: "snippet.rs".to_owned(),
    code: "fn f() {}".to_owned(),
    comment: false,
    span: true,
};
let cfg = AstCfg {
    id: payload.id.clone(),
    comment: payload.comment,
    span: payload.span,
};
let response = action::<AstCallback>(
    &LANG::Rust,
    payload.code.into_bytes(),
    &PathBuf::from(&payload.file_name),
    None,
    cfg,
);
let json = serde_json::to_string(&response).expect("AstResponse serializes");
println!("{json}");
```

For one-off in-process work, the `as_tree_sitter()` walker above is
cheaper (no allocation per node). Reach for `AstCallback` when you
need a serializable owned tree.

[ast_callback]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.AstCallback.html
[astnode]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.AstNode.html

## Out of scope

- **Incremental reparse** — tree-sitter supports
  `tree_sitter::InputEdit` for incremental updates, but `Ast` is a
  snapshot. To reflect a source edit, build a fresh `Ast::parse` or
  call `Parser::parse(&new_source, Some(&old_tree))` directly via the
  re-exported `tree_sitter` and feed the result through
  [`Ast::from_tree_sitter`](parse-once.md#adopting-a-caller-built-tree).
- **The crate-internal `big_code_analysis::Node` wrapper.** It is
  exposed for the metric walker's traversal needs, but most of its
  traversal methods (`kind`, `child_count`, `children`, `cursor`, …)
  stay `pub(crate)`. Library consumers should reach the tree-sitter
  `Node` through `as_tree_sitter().root_node()` — that is the
  documented seam.
