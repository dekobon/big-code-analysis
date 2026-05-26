// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(
    clippy::enum_glob_use,
    clippy::needless_pass_by_value,
    clippy::wildcard_imports
)]

use std::path::{Path, PathBuf};

use serde::Serialize;
use termcolor::{Color, ColorChoice, StandardStream, WriteColor};

use crate::traits::*;

use crate::checker::Checker;
use crate::getter::Getter;

use crate::tools::{color, intense_color};

/// Function span data.
#[derive(Debug, Serialize)]
pub struct FunctionSpan {
    /// The function name
    pub name: String,
    /// The first line of a function
    pub start_line: usize,
    /// The last line of a function
    pub end_line: usize,
    /// If `true`, an error is occurred in determining the span
    /// of a function
    pub error: bool,
}

// Hidden from rustdoc because the signature exposes `ParserTrait`,
// which is `#[doc(hidden)]` per issue #256. The CLI's `Function`
// callback remains the documented surface.
#[doc(hidden)]
/// Detects the span of each function in a code.
///
/// Returns a vector containing the [`FunctionSpan`] of each function
///
/// [`FunctionSpan`]: struct.FunctionSpan.html
pub fn function<T: ParserTrait>(parser: &T) -> Vec<FunctionSpan> {
    let root = parser.get_root();
    let code = parser.get_code();
    let mut spans = Vec::new();
    root.act_on_node(&mut |n| {
        if T::Checker::is_func(n) {
            let start_line = n.start_row() + 1;
            let end_line = n.end_row() + 1;
            if let Some(name) = T::Getter::get_func_name(n, code) {
                spans.push(FunctionSpan {
                    name: name.to_string(),
                    start_line,
                    end_line,
                    error: false,
                });
            } else {
                spans.push(FunctionSpan {
                    name: String::new(),
                    start_line,
                    end_line,
                    error: true,
                });
            }
        }
    });

    spans
}

fn dump_span(span: FunctionSpan, stdout: &mut dyn WriteColor, last: bool) -> std::io::Result<()> {
    let pref = if last { "   `- " } else { "   |- " };

    color(stdout, Color::Blue)?;
    write!(stdout, "{pref}")?;

    if span.error {
        intense_color(stdout, Color::Red)?;
        write!(stdout, "error: ")?;
    } else {
        intense_color(stdout, Color::Magenta)?;
        write!(stdout, "{}: ", span.name)?;
    }

    color(stdout, Color::Green)?;
    write!(stdout, "from line ")?;

    color(stdout, Color::White)?;
    write!(stdout, "{}", span.start_line)?;

    color(stdout, Color::Green)?;
    write!(stdout, " to line ")?;

    color(stdout, Color::White)?;
    writeln!(stdout, "{}.", span.end_line)
}

// Generic over `WriteColor` so production passes a locked
// `StandardStream` (colored stdout) and tests can capture the rendered
// bytes via `termcolor::NoColor` over a `Vec<u8>`. The trait-object
// alternative would also work; static dispatch is preferred here
// because there is exactly one production caller and one test caller.
fn dump_spans<W: WriteColor>(
    spans: Vec<FunctionSpan>,
    path: &Path,
    stdout: &mut W,
) -> std::io::Result<()> {
    if !spans.is_empty() {
        intense_color(stdout, Color::Yellow)?;
        writeln!(stdout, "In file {}", path.display())?;

        // Consume `spans` by value: `dump_span` takes `FunctionSpan`
        // by value, so cloning to use `split_last` would allocate
        // strings unnecessarily. The outer `is_empty` guard ensures
        // `spans.len() >= 1`, so `last_idx` is well-defined.
        let last_idx = spans.len() - 1;
        for (i, span) in spans.into_iter().enumerate() {
            dump_span(span, stdout, i == last_idx)?;
        }
        color(stdout, Color::White)?;
    }
    Ok(())
}

/// Configuration options for detecting the span of
/// each function in a code.
#[derive(Debug)]
pub struct FunctionCfg {
    /// Path to the file containing the code
    pub path: PathBuf,
}

/// Type tag identifying the function-extraction action; carries no data.
pub struct Function {
    _guard: (),
}

impl Callback for Function {
    type Res = std::io::Result<()>;
    type Cfg = FunctionCfg;

    fn call<T: ParserTrait>(cfg: Self::Cfg, parser: &T) -> Self::Res {
        let stdout = StandardStream::stdout(ColorChoice::Always);
        let mut stdout = stdout.lock();
        dump_spans(function(parser), &cfg.path, &mut stdout)
    }
}

#[cfg(test)]
#[path = "function_tests.rs"]
mod tests;
