# big-code-analysis-ast

The parse and classification layer behind
[`big-code-analysis`](https://crates.io/crates/big-code-analysis): the
tree-sitter wrappers (`Node`, `Ancestors`), the generated per-grammar
kind enums, the `LANG` enum and language detection, the `Checker` /
`Getter` / `Alterator` classifiers, the C-family preprocessor pass,
comment stripping, and the AST dump. It computes no metric.

## This crate is internal plumbing

It is published only so that `big-code-analysis` can be. That crate
pins it at an exact `=X.Y.Z` version, the two are released together,
and nothing here carries a stability promise of its own: names,
signatures and module paths may change in any release, including a
patch. Depend on `big-code-analysis` and reach what it re-exports
(`LANG`, `Node`, `MetricsError`, `SpaceKind`, the AST dump types, the
preprocessor types, the file readers), which its
[`STABILITY.md`](https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md)
covers. Depend on this crate directly only when you accept re-pinning
on every release.

The split exists (#1376) so a second structural consumer — a linter, a
call-graph builder, a language server — can share one classification
layer without the metric machinery. Every per-language Cargo feature
of `big-code-analysis` (`rust`, `python`, …, `all-languages`) is a
feature of this crate under the same name, and the grammar crates are
dependencies here.

## License

MPL-2.0, like the rest of the repository.
