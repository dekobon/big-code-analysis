# AST traversal

`bca.analyze(...)` gives you *metrics*. When you need the *syntax
tree* itself — to find every function definition, pull a docstring,
or port a [py-tree-sitter][pyts] matcher — parse once into an
[`Ast`](#the-ast-handle) and walk it with lazy
[`Node`](#the-node-handle) handles.

[pyts]: https://github.com/tree-sitter/py-tree-sitter

## The `Ast` handle

`bca.Ast.parse(code, language)` (or `bca.Ast.from_path(path)`) parses
the source **once** and hands back a handle you can draw both metrics
and the tree from, instead of parsing twice — once in py-tree-sitter,
once in `analyze()`:

```python
import big_code_analysis as bca

ast = bca.Ast.parse("fn main() { let x = 1 + 2; }", "rust")
ast.metrics()      # same dict as analyze_source(...)
ast.root_node      # the syntax tree, walked lazily (below)
```

The handle is immutable and thread-safe, so it composes with
`ThreadPoolExecutor` fan-out exactly like `analyze`.

## The `Node` handle

`ast.root_node` is the tree's root as a lazy `Node`. Unlike
[`ast.dump()`](#lazy-nodes-vs-dump), which materialises one dict per
node, a `Node` is a cursor into the retained tree: it costs nothing
until you read from it, and a selective extractor pays only for the
nodes it visits.

```python
root = ast.root_node
root.kind                       # "source_file"
root.children                   # list[Node], direct children
root.child_by_field_name("…")   # a field child, or None
node.text()                     # the node's source bytes
```

Traversal mirrors py-tree-sitter: `children` / `named_children`,
`parent`, `next_sibling` / `prev_sibling` (and the `*_named_*`
variants), `child(i)` / `named_child(i)`,
`child_by_field_name(name)` / `children_by_field_name(name)`, and the
`field_name` the parent reaches a node through.

### Walking the whole subtree

`walk()` is a lazy pre-order iterator over a node and its
descendants; `descendants_by_kind(kinds)` collects the matches in one
pass; and `ast.find(filters)` searches the whole tree, accepting the
same vocabulary as [`bca count`](../commands/count.md) (`function`,
`call`, `comment`, `string`, an exact kind, …):

```python
# Every function name in the file, the lazy way.
for fn in ast.find(["function_item"]):
    name = fn.child_by_field_name("name")
    print(name.text().decode())

# Or filter a subtree by raw grammar kind.
idents = root.descendants_by_kind(["identifier"])
```

These have Rust counterparts — `Node::preorder` and
`Node::descendants_by_kind` — so library callers get the same helpers.

## Coordinates

A node reports its one location in every vocabulary, so nothing has
to be converted by hand:

| Accessor | Meaning |
| --- | --- |
| `start_byte` / `end_byte` | byte offsets into `ast.source` |
| `start_point` / `end_point` | **0-based** `(row, col)` (py-tree-sitter parity) |
| `start_line` / `end_line` | **1-based** lines |
| `span` | the 1-based `{start_line, start_col, …, start_byte, end_byte}` dict `dump()` emits |

So `node.start_line == node.start_point[0] + 1`, and
`ast.source[node.start_byte:node.end_byte] == node.text()`.

## Lazy nodes vs. `dump()`

`ast.dump()` returns the tree as nested dicts; `ast.root_node` returns
lazy handles. They differ in two ways that matter:

1. **Memory.** `dump()` builds one dict (with `span`, `value`,
   `children`) per node — fine for small files, costly for the large
   ones. A `Node` walk allocates only the handles you touch.
2. **Taxonomy.** A `Node`'s `kind` is the **raw** grammar kind.
   `dump()` kinds pass through bca's `Alterator` and are curated — for
   example, string-literal nodes are renamed to `"string"` and
   *flattened* (their grammar children removed). So the two surfaces
   intentionally disagree on altered nodes:

   ```python
   ast = bca.Ast.parse('fn f() { let s = "hi"; }', "rust")
   # Raw tree: the string keeps its quote/content children.
   raw = next(n for n in ast.root_node.walk() if "string" in n.kind)
   assert raw.children
   ```

   Use lazy nodes when you want exactly what the grammar produced (the
   right choice for porting a py-tree-sitter matcher); use `dump()`
   when you want bca's curated, JSON-serialisable view.

## Lifetime and threading

A `Node` keeps its `Ast` alive: it stays valid even after you drop
every other reference to the parse, so returning a node (or a list of
nodes) from a function that builds the `Ast` locally is safe. Nodes
are also safe to share across threads.

> **C/C++ preprocessor.** For `Cpp` parsed with preprocessor inputs,
> `ast.source` — and therefore every node's byte offsets — indexes
> into the *expanded* source the parser saw, not the on-disk file.

## Where to go next

* [Metric selection](metrics.md) — compute only the metrics you need
  from the same parse.
* The CLI's [`dump`](../commands/dump.md) and
  [`count`](../commands/count.md) commands are the shell-level
  equivalents of `dump()` and `find()`.
