# Using as a Library

`big-code-analysis` is published on [crates.io] as a Rust library. The
CLI (`bca`) and REST server (`bca-web`) are both thin wrappers around
the same public API, so anything they can do you can do directly from
your own crate.

This section is task-oriented. For full type signatures and field
docs, follow the [rustdoc on docs.rs][docs.rs].

[crates.io]: https://crates.io/crates/big-code-analysis
[docs.rs]: https://docs.rs/big-code-analysis/

## When to embed the library

Reach for the library (instead of shelling out to `bca`) when you
want one or more of the following:

- **In-process analysis.** Avoid the cost of spawning a subprocess
  per file when scoring thousands of files in a custom tool, IDE
  plugin, or static-analysis pipeline.
- **In-memory source.** Score generated, pre-processed, or
  streamed source without writing it to disk first. See
  [Analyzing in-memory source](in-memory.md).
- **Selective walking.** Drive a custom traversal over the
  `FuncSpace` tree to extract per-function metrics on your own
  schedule. See [Walking FuncSpace results](walking-funcspace.md).
- **Custom output.** Skip the JSON / YAML / TOML / CBOR serializers
  shipped under `src/output/` and emit your own report format
  (CSV, SARIF, a database row, whatever).

If you just want a Markdown quality report or a CI threshold gate,
the [`bca` CLI](../commands/README.md) is faster to wire up.

## What is on offer today

- [Quick start](quick-start.md) — parse a string, get a
  `FuncSpace`, print the cognitive complexity.
- [Analyzing in-memory source](in-memory.md) — feed source from a
  buffer rather than a file.
- [Reusing an existing tree-sitter Tree](reuse-tree.md) — feed a
  caller-built `tree_sitter::Tree` into the metric walker.
- [Parse once, run metrics many times](parse-once.md) — hold a parsed
  `Ast` and run multiple metric subsets / custom walks against the
  same tree.
- [Walking the AST directly](ast-traversal.md) — count syntactic
  constructs, find nodes by kind, detect parse errors, or build a
  symbol table alongside the metrics walk.
- [Selecting metrics](selecting-metrics.md) — *(stub — planned)*.
- [Walking `FuncSpace` results](walking-funcspace.md) — recurse
  into nested function / class / impl spaces.
- [Error handling](error-handling.md) — what `Result<FuncSpace, MetricsError>`
  means today and how to turn it into a useful diagnostic.
- [Stability and versioning](stability.md) — what you can and
  cannot rely on across `0.x` versions.

## A note on API churn

The library is pre-`1.0`. Several entry points named in this
section will be renamed or replaced as the library DX umbrella
(tracked under [issue #250]) lands. Each page calls out which
sub-issue will change it and how. Until those land, every example
below compiles against the current published crate.

[issue #250]: https://github.com/dekobon/big-code-analysis/issues/250
