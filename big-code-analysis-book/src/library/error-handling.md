# Error handling

The entry point [`analyze`] returns `Result<FuncSpace, MetricsError>`.
This page documents what each variant means and how to act on it.

> `MetricsError` is `#[non_exhaustive]`, so always include a `_` arm
> when matching to stay forward-compatible with future variants.

## Pattern-matching the error variants

```rust
use big_code_analysis::{analyze, LANG, MetricsError, MetricsOptions, Source};

fn main() {
    let result = analyze(
        Source::new(LANG::Rust, b"this is not rust")
            .with_name(Some("snippet.rs".to_owned())),
        MetricsOptions::default(),
    );

    match result {
        Ok(space) => println!("ok: {} lines", space.metrics.loc.sloc()),
        Err(MetricsError::EmptyRoot) => {
            eprintln!("walker produced no top-level FuncSpace");
        }
        Err(MetricsError::LanguageDisabled(lang)) => {
            eprintln!("language {:?} is not enabled in this build", lang);
        }
        // `MetricsError` is `#[non_exhaustive]`; new variants may be added.
        Err(_) => eprintln!("unexpected MetricsError variant"),
    }
}
```

### What each variant means

- **`EmptyRoot`**: reserved; not produced today. The metric walker
  always pushes a synthetic top-level `Unit` [`FuncSpace`] before
  walking the AST, so every parse (including empty, whitespace-only,
  and comment-only input) returns `Ok(FuncSpace { kind: Unit, .. })`.
  The variant is kept for a future walker change that could let the
  state stack legitimately drain to empty.
- **`LanguageDisabled(LANG)`**: the requested `LANG` is not enabled
  in this build. Every dispatch entry point produces it when the
  caller selects a `LANG` whose per-language Cargo feature is off.
  The default feature set (`all-languages`) compiles every grammar
  in, so you only see this variant after opting into a narrower set
  (`--no-default-features --features rust,…`).

`MetricsError` has no `ParseHasErrors` or `NonUtf8Path` variant.
Since it stays `#[non_exhaustive]`, a future strict-parsing or
strict-identifier mode can introduce them without a breaking change.
Non-UTF-8 paths are already handled up front: the recommended
[`analyze`] entry point takes a caller-supplied [`Source::name`]
(`Option<String>`), so a lossy path is never round-tripped in the
first place. The file-backed [`Ast::from_path`][ast_from_path] is the
one entry point that reads a path itself, and it returns its own
`#[non_exhaustive]` [`FromPathError`][FromPathError] (`Io`,
`NonUtf8Path`, `Unreadable`, `UnknownLanguage`, `Parse(MetricsError)`)
rather than widening `MetricsError`.

## Tree-sitter does not always say "no"

Most parse errors do *not* surface as `Err(_)`.
[Tree-sitter](https://tree-sitter.github.io/tree-sitter/) is an
**error-recovering** parser: it produces a tree even for
syntactically broken input, marking the bad regions with `ERROR`
nodes. The metric walk computes numbers over the recovered tree.
That means:

- **Garbage in, numbers out.** Feeding C++ source to `LANG::Python`
  generally produces an `Ok(FuncSpace)` whose metrics are nonsense.
  Select the right language (via [`guess_language`], for example)
  before trusting the result.
- **Partial files score.** A truncated file with an unterminated
  brace still returns `Ok(FuncSpace)`. The metrics reflect the
  recovered tree, not the intended source.

If you need to know whether the input parsed cleanly, count
`ERROR` nodes by walking the tree-sitter AST yourself (see the
[`Node`][Node] escape hatch in
[`STABILITY.md`](https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md#escape-hatches)) or use
`bca find -t ERROR` on the CLI side (see the
[Nodes](../commands/nodes.md) page).

## Bubbling `MetricsError` through `?`

Because `MetricsError` implements [`std::error::Error`], you can
bubble it through any `Result<_, Box<dyn Error>>` chain without
boilerplate:

```rust
use std::error::Error;

use big_code_analysis::{analyze, FuncSpace, LANG, MetricsOptions, Source};

pub fn run(
    lang: LANG,
    source: &[u8],
    name: Option<String>,
) -> Result<FuncSpace, Box<dyn Error>> {
    Ok(analyze(
        Source::new(lang, source).with_name(name),
        MetricsOptions::default(),
    )?)
}
```

If you want a project-specific error type, an explicit `From` impl
keeps call sites clean while letting you attach extra context (file
path, language guess, and so on).

## Warnings are not errors

The library writes warnings to **stderr** for non-fatal issues
(malformed `bca:` suppression markers, and a `ConcurrentRunner` input
path that is not a regular file). They do not abort the walk and
they do not flip `Ok` to `Err`. If you run embedded inside a server
or library and need to capture those warnings, redirect stderr at
the process level; the library does not expose a programmatic
warning sink.

[`analyze`]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.analyze.html
[ast_from_path]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Ast.html#method.from_path
[FromPathError]: https://docs.rs/big-code-analysis/*/big_code_analysis/enum.FromPathError.html
[`Source::name`]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Source.html#method.with_name
[`FuncSpace`]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.FuncSpace.html
[`guess_language`]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.guess_language.html
[Node]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Node.html
