# Reusing an existing tree-sitter Tree

*Planned — not yet shipped. See [issue #251].*

[issue #251]: https://github.com/dekobon/big-code-analysis/issues/251

## What this page will cover

A common pain point is that callers who already drive
[tree-sitter] for syntax highlighting, code folding, or queries
end up parsing every file twice: once for their own tree, once
inside `get_function_spaces`. The planned parse seam will let you
hand `big-code-analysis` an already-parsed `tree_sitter::Tree` and
get the same `FuncSpace` back without re-parsing.

[tree-sitter]: https://tree-sitter.github.io/tree-sitter/

## Current status

`big-code-analysis` does not re-export the `tree-sitter` crate
yet, and there is no public entry point that accepts a
pre-parsed tree. Today the library always owns the parser end to
end. If you want to share a tree across both pipelines, you have
two options:

1. **Parse twice.** Inefficient but correct. The library pins
   `tree-sitter = "=0.26.8"`; if you pin to the same exact version
   on the consumer side, the parsed trees will agree
   structurally.
2. **Wait for [#251].** That issue tracks a first-class parse seam
   plus a `tree_sitter` re-export so consumers no longer need to
   keep their pin in lockstep with ours.

In the meantime the [`Node`][Node] wrapper does expose the
underlying `tree_sitter::Node` through `.0`, so if you are willing
to depend on the same exact `tree-sitter` pin you can already
read the AST that the metric walk runs over — see the
[escape-hatches section of STABILITY.md][stability-escape-hatches]
for the supported usage.

[Node]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Node.html
[stability-escape-hatches]: https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md#escape-hatches
