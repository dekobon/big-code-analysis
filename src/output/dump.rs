// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::enum_glob_use, clippy::ref_option, clippy::wildcard_imports)]

use std::io::Write;

use termcolor::{Color, ColorChoice, StandardStream, StandardStreamLock};

use crate::node::Node;
use crate::tools::{color, intense_color};

use crate::traits::*;

/// Dumps the `AST` of a code.
///
/// Returns a [`Result`] value, when an error occurs.
///
/// # Errors
///
/// Propagates any [`std::io::Error`] produced by the color-aware
/// writer that backs `stdout` (broken pipe, write failure, …).
///
/// # Examples
///
/// ```
/// use big_code_analysis::{dump_node, tree_sitter, LANG, Node};
///
/// let source = b"int a = 42;";
/// let mut parser = tree_sitter::Parser::new();
/// parser
///     .set_language(
///         &LANG::Cpp
///             .get_tree_sitter_language()
///             .expect("cpp feature enabled"),
///     )
///     .expect("cpp grammar pinned to a compatible version");
/// let tree = parser.parse(source, None).expect("parser has a language set");
/// let root = Node(tree.root_node());
///
/// // Dump the AST from the first line of code in a file to the last one
/// dump_node(source, &root, -1, None, None).unwrap();
/// ```
///
/// [`Result`]: #variant.Result
pub fn dump_node(
    code: &[u8],
    node: &Node,
    depth: i32,
    line_start: Option<usize>,
    line_end: Option<usize>,
) -> std::io::Result<()> {
    let stdout = StandardStream::stdout(ColorChoice::Always);
    let mut stdout = stdout.lock();
    let ret = dump_tree_helper(
        code,
        node,
        "",
        true,
        &mut stdout,
        depth,
        &line_start,
        &line_end,
    );

    color(&mut stdout, Color::White)?;

    ret
}

#[allow(clippy::too_many_arguments)]
fn dump_tree_helper(
    code: &[u8],
    node: &Node,
    prefix: &str,
    last: bool,
    stdout: &mut StandardStreamLock,
    depth: i32,
    line_start: &Option<usize>,
    line_end: &Option<usize>,
) -> std::io::Result<()> {
    if depth == 0 {
        return Ok(());
    }

    let (pref_child, pref) = if node.parent().is_none() {
        ("", "")
    } else if last {
        ("   ", "╰─ ")
    } else {
        ("│  ", "├─ ")
    };

    let node_row = node.start_row() + 1;
    let mut display = true;
    if let Some(line_start) = line_start {
        display = node_row >= *line_start;
    }
    if let Some(line_end) = line_end {
        display = display && node_row <= *line_end;
    }

    if display {
        color(stdout, Color::Blue)?;
        write!(stdout, "{prefix}{pref}")?;

        intense_color(stdout, Color::Yellow)?;
        write!(stdout, "{{{}:{}}} ", node.kind(), node.kind_id())?;

        color(stdout, Color::White)?;
        write!(stdout, "from ")?;

        color(stdout, Color::Green)?;
        let (pos_row, pos_column) = node.start_position();
        write!(stdout, "({}, {}) ", pos_row + 1, pos_column + 1)?;

        color(stdout, Color::White)?;
        write!(stdout, "to ")?;

        color(stdout, Color::Green)?;
        let (pos_row, pos_column) = node.end_position();
        write!(stdout, "({}, {}) ", pos_row + 1, pos_column + 1)?;

        if node.start_row() == node.end_row() {
            color(stdout, Color::White)?;
            write!(stdout, ": ")?;

            intense_color(stdout, Color::Red)?;
            let code = &code[node.start_byte()..node.end_byte()];
            if let Ok(code) = str::from_utf8(code) {
                write!(stdout, "{code} ")?;
            } else {
                stdout.write_all(code)?;
            }
        }

        writeln!(stdout)?;
    }

    let count = node.child_count();
    if count != 0 {
        let prefix = format!("{prefix}{pref_child}");
        let mut i = count;
        let mut cursor = node.cursor();
        cursor.goto_first_child();

        loop {
            i -= 1;
            dump_tree_helper(
                code,
                &cursor.node(),
                &prefix,
                i == 0,
                stdout,
                depth - 1,
                line_start,
                line_end,
            )?;
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }

    Ok(())
}

/// Configuration options for dumping the `AST` of a code.
#[derive(Debug)]
pub struct DumpCfg {
    /// The first line of code to dump
    ///
    /// If `None`, the code is dumped from the first line of code
    /// in a file
    pub line_start: Option<usize>,
    /// The last line of code to dump
    ///
    /// If `None`, the code is dumped until the last line of code
    /// in a file
    pub line_end: Option<usize>,
}

/// Type tag identifying the AST-dump action; carries no data.
pub struct Dump {
    _guard: (),
}

impl Callback for Dump {
    type Res = std::io::Result<()>;
    type Cfg = DumpCfg;

    fn call<T: ParserTrait>(cfg: Self::Cfg, parser: &T) -> Self::Res {
        dump_node(
            parser.get_code(),
            &parser.get_root(),
            -1,
            cfg.line_start,
            cfg.line_end,
        )
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]
mod tests {
    use std::path::PathBuf;

    use crate::{CppParser, ParserTrait};

    use super::*;

    #[test]
    fn dump_node_non_utf8_source_does_not_panic() {
        // Regression: `stdout.write_all(code).unwrap()` panicked when the raw-bytes
        // fallback branch was taken for non-UTF-8 source content.
        let code = b"char c = '\xff';";
        let path = PathBuf::from("test.c");
        let parser = CppParser::new(code.to_vec(), &path, None);
        let root = parser.get_root();
        assert!(dump_node(code, &root, -1, None, None).is_ok());
    }
}
