# Error handling

The current entry point [`get_function_spaces`] returns
`Option<FuncSpace>`. This page documents what the `None` arm
actually means and how to turn it into a useful diagnostic.

> **Heads up.** Issue [#253] will replace this return type with
> `Result<FuncSpace, MetricsError>`. Once that lands, you will be
> able to distinguish the error cases below at the type level.
> Until then, you have a single bit.

[#253]: https://github.com/dekobon/big-code-analysis/issues/253

## What `None` means today

```rust
use std::path::PathBuf;

use big_code_analysis::{get_function_spaces, LANG};

fn main() {
    let result = get_function_spaces(
        &LANG::Rust,
        b"this is not rust".to_vec(),
        &PathBuf::from("snippet.rs"),
        None,
    );

    match result {
        Some(space) => println!("ok: {} lines", space.metrics.loc.sloc()),
        None => eprintln!("could not produce a FuncSpace"),
    }
}
```

`None` collapses several distinct conditions into one bit:

1. The tree-sitter grammar refused to produce a root node (very
   rare — even unparseable input usually yields a partial tree).
2. The traversal produced no `Unit` space (defensive — should not
   happen for any supported language but is treated as a
   non-fatal failure rather than a panic).
3. An internal invariant was violated during finalisation.

Of these, (1) is by far the most common cause in practice. The
others tend to indicate a bug in `big-code-analysis` itself; if
you hit them on real-world source, please file an issue.

## Tree-sitter does not always say "no"

Most parse errors do *not* surface as `None`. Tree-sitter is an
**error-recovering** parser — it will produce a tree even for
syntactically broken input, marking the bad regions with
`ERROR` nodes. The metric walk happily computes numbers over the
recovered tree. That means:

- **Garbage in, numbers out.** Feeding C++ source to `LANG::Python`
  generally produces a `Some(FuncSpace)` whose metrics are
  nonsense. Make sure you have selected the right language
  (e.g. via [`guess_language`]) before trusting the result.
- **Partial files score.** A truncated file with an unterminated
  brace will still return `Some(FuncSpace)`. The metrics reflect
  the recovered tree, not the intended source.

If you need to know whether the input parsed cleanly, count
`ERROR` nodes by walking the tree-sitter AST yourself (see the
[`Node`][Node] escape hatch in
[`STABILITY.md`](https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md#escape-hatches)) or use the
[`bca nodes`](../commands/nodes.md) subcommand on the CLI side.

## Suggested wrapper

Until [#253] lands, a thin caller-side wrapper covers most needs:

```rust
use std::path::Path;

use big_code_analysis::{get_function_spaces, FuncSpace, LANG};

#[derive(Debug)]
pub enum AnalyzeError {
    UnsupportedLanguage,
    UnusableTree { path: String },
}

pub fn analyze(
    lang: Option<LANG>,
    source: Vec<u8>,
    path: &Path,
) -> Result<FuncSpace, AnalyzeError> {
    let lang = lang.ok_or(AnalyzeError::UnsupportedLanguage)?;
    get_function_spaces(&lang, source, path, None).ok_or_else(|| {
        AnalyzeError::UnusableTree {
            path: path.display().to_string(),
        }
    })
}
```

This keeps the call site clean and gives you somewhere to attach
better diagnostics (line numbers, suggested language, etc.) as
they become available. When [#253] lands you will swap the
mapping for the real `MetricsError` and delete `UnusableTree`.

## Warnings are not errors

The library writes warnings to **stderr** for non-fatal issues
(malformed `bca:` suppression markers, mainly). They do not
abort the walk and they do not flip `Some` to `None`. If you are
running embedded inside a server or library and need to capture
those warnings, redirect stderr at the process level — the
library does not currently expose a programmatic warning sink.
That is tracked under the library-DX umbrella ([#250]).

[`get_function_spaces`]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.get_function_spaces.html
[`guess_language`]: https://docs.rs/big-code-analysis/*/big_code_analysis/fn.guess_language.html
[Node]: https://docs.rs/big-code-analysis/*/big_code_analysis/struct.Node.html
[#250]: https://github.com/dekobon/big-code-analysis/issues/250
