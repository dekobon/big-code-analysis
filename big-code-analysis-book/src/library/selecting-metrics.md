# Selecting metrics

*Planned — not yet shipped. See [issue #257].*

[issue #257]: https://github.com/dekobon/big-code-analysis/issues/257

## What this page will cover

Today, every call to [`analyze`] runs the full metric suite — ABC,
cognitive, cyclomatic, Halstead, LoC, MI, NArgs, NExits, NOM, NPA,
NPM, tokens, WMC. That is the right default for the CLI, where the
user has just asked for "the metrics", but it is heavyweight for
callers that want one number per file.

The planned [#257] work splits each metric behind its own Cargo
feature flag so consumers can compile the library with only the
metrics they need. Compile time, binary size, and runtime cost all
drop in tandem.

## What you can do today

Two interim options:

1. **Run the full suite and read one field.** Every metric value
   lives on [`FuncSpace::metrics`][FuncSpace]. Computing the
   others alongside is cheap enough that this is the right call
   for one-shot tools and CI.

   ```rust
   use big_code_analysis::{analyze, LANG, MetricsOptions, Source};

   fn main() {
       let space = analyze(
           Source::new(
               LANG::Rust,
               b"fn f(x: i32) -> i32 { if x > 0 { 1 } else { 0 } }",
           )
           .with_name(Some("snippet.rs".to_owned())),
           MetricsOptions::default(),
       )
       .expect("parses");

       // Read just one metric; ignore the rest.
       println!("{}", space.metrics.cognitive.cognitive_sum());
   }
   ```

2. **Use [`MetricsOptions`][MetricsOptions] for the knobs that
   already exist.** Today the only switch is `exclude_tests`,
   which prunes Rust `#[test]` / `#[cfg(test)]` subtrees before
   the metric walk runs. That is closer to "scope selection" than
   "metric selection", but if you only care about non-test code it
   keeps the numbers tighter:

   ```rust
   use big_code_analysis::{analyze, LANG, MetricsOptions, Source};

   fn main() {
       let options = MetricsOptions::default().with_exclude_tests(true);
       let _space = analyze(
           Source::new(LANG::Rust, b"fn lib() {} #[test] fn t() {}")
               .with_name(Some("snippet.rs".to_owned())),
           options,
       );
   }
   ```

When [#257] lands this page will document the feature-flag
matrix and any opt-in flags on `MetricsOptions` that replace the
"run-everything-and-ignore" workaround.

[`analyze`]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.analyze.html
[FuncSpace]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.FuncSpace.html
[MetricsOptions]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.MetricsOptions.html
