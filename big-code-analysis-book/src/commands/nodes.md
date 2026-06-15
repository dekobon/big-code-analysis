# Nodes

`bca` provides commands to analyze and extract
information about nodes in the **Abstract Syntax Tree (AST)** of a
source file.

> **Migrating?** The verbs below replace the pre-restructure flag
> actions (`-d`, `-f`, `--count`, ...). See the
> [migration guide](../migration.md).

## Error detection

To detect syntactic errors in your code, run:

```bash
bca find -t ERROR -I "*.ext" /path/to/your/file/or/directory
```

- `[PATHS]...` / `-p, --paths`: file or directory to analyze (analyzes
  all files when given a directory). Paths are given positionally or via
  `--paths`; both are unioned. Flags follow the subcommand.
- `-t, --type`: the node type to match. Repeat the flag for several
  types (`-t function_item -t struct_item`); at least one is required. A
  *string* value matches the node-type name exactly (for example
  `function_item`). A purely *numeric* value is instead interpreted as a
  raw tree-sitter `kind_id` and matches nodes whose internal symbol id
  equals that number (so `-t 0` matches the end/`ERROR` sentinel). The
  numeric form is an escape hatch for grammar inspection and is unstable:
  a `kind_id` is an index into the grammar's symbol table, so the same
  number names a different node after a grammar-version bump. Prefer the
  string form unless you specifically need a kind that has no stable name.
- `-I, --include`: glob filter for selecting files by extension (e.g.
  `*.js`, `*.rs`). Each `-I` takes exactly one value, so a following
  positional path is never swallowed.

## Counting nodes

Count occurrences of one or more node types with the `count` command:

```bash
bca count -t <NODE_TYPE> [-t <NODE_TYPE>...] -I "*.ext" \
    /path/to/your/file/or/directory
```

## Printing the AST

To visualize the AST of a source file, use the `dump` command (which
requires an explicit path — a whole-tree AST dump is never useful):

```bash
bca dump /path/to/your/file/or/directory
```

## Analyzing code portions

To analyze only a specific portion of the code, use the `dump`
subcommand's `--line-start` and `--line-end` options. For example, to
print the AST of a single function from line 5 to line 10:

```bash
bca dump --line-start 5 --line-end 10 /path/to/your/file/or/directory
```

These flags are specific to `dump` and `find`, so they must follow the
subcommand. The short `--ls` / `--le` spellings still work as
deprecated aliases but are slated for removal in 2.0.

## Listing functions

For a list of every function or method and its line span, use:

```bash
bca functions /path/to/your/file/or/directory
```
