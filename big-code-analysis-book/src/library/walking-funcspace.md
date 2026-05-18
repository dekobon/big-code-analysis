# Walking `FuncSpace` results

[`FuncSpace`][FuncSpace] is the tree the library hands back from
[`analyze`]. The top-level node represents the whole file; its
`spaces` field holds nested function / class / impl / trait /
namespace spaces. Each node carries the same
[`CodeMetrics`][CodeMetrics] payload, so any metric is available at
any level of granularity.

## Anatomy of a `FuncSpace`

The fields you reach for most often are:

| Field         | Type                | What it is                                    |
|---------------|---------------------|-----------------------------------------------|
| `name`        | `Option<String>`    | Caller-supplied identifier (top-level) or symbol name (nested) |
| `kind`        | `SpaceKind`         | `Unit`, `Function`, `Class`, `Impl`, …        |
| `start_line`  | `usize`             | First line (1-based)                          |
| `end_line`    | `usize`             | Last line (1-based)                           |
| `spaces`      | `Vec<FuncSpace>`    | Nested spaces                                 |
| `metrics`     | `CodeMetrics`       | All per-space metric values                   |
| `suppressed`  | `SuppressionScope`  | In-source suppression markers                 |

[`SpaceKind`][SpaceKind] is an enum — match on it to filter what
you care about (`Function` only, or "anything that owns methods").

## Recursive walk

Recursion mirrors the tree shape. Here we collect every function
space whose cognitive complexity exceeds a threshold:

```rust
use big_code_analysis::{
    analyze, FuncSpace, MetricsOptions, SpaceKind, Source, LANG,
};

fn hotspots(space: &FuncSpace, threshold: f64, out: &mut Vec<String>) {
    if space.kind == SpaceKind::Function
        && space.metrics.cognitive.cognitive_sum() > threshold
    {
        if let Some(name) = &space.name {
            out.push(format!(
                "{name} (lines {}–{})",
                space.start_line, space.end_line,
            ));
        }
    }
    for child in &space.spaces {
        hotspots(child, threshold, out);
    }
}

fn main() {
    let source = b"\
fn easy() { let _ = 1; }
fn hard(x: i32) -> i32 {
    if x > 0 { if x > 10 { 1 } else { 2 } } else { 3 }
}
";
    let space = analyze(
        Source::new(LANG::Rust, source).with_name(Some("snippet.rs".to_owned())),
        MetricsOptions::default(),
    )
    .expect("parses");

    let mut hits = Vec::new();
    hotspots(&space, 2.0, &mut hits);
    for hit in hits {
        println!("{hit}");
    }
}
```

## Iterative walk

For deep trees, prefer an explicit stack — Rust does not
tail-call-optimise, and pathological generated code can be
arbitrarily nested:

```rust
use big_code_analysis::FuncSpace;

fn total_functions(root: &FuncSpace) -> usize {
    let mut stack = vec![root];
    let mut count = 0;
    while let Some(space) = stack.pop() {
        if space.kind == big_code_analysis::SpaceKind::Function {
            count += 1;
        }
        stack.extend(space.spaces.iter());
    }
    count
}
```

## Reading per-metric numbers

`CodeMetrics` exposes each metric as its own `Stats` struct.
Inside, each struct offers integer-valued summary accessors plus
per-space derived ones. A few patterns:

```rust
use big_code_analysis::FuncSpace;

fn summary(space: &FuncSpace) {
    let m = &space.metrics;

    println!("cognitive (this space):     {}", m.cognitive.cognitive_sum());
    println!("cyclomatic (this space):    {}", m.cyclomatic.cyclomatic_sum());
    println!("# functions in this space:  {}", m.nom.functions_sum());
    println!("source lines (sloc):        {}", m.loc.sloc());
    println!("physical lines (ploc):      {}", m.loc.ploc());
    println!("ABC branches:               {}", m.abc.branches());
}
```

The `*_sum` accessors aggregate across child spaces; bare
accessors like `m.loc.sloc()` are the value attributable to *this*
node. The full list of fields and methods lives in the
[per-metric rustdoc][docs-metrics].

## Don't rely on traversal order

The library walks the AST in source order, but the contract is
only that every space appears once in the tree. If you need a
stable order across versions, sort by `start_line` after the
walk:

```rust
use big_code_analysis::FuncSpace;

fn flatten(space: &FuncSpace, out: &mut Vec<(usize, String)>) {
    if let Some(name) = &space.name {
        out.push((space.start_line, name.clone()));
    }
    for child in &space.spaces {
        flatten(child, out);
    }
}

fn sorted(space: &FuncSpace) -> Vec<(usize, String)> {
    let mut v = Vec::new();
    flatten(space, &mut v);
    v.sort_by_key(|&(line, _)| line);
    v
}
```

[`analyze`]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.analyze.html
[FuncSpace]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.FuncSpace.html
[CodeMetrics]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.CodeMetrics.html
[SpaceKind]: https://docs.rs/big-code-analysis/*/big_code_analysis/enum.SpaceKind.html
[docs-metrics]: https://docs.rs/big-code-analysis/*/big_code_analysis/
