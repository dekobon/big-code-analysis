# AST queries

Recipes that work with the parsed syntax tree directly: searching for
node types, counting them, or dumping the tree.

> **Library-side equivalents.** Every recipe below has an in-process
> Rust counterpart in
> [Walking the AST directly](../library/ast-traversal.md) — useful
> when shelling out per file is too slow or when you want to compose
> metrics with custom AST analysis in one parse.

## Detect parse errors before committing

Tree-sitter exposes a synthetic `ERROR` node anywhere it could not
parse. Use `find` to surface them:

```bash
bca find \
    --include "*.rs" \
    --paths "$PWD" \
    -t ERROR
```

> **One glob per occurrence.** `--include` and `--exclude` take
> exactly one value each time they appear; repeat the flag for
> multiple globs (`--include "*.rs" --include "*.py"`). A positional
> path that follows is never swallowed. The `=` form
> (`--include="*.rs"`) also works.

A clean run prints nothing. Wire this into a pre-commit hook to fail
fast when a syntactically broken file is staged.

## Count specific syntactic constructs

`count` takes one or more node types via the repeatable `-t`/`--type`
flag and reports the totals. For example, to count `if`, `for`, and
`while` constructs across a Rust project:

```bash
bca count \
    --include "*.rs" \
    --paths src/ \
    -t if_expression -t for_expression -t while_expression
```

The exact node-type names come from the underlying tree-sitter
grammar. To discover them, dump the AST of a small sample file
(see below) and read the node names off the tree.

## Find all `unsafe` blocks in a Rust crate

```bash
bca find \
    --include "*.rs" \
    --paths src/ \
    -t unsafe_block
```

Each match prints the file path and the line range of the node.

## Dump the AST of a file

Useful for understanding why a metric came out the way it did, or for
discovering the tree-sitter node names you need for `find` / `count`:

```bash
bca dump --paths src/lib.rs
```

To narrow the dump to a specific function or block, add line bounds
with the `--line-start` and `--line-end` flags (they must follow the
`dump` subcommand):

```bash
bca dump \
    --paths src/lib.rs \
    --line-start 42 --line-end 88
```

`--line-start` / `--line-end` apply to `dump` and `find`, so the same
range can be used to scope a search to a single function:

```bash
bca find \
    --paths src/lib.rs \
    --line-start 42 --line-end 88 -t return_expression
```

The short `--ls` / `--le` spellings remain as deprecated aliases and
are slated for removal in the next major.

## List every function or method

For a quick human-readable inventory:

```bash
bca functions \
    --include "*.rs" \
    --paths src/
```

The output is a tree per file: an `In file …` header followed by an
indented row per function with name and line span. It is intended for
reading, not parsing.

For tooling that needs a structured inventory — coverage mapping,
documentation generation, code-owner reports — use the JSON `metrics`
output instead and walk `.spaces[]` recursively, taking entries whose
`kind` is `function`:

```bash
bca metrics \
    --include "*.rs" \
    --paths src/ \
    -O json \
  | jq -c '
      . as $root
      | def funcs: if .kind == "function" then [.] else [] end
                   + (.spaces // [] | map(funcs) | add // []);
      funcs[] | {file: $root.name, name, start_line, end_line}
    '
```

This emits one JSON object per function and is safe to pipe into
downstream tooling.
