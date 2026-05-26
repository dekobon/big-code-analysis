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
    // Build the six (color, intense, text) segments once, then write them
    // in one loop. The original 25-line body called color() / intense_color()
    // and write!()/writeln!() in alternation, scattering 13 `?` exits across
    // the function — over the per-fn nexits cap. Collapsing into a table
    // keeps the rendered byte sequence identical (verified by the
    // `dump_span_ansi_layout_*` tests) while reducing nexits to two `?`
    // sites inside the loop and one trailing `Ok(())`.
    let prefix = if last { "   `- " } else { "   |- " };
    let (label_color, label_intense, label_text) = if span.error {
        (Color::Red, true, "error: ".to_string())
    } else {
        (Color::Magenta, true, format!("{}: ", span.name))
    };
    let start = span.start_line.to_string();
    let end = format!("{}.\n", span.end_line);

    let segments: [(Color, bool, &str); 6] = [
        (Color::Blue, false, prefix),
        (label_color, label_intense, &label_text),
        (Color::Green, false, "from line "),
        (Color::White, false, &start),
        (Color::Green, false, " to line "),
        (Color::White, false, &end),
    ];
    for (col, intense, text) in segments {
        if intense {
            intense_color(stdout, col)?;
        } else {
            color(stdout, col)?;
        }
        stdout.write_all(text.as_bytes())?;
    }
    Ok(())
}

// Trait-object writer so production passes a locked `StandardStream`
// (colored stdout) and tests capture rendered bytes via `termcolor::NoColor`
// over a `Vec<u8>` — matches the dispatch shape of `dump_span` and the
// `color` / `intense_color` helpers in `tools.rs`.
fn dump_spans(
    spans: Vec<FunctionSpan>,
    path: &Path,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    if spans.is_empty() {
        return Ok(());
    }
    intense_color(stdout, Color::Yellow)?;
    writeln!(stdout, "In file {}", path.display())?;
    // Consume `spans` by value: cloning to use `split_last` would
    // allocate each `FunctionSpan`'s `name: String` unnecessarily.
    let last_idx = spans.len() - 1;
    for (i, span) in spans.into_iter().enumerate() {
        dump_span(span, stdout, i == last_idx)?;
    }
    color(stdout, Color::White)
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
        // Skip the stdout lock entirely when the parser produced no
        // function spans (the common case for config / data files in
        // a whole-repo run). `dump_spans` still self-guards for the
        // same case so it can be called from tests that pass an
        // empty Vec directly.
        let spans = function(parser);
        if spans.is_empty() {
            return Ok(());
        }
        let stdout = StandardStream::stdout(ColorChoice::Always);
        let mut stdout = stdout.lock();
        dump_spans(spans, &cfg.path, &mut stdout)
    }
}

#[cfg(test)]
#[path = "function_tests.rs"]
mod tests;
