# Quick start

This page shows the minimum code needed to compute metrics from a
string of source code. It assumes a working Rust toolchain and a
crate to add the dependency to.

## 1. Add the crate

```toml
# Cargo.toml
[dependencies]
big-code-analysis = "2.4.0"
```

The crate uses Rust edition 2024 and pins `rust-version = "1.94"`.
Older toolchains will not build it; see the
[MSRV section of STABILITY.md][stability-msrv] for the policy.

[stability-msrv]: https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md#msrv-policy

## 2. Compute metrics from a string

The recommended entry point is [`analyze`]: pass a [`Source`]
carrying the language, source bytes, and an optional display name,
plus a [`MetricsOptions`] for any per-traversal flags. No
filesystem path is needed.

```rust
use big_code_analysis::{analyze, MetricsOptions, Source, LANG};

fn main() {
    let source = "fn add(a: i32, b: i32) -> i32 { a + b }";

    let space = analyze(
        Source::new(LANG::Rust, source.as_bytes())
            .with_name(Some("snippet.rs".to_owned())),
        MetricsOptions::default(),
    )
    .expect("Rust source should parse");

    println!(
        "cognitive complexity (file-level): {}",
        space.metrics.cognitive.cognitive_sum(),
    );
}
```

`Source::name` becomes the top-level [`FuncSpace::name`]; passing
`None` leaves the top-level name unset. The return type is
[`Result<FuncSpace, MetricsError>`][MetricsError]. In practice the
`Err` variant means the requested language's Cargo feature is
disabled in this build. A parse failure does **not** produce `Err`:
[tree-sitter](https://tree-sitter.github.io/tree-sitter/) recovers
with `ERROR` nodes. See [Error handling](error-handling.md) for the
variant set and matching patterns. `MetricsError` is
`#[non_exhaustive]`, so always include a `_` arm when matching.

Tip: `use big_code_analysis::prelude::*;` brings the recommended
entry points (`analyze`, `Ast`, `Source`, `MetricsOptions`,
`MetricsError`, `FromPathError`, `SkipReason`, `LANG`, `FuncSpace`,
`CodeMetrics`, `SpaceKind`, `Metric`) into scope in one line.
Anything outside the prelude is still reachable by name, for example
`use big_code_analysis::guess_language;`.

> Need more than metrics from one parse (operators and operands, an
> AST dump, a function-span list)? Parse once with [`Ast::parse`] and
> call the per-pass methods on the handle. See
> [Parse once, run metrics many times](parse-once.md). If you already
> drive your own `tree_sitter::Parser`, adopt the resulting tree with
> [`Ast::from_tree_sitter`] (see
> [Reusing an existing tree-sitter Tree](reuse-tree.md)).

[`Ast::parse`]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Ast.html#method.parse
[`Ast::from_tree_sitter`]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Ast.html#method.from_tree_sitter

## 3. Inspect the result {#3-what-you-got-back}

`FuncSpace` is a tree of spaces. The top-level node represents the
whole file; its `spaces` field holds nested function, class and impl
spaces. Every node carries the same [`CodeMetrics`][CodeMetrics]
struct, so you can read any metric at any level.

```rust
use big_code_analysis::{analyze, MetricsOptions, Source, SpaceKind, LANG};

fn main() {
    let source = "\
fn outer() {
    fn inner() {}
}
";
    let space = analyze(
        Source::new(LANG::Rust, source.as_bytes())
            .with_name(Some("snippet.rs".to_owned())),
        MetricsOptions::default(),
    )
    .expect("Rust source should parse");

    assert_eq!(space.kind, SpaceKind::Unit);
    assert_eq!(space.spaces.len(), 1); // `outer`
    assert_eq!(space.spaces[0].spaces.len(), 1); // `inner`
}
```

For a deeper walk over `FuncSpace`, see
[Walking FuncSpace results](walking-funcspace.md).

## Picking a language

If you do not know the language up front, use [`guess_language`]. It
consults the path extension, an
[Emacs mode declaration](https://www.gnu.org/software/emacs/manual/html_node/emacs/Specifying-File-Variables.html)
in the buffer, and the shebang, in that order:

```rust
use std::path::PathBuf;

use big_code_analysis::{analyze, guess_language, MetricsOptions, Source};

fn main() {
    let source = b"print('hi')\n";
    let path = PathBuf::from("hello.py");

    let (Some(lang), _name) = guess_language(source, &path) else {
        eprintln!("unrecognised language");
        return;
    };

    let _space = analyze(
        Source::new(lang, source).with_name(Some("hello.py".to_owned())),
        MetricsOptions::default(),
    );
}
```

`guess_language` returns `(None, _)` only when the extension, the
mode declaration, and the shebang all fail to name a language; treat
that as "skip this file" rather than as a parse error.

[`analyze`]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.analyze.html
[`Source`]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Source.html
[`MetricsOptions`]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.MetricsOptions.html
[`FuncSpace::name`]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.FuncSpace.html#structfield.name
[`guess_language`]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.guess_language.html
[MetricsError]: https://docs.rs/big-code-analysis/*/big_code_analysis/enum.MetricsError.html
[CodeMetrics]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.CodeMetrics.html
