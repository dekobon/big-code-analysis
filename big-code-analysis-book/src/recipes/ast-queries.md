# AST queries

Recipes that work with the parsed syntax tree directly: searching for
node types, counting them, or dumping the tree.

## Detect parse errors before committing

Tree-sitter exposes a synthetic `ERROR` node anywhere it could not
parse. Use `find` to surface them:

```bash
bca \
    --include "*.rs" \
    --paths "$PWD" \
    find ERROR
```

> **Flag ordering.** `--include` and `--exclude` are variadic and
> consume tokens until the next flag begins, so put them **before**
> `--paths` to avoid the subcommand name being eaten as a glob. The
> single-value `=` form (`--include="*.rs"`) also works.

A clean run prints nothing. Wire this into a pre-commit hook to fail
fast when a syntactically broken file is staged.

## Count specific syntactic constructs

`count` accepts one or more node-type names and reports the totals.
For example, to count `if`, `for`, and `while` constructs across a
Rust project:

```bash
bca \
    --include "*.rs" \
    --paths src/ \
    count if_expression for_expression while_expression
```

The exact node-type names come from the underlying tree-sitter
grammar. To discover them, dump the AST of a small sample file
(see below) and read the node names off the tree.

## Find all `unsafe` blocks in a Rust crate

```bash
bca \
    --include "*.rs" \
    --paths src/ \
    find unsafe_block
```

Each match prints the file path and the line range of the node.

## Dump the AST of a file

Useful for understanding why a metric came out the way it did, or for
discovering the tree-sitter node names you need for `find` / `count`:

```bash
bca --paths src/lib.rs dump
```

To narrow the dump to a specific function or block, add line bounds
with the global `--ls` and `--le` flags:

```bash
bca \
    --paths src/lib.rs \
    --ls 42 --le 88 \
    dump
```

`--ls` / `--le` apply to `dump` and `find`, so the same range can be
used to scope a search to a single function:

```bash
bca \
    --paths src/lib.rs \
    --ls 42 --le 88 \
    find return_expression
```

## List every function or method

For a quick human-readable inventory:

```bash
bca \
    --include "*.rs" \
    --paths src/ \
    functions
```

The output is a tree per file: an `In file …` header followed by an
indented row per function with name and line span. It is intended for
reading, not parsing.

For tooling that needs a structured inventory — coverage mapping,
documentation generation, code-owner reports — use the JSON `metrics`
output instead and walk `.spaces[]` recursively, taking entries whose
`kind` is `function`:

```bash
bca \
    --include "*.rs" \
    --paths src/ \
    metrics -O json \
  | jq -c '
      . as $root
      | def funcs: if .kind == "function" then [.] else [] end
                   + (.spaces // [] | map(funcs) | add // []);
      funcs[] | {file: $root.name, name, start_line, end_line}
    '
```

This emits one JSON object per function and is safe to pipe into
downstream tooling.
