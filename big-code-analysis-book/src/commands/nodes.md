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

## Semantic filters {#semantic-filters}

Six `-t/--type` values are not node-type names but *semantic* filters,
resolved per language so that one spelling works across every grammar.
They take precedence over an identically named node type, so a grammar
with a literal `call` node cannot be matched by name — use its numeric
`kind_id` for that.

- `all` — every node, named and anonymous. `bca count -t all` therefore
  always reports 100%.
- `function` — a *named* function, method or other callable
  declaration. Anonymous ones are excluded where the language
  distinguishes them: in the JavaScript family an arrow function or
  function expression bound to a name (`const add = (a, b) => …`) is a
  `function` match, while the same expression passed inline as a
  callback (`xs.map(x => x * 2)`) is a closure and is not.
- `call` — a *call site*: a function, method or command invocation a
  reader would navigate to. Object construction (`new T(…)`) and
  constructor delegation (C#'s `: base(x)` and a C# 12 primary
  constructor's `: Base(x)`, Java's `super(…)`, Kotlin's superclass
  call) are deliberately **excluded**. They are counted by the
  [ABC](../metrics.md#abc) `branches` axis, whose Fitzpatrick rule is
  "function invocation or object creation" — a wider question than "where
  is this called". Expect `abc.branches` to exceed a file's `call` count
  in C#, Java, Groovy, Kotlin, C++, JavaScript, TypeScript, PHP, Ruby
  and Rust; see
  [Where a constructor call lands](../metrics.md#abc-constructor-attribution).
- `comment` — a comment of any of the language's forms.
- `string` — a string *literal*. Type-annotation keywords that share the
  literal's node name (TypeScript's `: string`) are excluded.
- `error` — a parse-error node. `-t ERROR` matches the same nodes by
  name and is the spelling used above.

## Counting nodes {#counting-nodes}

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
deprecated aliases but are slated for removal in the next major.

## Listing functions

For a list of every function or method and its line span, use:

```bash
bca functions /path/to/your/file/or/directory
```
