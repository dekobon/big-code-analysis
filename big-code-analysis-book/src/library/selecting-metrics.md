# Selecting metrics

By default, every call to [`analyze`] computes the full metric
suite — ABC, cognitive, cyclomatic, Halstead, LoC, MI, NArgs,
NExits, NOM, NPA, NPM, tokens, and WMC. That is the right default
for the CLI, where the user has just asked for *the* metrics, but
it is heavyweight for callers that only want one number per file.

`MetricsOptions::with_only(&[Metric])` lets you restrict the walker
to a subset of metrics. Unselected metrics are skipped at the
per-node level — no `T::Halstead::compute`, no
`T::Cognitive::compute`, etc. — and elided from the
[`CodeMetrics`][CodeMetrics] serialization output.

## A worked example

Compute LoC only, then read the result:

```rust
use big_code_analysis::{analyze, LANG, Metric, MetricsOptions, Source};

fn main() {
    let source = b"fn f(x: i32) -> i32 { if x > 0 { 1 } else { 0 } }";

    let opts = MetricsOptions::default().with_only(&[Metric::Loc]);
    let space = analyze(
        Source::new(LANG::Rust, source).with_name(Some("snippet.rs".to_owned())),
        opts,
    )
    .expect("parses");

    // LoC was selected — it carries real numbers.
    println!("ploc = {}", space.metrics.loc.ploc());

    // Halstead, cognitive, cyclomatic, … were skipped. Their
    // `Stats` fields are at `Default` and elided from JSON output.
    let json = serde_json::to_string_pretty(&space.metrics).unwrap();
    println!("{json}");
}
```

The JSON output for that call contains only the `loc` object;
every other metric is absent.

## Dependencies between metrics

Two metrics are *derived* — they consume the outputs of other
metrics during the finalize step:

| Metric | Dependencies |
|---|---|
| `Metric::Mi`  | `Loc`, `Cyclomatic`, `Halstead` |
| `Metric::Wmc` | `Cyclomatic`, `Nom` |

`with_only` resolves these closures silently. Asking for `Mi`
alone still computes `Loc + Cyclomatic + Halstead`, so the MI
value is meaningful rather than a function of zero-default
inputs:

```rust
# use big_code_analysis::{Metric, MetricSet, MetricsOptions};
let opts = MetricsOptions::default().with_only(&[Metric::Mi]);
// opts.metrics now contains Mi + Loc + Cyclomatic + Halstead.
```

You can introspect the final set from the resulting
[`FuncSpace`][FuncSpace] via `space.metrics.selected()`:

```rust,no_run
# use big_code_analysis::{analyze, LANG, Metric, MetricsOptions, Source};
let space = analyze(
    Source::new(LANG::Rust, b"fn f() {}"),
    MetricsOptions::default().with_only(&[Metric::Mi]),
).unwrap();
let sel = space.metrics.selected();
assert!(sel.contains(Metric::Mi));
assert!(sel.contains(Metric::Loc)); // auto-added dependency
```

## Default behaviour is unchanged

`MetricsOptions::default()` selects every metric. The
pre-#257 entry points (`analyze` without `with_only`, plus the
deprecated `metrics` / `metrics_with_options` shims) produce
byte-for-byte the same JSON they always did.

## What about "everything except *X*"?

There is no built-in complement API — `with_only` takes a positive
selection, not an exclusion list. The intentional asymmetry keeps
the dependency closure unambiguous: a positive list always grows
through `Metric::dependencies`, whereas an exclusion list would
need to decide what to do when the caller excludes a dependency
of a metric they kept.

If you genuinely want "all except Halstead", build the list
explicitly. Because `Metric` is `#[non_exhaustive]`, downstream
crates can construct the variants but cannot exhaustively `match`
on them, so the conventional pattern is to enumerate the variants
you want and accept that adding a future `Metric` variant will not
silently opt you in:

```rust
use big_code_analysis::{Metric, MetricsOptions};

let opts = MetricsOptions::default().with_only(&[
    Metric::Cognitive,
    Metric::Cyclomatic,
    Metric::Loc,
    Metric::Nom,
    Metric::Tokens,
    Metric::NArgs,
    Metric::Exit,
    Metric::Abc,
    Metric::Npm,
    Metric::Npa,
    Metric::Wmc,
    // Metric::Mi intentionally omitted: it would pull Halstead
    // back in via the dependency closure.
]);
```

Note the trap: keeping `Metric::Mi` re-adds `Metric::Halstead`
through `Metric::dependencies`. To truly drop Halstead you must
also drop `Mi`.

## When to reach for `with_only`

- **Hot paths** that need only one or two metrics per file —
  Halstead in particular owns its own per-space `HalsteadMaps`
  allocation and is the headline saving for an LoC-only run.
- **CI integrations** that only display one number (e.g. a
  cognitive-complexity gate) and want the rest of `CodeMetrics`
  to drop out of the cached JSON payload.
- **Library callers** wiring `big-code-analysis` into their own
  reports who would otherwise see fields for every metric in
  their own UI.

Per-metric Cargo features (compile-time stripping) are not
covered by this knob; they remain tracked separately under the
grammar-feature work (#252).

[`analyze`]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.analyze.html
[FuncSpace]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.FuncSpace.html
[CodeMetrics]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.CodeMetrics.html
