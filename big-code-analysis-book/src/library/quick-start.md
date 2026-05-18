# Quick start

This page walks through the minimum amount of code needed to compute
metrics from a string of source code.

## 1. Add the crate

```toml
# Cargo.toml
[dependencies]
big-code-analysis = "0.0.25"
```

The crate uses Rust edition 2024 and pins `rust-version = "1.94"`.
Older toolchains will not build it — see the
[MSRV section of STABILITY.md][stability-msrv] for the policy.

[stability-msrv]: https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md#msrv-policy

## 2. Compute metrics from a string

The simplest path is [`get_function_spaces`]: hand it a language
selector, the source bytes, and a "virtual" path that names the
file. The path is used purely as an identifier — nothing is read
from disk.

```rust
use std::path::PathBuf;

use big_code_analysis::{get_function_spaces, LANG};

fn main() {
    let source = "fn add(a: i32, b: i32) -> i32 { a + b }";
    let path = PathBuf::from("snippet.rs");

    let space = get_function_spaces(
        &LANG::Rust,
        source.as_bytes().to_vec(),
        &path,
        None, // No C/C++ preprocessor data.
    )
    .expect("Rust source should parse");

    println!(
        "cognitive complexity (file-level): {}",
        space.metrics.cognitive.cognitive_sum(),
    );
}
```

The return type is [`Option<FuncSpace>`][FuncSpace]. `None` means
the parser failed to produce a usable tree — see
[Error handling](error-handling.md) for the details.

## 3. What you got back

`FuncSpace` is a tree of spaces. The top-level node represents the
whole file; its `spaces` field holds nested function / class / impl
spaces. Every node carries the same [`CodeMetrics`][CodeMetrics]
struct, so you can read any metric at any level of granularity.

```rust
use std::path::PathBuf;

use big_code_analysis::{get_function_spaces, LANG, SpaceKind};

fn main() {
    let source = "\
fn outer() {
    fn inner() {}
}
";
    let path = PathBuf::from("snippet.rs");
    let space = get_function_spaces(
        &LANG::Rust,
        source.as_bytes().to_vec(),
        &path,
        None,
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

If you do not know the language up front, use [`guess_language`] —
it consults the path extension, an Emacs mode line in the buffer,
and the shebang in that order:

```rust
use std::path::PathBuf;

use big_code_analysis::{get_function_spaces, guess_language};

fn main() {
    let source = b"print('hi')\n";
    let path = PathBuf::from("hello.py");

    let (Some(lang), _name) = guess_language(source, &path) else {
        eprintln!("unrecognised language");
        return;
    };

    let _space = get_function_spaces(&lang, source.to_vec(), &path, None);
}
```

`guess_language` returns `(None, _)` for unknown extensions; treat
that as "skip this file" rather than as a parse error.

## What changes when

The function above is named `get_function_spaces` today. Issue
[#253] will rename the entry point and change the return type to
`Result<FuncSpace, MetricsError>`. The signature will become
something like:

```text
analyze(source, options) -> Result<FuncSpace, MetricsError>
```

Until that lands, the example on this page is the supported shape.

[#253]: https://github.com/dekobon/big-code-analysis/issues/253
[`get_function_spaces`]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.get_function_spaces.html
[`guess_language`]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.guess_language.html
[FuncSpace]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.FuncSpace.html
[CodeMetrics]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.CodeMetrics.html
