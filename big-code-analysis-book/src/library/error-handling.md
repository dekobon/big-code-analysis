# Error handling

The entry point [`get_function_spaces`] returns
`Result<FuncSpace, MetricsError>`. This page documents what each
variant means and how to act on it.

> **Heads up.** Prior to [#253] this entry point returned
> `Option<FuncSpace>` and collapsed every failure mode into a single
> `None`. The `Result` variant set is additive — `MetricsError` is
> `#[non_exhaustive]`, so always include a `_` arm when matching
> exhaustively to stay forward-compatible with future variants.

[#253]: https://github.com/dekobon/big-code-analysis/issues/253

## Pattern-matching the error variants

```rust
use std::path::PathBuf;

use big_code_analysis::{get_function_spaces, LANG, MetricsError};

fn main() {
    let result = get_function_spaces(
        &LANG::Rust,
        b"this is not rust".to_vec(),
        &PathBuf::from("snippet.rs"),
        None,
    );

    match result {
        Ok(space) => println!("ok: {} lines", space.metrics.loc.sloc()),
        Err(MetricsError::EmptyRoot) => {
            eprintln!("walker produced no top-level FuncSpace");
        }
        Err(MetricsError::ParseHasErrors) => {
            eprintln!("tree-sitter reported syntax errors (strict mode)");
        }
        Err(MetricsError::LanguageDisabled(lang)) => {
            eprintln!("language {:?} is not enabled in this build", lang);
        }
        Err(MetricsError::NonUtf8Path) => {
            eprintln!("path is not valid UTF-8");
        }
        // `MetricsError` is `#[non_exhaustive]`; new variants may be added.
        Err(_) => eprintln!("unexpected MetricsError variant"),
    }
}
```

### What each variant means

- **`EmptyRoot`** — The walker reached the end of the AST without
  producing a top-level [`FuncSpace`]. The most common cause is empty
  input or input whose only content is comments. Defensive failures
  (the traversal produced no `Unit` space for any supported
  language) also surface here; if you hit one on real-world source,
  please file an issue.
- **`ParseHasErrors`** — Reserved for a future strict-parsing toggle
  on [`MetricsOptions`]. Not produced by today's default entry
  points; tree-sitter's error recovery is intentionally tolerant
  (see below).
- **`LanguageDisabled(LANG)`** — Reserved for upcoming per-language
  Cargo features (see [#252]). The current build enables every
  supported language, so this variant is never produced today.
- **`NonUtf8Path`** — Reserved for callers that opt into
  strict-identifier mode (see [#254]). The default entry points use
  lossy UTF-8 conversion at the [`FuncSpace::name`] boundary and
  flag it via `name_was_lossy`, so this variant is not produced
  today.

[#252]: https://github.com/dekobon/big-code-analysis/issues/252
[#254]: https://github.com/dekobon/big-code-analysis/issues/254

## Tree-sitter does not always say "no"

Most parse errors do *not* surface as `Err(_)`. Tree-sitter is an
**error-recovering** parser — it will produce a tree even for
syntactically broken input, marking the bad regions with `ERROR`
nodes. The metric walk happily computes numbers over the recovered
tree. That means:

- **Garbage in, numbers out.** Feeding C++ source to `LANG::Python`
  generally produces an `Ok(FuncSpace)` whose metrics are nonsense.
  Make sure you have selected the right language (e.g. via
  [`guess_language`]) before trusting the result.
- **Partial files score.** A truncated file with an unterminated
  brace will still return `Ok(FuncSpace)`. The metrics reflect the
  recovered tree, not the intended source.

If you need to know whether the input parsed cleanly, count
`ERROR` nodes by walking the tree-sitter AST yourself (see the
[`Node`][Node] escape hatch in
[`STABILITY.md`](https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md#escape-hatches)) or use the
[`bca nodes`](../commands/nodes.md) subcommand on the CLI side.

## Bubbling `MetricsError` through `?`

Because `MetricsError` implements [`std::error::Error`], you can
bubble it through any `Result<_, Box<dyn Error>>` chain without
boilerplate:

```rust
use std::error::Error;
use std::path::Path;

use big_code_analysis::{get_function_spaces, FuncSpace, LANG};

pub fn analyze(
    lang: LANG,
    source: Vec<u8>,
    path: &Path,
) -> Result<FuncSpace, Box<dyn Error>> {
    Ok(get_function_spaces(&lang, source, path, None)?)
}
```

If you want a project-specific error type, an explicit `From` impl
keeps call sites clean while letting you attach extra context
(file path, language guess, etc.).

## Warnings are not errors

The library writes warnings to **stderr** for non-fatal issues
(malformed `bca:` suppression markers, mainly). They do not abort
the walk and they do not flip `Ok` to `Err`. If you are running
embedded inside a server or library and need to capture those
warnings, redirect stderr at the process level — the library does
not currently expose a programmatic warning sink. That is tracked
under the library-DX umbrella ([#250]).

[`get_function_spaces`]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.get_function_spaces.html
[`MetricsOptions`]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.MetricsOptions.html
[`FuncSpace`]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.FuncSpace.html
[`FuncSpace::name`]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.FuncSpace.html#structfield.name
[`guess_language`]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.guess_language.html
[Node]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Node.html
[#250]: https://github.com/dekobon/big-code-analysis/issues/250
