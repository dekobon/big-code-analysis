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

use std::io::Write;
use std::path::PathBuf;

use serde::Serialize;
use termcolor::{Color, ColorChoice, StandardStream, StandardStreamLock};

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

fn dump_span(
    span: FunctionSpan,
    stdout: &mut StandardStreamLock,
    last: bool,
) -> std::io::Result<()> {
    /*if !span.error {
        return Ok(());
    }*/

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

fn dump_spans(spans: Vec<FunctionSpan>, path: PathBuf) -> std::io::Result<()> {
    if !spans.is_empty() {
        let stdout = StandardStream::stdout(ColorChoice::Always);
        let mut stdout = stdout.lock();

        intense_color(&mut stdout, Color::Yellow)?;
        writeln!(&mut stdout, "In file {}", path.to_str().unwrap_or("..."))?;

        // Consume `spans` by value: `dump_span` takes `FunctionSpan`
        // by value, so cloning to use `split_last` would allocate
        // strings unnecessarily. The outer `is_empty` guard ensures
        // `spans.len() >= 1`, so `last_idx` is well-defined.
        let last_idx = spans.len() - 1;
        for (i, span) in spans.into_iter().enumerate() {
            dump_span(span, &mut stdout, i == last_idx)?;
        }
        color(&mut stdout, Color::White)?;
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
        dump_spans(function(parser), cfg.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_span(name: &str, start: usize, end: usize) -> FunctionSpan {
        FunctionSpan {
            name: name.to_owned(),
            start_line: start,
            end_line: end,
            error: false,
        }
    }

    // `dump_spans` writes to the real stdout via `StandardStream`, so we
    // cannot capture output here. These tests exercise the index-based
    // dispatch added when the `drain + pop().unwrap()` pair was replaced
    // with `into_iter().enumerate()`: an off-by-one in the new code
    // would still terminate but is most likely to surface as a panic
    // (empty `Vec` -> `len() - 1` underflow) for the n=0 case the outer
    // guard protects.

    #[test]
    fn dump_spans_empty_is_ok() {
        // Reverting the `is_empty` guard would underflow `spans.len() - 1`
        // and panic on subtract-with-overflow in debug builds.
        let result = dump_spans(Vec::new(), PathBuf::from("/tmp/empty.rs"));
        assert!(result.is_ok());
    }

    #[test]
    fn dump_spans_single_span_is_ok() {
        // n=1 is the subtle path: `last_idx = 0`, the loop runs once
        // with `i == last_idx` so the single span is marked as last.
        let result = dump_spans(
            vec![make_span("only", 1, 5)],
            PathBuf::from("/tmp/single.rs"),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn dump_spans_many_spans_is_ok() {
        // n>1 exercises the `i != last_idx` branch (non-final entries)
        // followed by the final `i == last_idx` entry.
        let spans = vec![
            make_span("a", 1, 5),
            make_span("b", 7, 12),
            make_span("c", 14, 20),
        ];
        let result = dump_spans(spans, PathBuf::from("/tmp/many.rs"));
        assert!(result.is_ok());
    }
}
