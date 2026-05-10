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
bca -I "*.ext" -p /path/to/your/file/or/directory find ERROR
```

- `-p, --paths`: file or directory (analyzes all files when given a
  directory).
- `-I, --include`: glob filter for selecting files by extension (e.g.
  `*.js`, `*.rs`). Variadic — put it **before** `-p` so the
  subcommand isn't swallowed as another glob, or use the `-I=GLOB`
  single-value form.
- `find <NODE>`: search for nodes of a specific type (one or more
  positional names).

## Counting nodes

Count occurrences of one or more node types with the `count` command:

```bash
bca -I "*.ext" -p /path/to/your/file/or/directory \
    count <NODE_TYPE> [<NODE_TYPE>...]
```

## Printing the AST

To visualize the AST of a source file, use the `dump` command:

```bash
bca -p /path/to/your/file/or/directory dump
```

## Analyzing code portions

To analyze only a specific portion of the code, use the global `--ls`
(line start) and `--le` (line end) options. For example, to print the
AST of a single function from line 5 to line 10:

```bash
bca -p /path/to/your/file/or/directory --ls 5 --le 10 dump
```

## Listing functions

For a list of every function or method and its line span, use:

```bash
bca -p /path/to/your/file/or/directory functions
```
