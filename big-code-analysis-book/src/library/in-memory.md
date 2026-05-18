# Analyzing in-memory source

`big-code-analysis` never requires source to live on disk. Every
public entry point accepts source bytes plus a virtual path; the
path is used only as an identifier and is recorded in
[`FuncSpace::name`][FuncSpace]. Nothing is read from the filesystem.

This is useful for:

- Scoring **generated code** before it is written out.
- Scoring **pre-processed** or **bundled** source (e.g. after a
  template expansion).
- Driving the analyzer from a **language server** or **editor
  plugin** that already holds the buffer in memory.
- **Stdin pipelines** and unit tests that should not touch the
  filesystem.

## Reading from a buffer

```rust
use std::path::PathBuf;

use big_code_analysis::{get_function_spaces, LANG};

fn analyze_buffer(source: &[u8]) -> Option<f64> {
    // The path is a label, not a filesystem read — pick whatever
    // is meaningful for downstream consumers (logs, JSON output).
    let path = PathBuf::from("<stdin>");

    // `get_function_spaces` returns `Result<FuncSpace, MetricsError>`;
    // collapse to `Option` at the boundary so this helper's signature
    // matches the caller's expectations.
    let space = get_function_spaces(
        &LANG::Python,
        source.to_vec(),
        &path,
        None,
    )
    .ok()?;

    Some(space.metrics.cognitive.cognitive_sum())
}
```

`get_function_spaces` takes a `Vec<u8>` by value — it consumes the
buffer rather than borrowing it. If your caller still needs the
source afterwards (e.g. to highlight findings), clone before
passing it in.

## Reading from stdin

```rust
use std::io::{self, Read};
use std::path::PathBuf;

use big_code_analysis::{get_function_spaces, LANG};

fn main() -> io::Result<()> {
    let mut source = Vec::new();
    io::stdin().read_to_end(&mut source)?;

    let path = PathBuf::from("<stdin>");
    let space = match get_function_spaces(
        &LANG::Javascript,
        source,
        &path,
        None,
    ) {
        Ok(space) => space,
        Err(err) => {
            eprintln!("parse failed: {err}");
            std::process::exit(1);
        }
    };

    println!("{}", space.metrics.cyclomatic.cyclomatic_sum());
    Ok(())
}
```

## Picking the language from content

If you do not know the language up front, combine
[`guess_language`] with `get_function_spaces`. `guess_language`
peeks at the path extension, an Emacs mode-line, and the shebang
in that order:

```rust
use std::path::PathBuf;

use big_code_analysis::{get_function_spaces, guess_language};

fn analyze(path: PathBuf, source: Vec<u8>) -> Option<()> {
    let (lang, _name) = guess_language(&source, &path);
    let lang = lang?;
    // `.ok()?` collapses `MetricsError` into `None` so this helper's
    // `Option` return shape is preserved. See `error-handling.md` for
    // a richer mapping that preserves the variant.
    let _space = get_function_spaces(&lang, source, &path, None).ok()?;
    Some(())
}
```

`guess_language` returns `(None, _)` for unrecognised extensions —
treat that as "skip" rather than as a hard error.

## Watch out for these

- **Path identity matters.** Top-level `FuncSpace::name` is derived
  from the path you pass in. Two analyses sharing the same virtual
  path will look identical to a downstream consumer that keys on
  it. Use distinct labels for distinct buffers.
- **Non-UTF-8 paths.** The path is stored via lossy UTF-8
  conversion. If you pass a non-UTF-8 path, `FuncSpace::name_was_lossy`
  is `true`; downstream consumers should not use the resulting
  string as a stable identifier.
- **No filesystem fallback.** Unlike the CLI, the library does not
  read sibling files, follow `#include`s, or interpret a
  `.gitignore`. Feed it exactly the bytes you want analyzed.

[FuncSpace]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.FuncSpace.html
[`guess_language`]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.guess_language.html
