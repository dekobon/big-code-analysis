# Per-language Cargo features

Every tree-sitter grammar this library bundles is gated behind its
own Cargo feature. The default feature set is `all-languages`, so
the default

```toml
[dependencies]
big-code-analysis = "2.2.0"
```

pulls every grammar in — matching the library's historical
behaviour and what the `bca` / `bca-web` binaries themselves ship
with. The cost is concrete: every grammar crate compiles when the
library compiles, and every grammar's parsing tables stay live in
the final binary.

Library consumers that only need a subset of languages can opt out
of the defaults and re-enable just the grammars they care about.

## A worked example

A downstream service that only analyses Rust and TypeScript:

```toml
[dependencies]
big-code-analysis = { version = "2.2.0", default-features = false, features = ["rust", "typescript"] }
```

The library still compiles, the `LANG` enum still has every
variant, and `analyze` / `Ast` / the rest of the dispatch surface
still work for the enabled languages.

## Supported features

The following per-language features are available. Each feature
pulls in the matching grammar crate (and any helper grammars the
per-language pipeline depends on).

| Feature      | Grammar crates pulled in                                                       |
|--------------|--------------------------------------------------------------------------------|
| `bash`       | `tree-sitter-bash`                                                             |
| `c`          | `tree-sitter-c` (+ `c-family-helpers`); dedicated `C` variant owning `.c`, added in #721 |
| `c-family-helpers` | `bca-tree-sitter-ccomment`, `bca-tree-sitter-preproc` — **internal**, enabled automatically by `c` / `cpp` / `mozcpp`; gates the `Ccomment` / `Preproc` helper variants. Not meant to be selected directly |
| `cpp`        | `tree-sitter-cpp` (+ `c-family-helpers`); the `Cpp` variant, upstream grammar since #720. Also enables the `Ccomment` / `Preproc` helper variants |
| `csharp`     | `tree-sitter-c-sharp`                                                          |
| `elixir`     | `tree-sitter-elixir`                                                           |
| `go`         | `tree-sitter-go`                                                               |
| `groovy`     | `dekobon-tree-sitter-groovy`                                                   |
| `irules`     | `tree-sitter-irules` (F5 iRules, a Tcl dialect)                                |
| `java`       | `tree-sitter-java`                                                             |
| `javascript` | `tree-sitter-javascript`                                                       |
| `kotlin`     | `tree-sitter-kotlin-ng`                                                        |
| `lua`        | `tree-sitter-lua`                                                              |
| `mozcpp`     | `bca-tree-sitter-mozcpp` (+ `c-family-helpers`); opt-in Mozilla/Gecko C++ dialect, `Mozcpp` variant — owns no extensions, selected by name |
| `mozjs`      | `bca-tree-sitter-mozjs`                                                        |
| `objc`       | `tree-sitter-objc`; the `Objc` variant (Objective-C). Owns `.m`; `.mm` Objective-C++ stays on `Cpp` |
| `perl`       | `tree-sitter-perl`                                                             |
| `php`        | `tree-sitter-php`                                                              |
| `python`     | `tree-sitter-python`                                                           |
| `ruby`       | `tree-sitter-ruby`                                                             |
| `rust`       | `tree-sitter-rust`                                                             |
| `tcl`        | `bca-tree-sitter-tcl`                                                          |
| `typescript` | `tree-sitter-typescript` (used by both the `Typescript` and `Tsx` variants)    |

The umbrella `all-languages` feature enables every entry in this
table. The `bca-tree-sitter-*` crates are in-tree forks of the
upstream Mozilla / community grammars; the Rust import path remains
`tree_sitter_<lang>` regardless. See
[`RELEASING.md`](https://github.com/dekobon/big-code-analysis/blob/main/RELEASING.md#vendored-tree-sitter-grammar-publishability)
for the rename rationale and the workspace `package = ...` alias
trick that keeps consumer call sites unchanged.

## What happens when a feature is off

The `LANG` enum keeps every variant defined regardless of the
active feature set — disabling a feature does not change the enum
surface or any of the file-extension / emacs-mode detection
helpers. Selecting a `LANG` whose feature is off only affects the
dispatch path.

Every dispatch entry point that returns a `Result` surfaces the
disabled state as `Err(MetricsError::LanguageDisabled(LANG))`:

- [`analyze`]
- `Ast::parse` / `Ast::from_tree_sitter` (and the `metrics` / `ops`
  methods on the returned `Ast`)
- [`LANG::tree_sitter_language`] — this returns
  `Result<tree_sitter::Language, MetricsError>` rather than the bare
  `Language` the upstream project returned

Callers can query the compiled-in set without going through a
dispatcher:

```rust
use big_code_analysis::LANG;

for lang in LANG::into_enum_iter() {
    if lang.is_enabled() {
        println!("{:?} is compiled in", lang);
    }
}
```

This pairs well with the
[`get_language_for_file`](https://docs.rs/big-code-analysis/latest/big_code_analysis/fn.get_language_for_file.html) /
[`guess_language`](https://docs.rs/big-code-analysis/latest/big_code_analysis/fn.guess_language.html)
helpers, which still hand back any `LANG` variant for a recognised
extension — callers walking a directory may want to skip files
whose language is not enabled in the current build.

## Stability

Per-language features are themselves part of the contract. Adding a
new language feature is an additive minor-bump change; removing one
is a major-bump (`3.0`) break. The `all-languages` default is
permanent within `2.x`, so the variants a default build covers do not
shrink before `3.0`. Any such change will be flagged in the changelog
under **(breaking)**.

[`analyze`]: https://docs.rs/big-code-analysis/latest/big_code_analysis/fn.analyze.html
[`LANG::tree_sitter_language`]: https://docs.rs/big-code-analysis/latest/big_code_analysis/enum.LANG.html#method.tree_sitter_language
