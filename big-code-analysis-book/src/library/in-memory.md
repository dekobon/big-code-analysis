# Analyzing in-memory source

`big-code-analysis` never requires source to live on disk. The
recommended entry point [`analyze`] takes a [`Source`] carrying the
language, source bytes, and an *optional* caller-supplied display
name; no filesystem path is involved unless the C/C++ preprocessor
lookup needs one (`Source::preproc_path`).

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
use big_code_analysis::{analyze, MetricsOptions, Source, LANG};

fn analyze_buffer(source: &[u8]) -> Option<f64> {
    // `Source::name` is the display identifier baked into the
    // top-level `FuncSpace`. Pick whatever is meaningful for
    // downstream consumers (logs, JSON output); pass `None` if
    // you have nothing useful to attach.
    let space = analyze(
        Source::new(LANG::Python, source).with_name(Some("<stdin>".to_owned())),
        MetricsOptions::default(),
    )
    .ok()?;

    Some(space.metrics.cognitive.cognitive_sum())
}
```

`Source::new` borrows the source bytes — the caller retains
ownership. If your downstream pipeline needs to highlight findings
on the same bytes, you can keep using the original buffer after
`analyze` returns.

## Reading from stdin

```rust
use std::io::{self, Read};

use big_code_analysis::{analyze, MetricsOptions, Source, LANG};

fn main() -> io::Result<()> {
    let mut source = Vec::new();
    io::stdin().read_to_end(&mut source)?;

    let space = match analyze(
        Source::new(LANG::Javascript, &source)
            .with_name(Some("<stdin>".to_owned())),
        MetricsOptions::default(),
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
[`guess_language`] with `analyze`. `guess_language` peeks at the
path extension, an Emacs mode-line, and the shebang in that order:

```rust
use std::path::PathBuf;

use big_code_analysis::{analyze, guess_language, MetricsOptions, Source};

fn analyze_unknown(path: PathBuf, source: Vec<u8>) -> Option<()> {
    let (lang, _name) = guess_language(&source, &path);
    let lang = lang?;
    // `.ok()?` collapses `MetricsError` into `None` so this helper's
    // `Option` return shape is preserved. See `error-handling.md` for
    // a richer mapping that preserves the variant.
    let _space = analyze(
        Source::new(lang, &source)
            .with_name(path.to_str().map(str::to_owned)),
        MetricsOptions::default(),
    )
    .ok()?;
    Some(())
}
```

`guess_language` returns `(None, _)` for unrecognised extensions —
treat that as "skip" rather than as a hard error.

## Watch out for these

- **Name identity matters.** Top-level `FuncSpace::name` is whatever
  string you put in `Source::name`. Two analyses sharing the same
  name will look identical to a downstream consumer that keys on
  it. Use distinct labels for distinct buffers.
- **`Source::name` is `Option<String>`.** Passing `None` leaves the
  top-level `FuncSpace::name` as `None` — useful for ad-hoc
  snippets that have no meaningful identity. Downstream consumers
  that *require* a stable identifier should check for `None`
  explicitly.
- **No filesystem fallback.** Unlike the CLI, the library does not
  read sibling files, follow `#include`s, or interpret a
  `.gitignore`. Feed it exactly the bytes you want analyzed.

## Alternative: the path-positional shim

For backwards compatibility, the older path-positional entry points
([`get_function_spaces`] and [`metrics_with_options`]) still work
but are `#[deprecated]` in favour of `analyze`. They derive
`FuncSpace::name` from the supplied `&Path` via lossy UTF-8
conversion and are otherwise equivalent.

[FuncSpace]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.FuncSpace.html
[`analyze`]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.analyze.html
[`Source`]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Source.html
[`guess_language`]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.guess_language.html
[`get_function_spaces`]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.get_function_spaces.html
[`metrics_with_options`]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.metrics_with_options.html
