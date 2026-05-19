# Parse once, run metrics many times

`big-code-analysis`'s one-shot entry point [`analyze`][analyze] re-parses
its [`Source`][source] on every call. For pipelines that score a file
multiple times — different metric subsets, an interleaved custom
tree-sitter walk, or a metric re-run after a configuration change — that
re-parse is wasted work.

The [`Ast`][ast] type, added in `0.0.26` ([#264]), exposes the seam:
parse the source once, then call [`Ast::metrics`][ast_metrics] as many
times as you need against the held parse.

[analyze]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.analyze.html
[source]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Source.html
[ast]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Ast.html
[ast_metrics]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Ast.html#method.metrics
[#264]: https://github.com/dekobon/big-code-analysis/issues/264

## When to use this

Reach for `Ast` when any of the following applies:

- **Selective metric runs.** You compute one set of metrics for a
  report, then another for a CI threshold gate, against the same file.
- **Custom tree-sitter walks.** You already drive a `tree_sitter::Tree`
  for queries / highlighting / symbol extraction and want to fold the
  metric walker into the same parse.
- **Cached analysis.** An LSP-like service that holds parsed files in
  memory should be able to re-run metrics on demand when configuration
  changes, without going back to bytes.

If you only ever compute every metric once per file, stick with
[`analyze`][analyze] — it now delegates to `Ast` internally, so the
shapes line up but the one-shot API stays simpler.

## Selective metrics across calls

```rust,no_run
use big_code_analysis::{Ast, LANG, Metric, MetricsOptions, Source};

let source = b"fn f(x: i32) -> i32 { if x > 0 { 1 } else { -1 } }";

// One parse, two metric subsets.
let ast = Ast::parse(Source::new(LANG::Rust, source))
    .expect("rust feature enabled");

let loc = ast
    .metrics(MetricsOptions::default().with_only(&[Metric::Loc]))
    .expect("walker succeeds");
let cyclomatic = ast
    .metrics(MetricsOptions::default().with_only(&[Metric::Cyclomatic]))
    .expect("walker succeeds");

println!("ploc = {}", loc.metrics.loc.ploc());
println!("ccn  = {}", cyclomatic.metrics.cyclomatic.cyclomatic_sum());
```

Each `metrics` call walks the tree once. The savings versus calling
[`analyze`][analyze] twice come from skipping the parse, which dominates
runtime for everything except the very largest source files.

## Custom tree-sitter walk + metrics on the same parse

`Ast::as_tree_sitter` borrows the underlying `tree_sitter::Tree`. The
returned reference is valid for the lifetime of the `Ast`; nodes
obtained from it resolve against `Ast::source` (see the [note on the
C++ preprocessor](#c-preprocessor) below for what `source` returns
under macro expansion).

> **For realistic AST work** — counting node kinds, finding constructs
> by name, detecting parse errors, building a symbol table — see
> [Walking the AST directly](ast-traversal.md). The example below is a
> minimal smoke test; the dedicated chapter shows the full pattern
> (reusable depth-first walker, field-name lookup, error detection).

```rust,no_run
use big_code_analysis::{Ast, LANG, MetricsOptions, Source};

let ast = Ast::parse(Source::new(LANG::Rust, b"fn f() {}"))
    .expect("rust feature enabled");

// Walk the tree for your own purposes…
let root = ast.as_tree_sitter().root_node();
assert_eq!(root.kind(), "source_file");

// …and run the metric walker over the same parse.
let space = ast
    .metrics(MetricsOptions::default())
    .expect("walker succeeds");
println!("name = {:?}", space.name);
```

## Adopting a caller-built tree

If you already build the `tree_sitter::Tree` yourself (e.g. because
your editor / LSP has its own parser pool),
[`Ast::from_tree_sitter`][ast_from_ts] is the `Source`-flavored
counterpart of the older [`metrics_from_tree`][mft]. It carries an
explicit `name: Option<String>` end-to-end instead of deriving one
from a path via lossy UTF-8 conversion.

[ast_from_ts]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Ast.html#method.from_tree_sitter
[mft]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.metrics_from_tree.html

```rust,no_run
use big_code_analysis::{Ast, LANG, MetricsOptions, tree_sitter};

let source = b"fn f() {}".to_vec();
let mut parser = tree_sitter::Parser::new();
parser
    .set_language(
        &LANG::Rust
            .get_tree_sitter_language()
            .expect("rust feature enabled"),
    )
    .expect("rust grammar compatible");
let tree = parser
    .parse(&source, None)
    .expect("parser has a language set");

let ast = Ast::from_tree_sitter(LANG::Rust, tree, source, None)
    .expect("rust feature enabled");
let _ = ast.metrics(MetricsOptions::default()).expect("walker succeeds");
```

The tree must have been produced from `code` with the grammar returned
by [`LANG::get_tree_sitter_language`][lang_grammar] for `lang`; a
mismatch is not `unsafe`, but the metric walker matches on tree-sitter
`kind_id` values that come from the language's enum, so values from a
different grammar yield nonsensical results.

[lang_grammar]: https://docs.rs/big-code-analysis/*/big_code_analysis/enum.LANG.html#method.get_tree_sitter_language

## C++ preprocessor

When `Ast::parse` is called on a [`Source`][source] carrying preprocessor
inputs (`Source::with_preproc_path` + `Source::with_preproc`) and the
language is [`LANG::Cpp`][lang], the macro pre-pass runs before
`tree-sitter` does — and `Ast::source` returns the *expanded* bytes the
parser actually saw, not the original input.

`Ast::from_tree_sitter` is unaffected: it adopts whatever tree the
caller built. Whatever expansion (or lack thereof) the caller applied
before building the tree is what `Ast::source` reflects.

[lang]: https://docs.rs/big-code-analysis/*/big_code_analysis/enum.LANG.html

## Concurrency

`Ast` is `Send + Sync`. Running `Ast::metrics` from multiple threads
against the same `&Ast` is safe — the walker only reads from the held
`tree_sitter::Tree`. (Benchmarking parallel metric runs is a separate
follow-up.)

## Out of scope

- **Incremental reparse via `tree_sitter::InputEdit`.** Caching a stable
  `Ast` across an analysis pipeline is in scope; editing the held tree
  is not.
- **Parallel-by-default APIs.** `Ast::metrics` does not internally
  parallelize across the metric set. Callers that want one thread per
  subset are free to do so.
