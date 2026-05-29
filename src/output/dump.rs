// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::enum_glob_use, clippy::ref_option, clippy::wildcard_imports)]

use termcolor::{Color, ColorChoice, StandardStream, WriteColor};

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
    let mut state = DumpState {
        code,
        line_start: &line_start,
        line_end: &line_end,
        stdout: &mut stdout,
    };
    let ret = dump_tree_helper(&mut state, node, "", true, depth);

    color(&mut stdout, Color::White)?;

    ret
}

/// Recursion-invariant rendering state threaded through the AST walk:
/// the source bytes, the optional line-range filter, and the colored
/// writer. Bundling these keeps every walk function under the
/// argument-count limit (the pre-split helper carried eight arguments)
/// and lets tests substitute a `termcolor::NoColor` sink over a
/// `Vec<u8>` for byte-exact output assertions.
struct DumpState<'a> {
    code: &'a [u8],
    line_start: &'a Option<usize>,
    line_end: &'a Option<usize>,
    stdout: &'a mut dyn WriteColor,
}

fn dump_tree_helper(
    state: &mut DumpState,
    node: &Node,
    prefix: &str,
    last: bool,
    depth: i32,
) -> std::io::Result<()> {
    if depth == 0 {
        return Ok(());
    }

    let (pref_child, pref) = branch_glyphs(node, last);

    if line_in_range(node.start_row() + 1, state.line_start, state.line_end) {
        write_node_line(state.stdout, state.code, node, prefix, pref)?;
    }

    dump_children(state, node, prefix, pref_child, depth)
}

/// Box-drawing prefixes for `node` as `(pref_child, pref)`. The root
/// (no parent) renders flush-left regardless of `last`; this check must
/// stay first because `dump_node` passes `last = true` for the root.
fn branch_glyphs(node: &Node, last: bool) -> (&'static str, &'static str) {
    if node.parent().is_none() {
        ("", "")
    } else if last {
        ("   ", "╰─ ")
    } else {
        ("│  ", "├─ ")
    }
}

/// Whether 1-based `row` falls within the optional `[line_start,
/// line_end]` filter. Either bound being `None` leaves that side
/// unconstrained, so `(None, None)` always shows the node.
fn line_in_range(row: usize, line_start: &Option<usize>, line_end: &Option<usize>) -> bool {
    line_start.is_none_or(|start| row >= start) && line_end.is_none_or(|end| row <= end)
}

/// Set `c` then write `args` in that color. Collapsing the recurring
/// set-color-then-write pair into one fallible call keeps each writer
/// helper's exit count under the threshold.
fn paint(stdout: &mut dyn WriteColor, c: Color, args: std::fmt::Arguments) -> std::io::Result<()> {
    color(stdout, c)?;
    stdout.write_fmt(args)
}

/// Emit the full colored description line for one node: header, position
/// range, optional same-row snippet, then the trailing newline (always,
/// even for multi-row nodes whose snippet is skipped).
fn write_node_line(
    stdout: &mut dyn WriteColor,
    code: &[u8],
    node: &Node,
    prefix: &str,
    pref: &str,
) -> std::io::Result<()> {
    write_node_header(stdout, node, prefix, pref)?;
    write_node_location(stdout, node)?;
    write_node_snippet(stdout, code, node)?;
    writeln!(stdout)
}

/// Prefix glyphs followed by the `{kind:kind_id}` tag.
fn write_node_header(
    stdout: &mut dyn WriteColor,
    node: &Node,
    prefix: &str,
    pref: &str,
) -> std::io::Result<()> {
    paint(stdout, Color::Blue, format_args!("{prefix}{pref}"))?;
    intense_color(stdout, Color::Yellow)?;
    write!(stdout, "{{{}:{}}} ", node.kind(), node.kind_id())
}

/// The `from (row, col) to (row, col)` 1-based position range.
fn write_node_location(stdout: &mut dyn WriteColor, node: &Node) -> std::io::Result<()> {
    paint(stdout, Color::White, format_args!("from "))?;
    let (row, column) = node.start_position();
    paint(
        stdout,
        Color::Green,
        format_args!("({}, {}) ", row + 1, column + 1),
    )?;
    paint(stdout, Color::White, format_args!("to "))?;
    let (row, column) = node.end_position();
    paint(
        stdout,
        Color::Green,
        format_args!("({}, {}) ", row + 1, column + 1),
    )
}

/// Source snippet for single-row nodes only. Multi-row nodes return
/// without writing (the caller still emits the trailing newline).
/// Non-UTF-8 spans fall back to raw bytes — regression guard
/// `dump_node_non_utf8_source_does_not_panic`.
fn write_node_snippet(
    stdout: &mut dyn WriteColor,
    code: &[u8],
    node: &Node,
) -> std::io::Result<()> {
    if node.start_row() != node.end_row() {
        return Ok(());
    }

    paint(stdout, Color::White, format_args!(": "))?;
    intense_color(stdout, Color::Red)?;
    let snippet = &code[node.start_byte()..node.end_byte()];
    match str::from_utf8(snippet) {
        Ok(text) => write!(stdout, "{text} "),
        Err(_) => stdout.write_all(snippet),
    }
}

/// Recurse into `node`'s children, extending the prefix and marking the
/// final child so it renders with the closing `╰─` glyph. Leaves
/// allocate no prefix string.
fn dump_children(
    state: &mut DumpState,
    node: &Node,
    prefix: &str,
    pref_child: &str,
    depth: i32,
) -> std::io::Result<()> {
    let count = node.child_count();
    if count == 0 {
        return Ok(());
    }

    let prefix = format!("{prefix}{pref_child}");
    for (i, child) in node.children().enumerate() {
        dump_tree_helper(state, &child, &prefix, i + 1 == count, depth - 1)?;
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

    use termcolor::NoColor;

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

    #[test]
    fn line_in_range_unbounded_always_shows() {
        // Both bounds `None` is the "dump everything" default.
        assert!(line_in_range(5, &None, &None));
        assert!(line_in_range(1, &None, &None));
    }

    #[test]
    fn line_in_range_respects_inclusive_bounds() {
        // Lower bound only.
        assert!(line_in_range(5, &Some(3), &None));
        assert!(!line_in_range(2, &Some(3), &None));
        // Upper bound only.
        assert!(line_in_range(5, &None, &Some(6)));
        assert!(!line_in_range(7, &None, &Some(6)));
        // Both bounds AND-composed.
        assert!(line_in_range(5, &Some(3), &Some(6)));
        assert!(!line_in_range(5, &Some(6), &Some(9))); // below start
        assert!(!line_in_range(5, &Some(1), &Some(4))); // above end
        // Bounds are inclusive on both ends.
        assert!(line_in_range(3, &Some(3), &Some(3)));
    }

    #[test]
    fn branch_glyphs_root_is_flush_left_regardless_of_last() {
        let code = b"int a = 42;\n";
        let parser = CppParser::new(code.to_vec(), &PathBuf::from("t.c"), None);
        let root = parser.get_root();
        // The root has no parent: empty prefixes whatever `last` says.
        assert_eq!(branch_glyphs(&root, true), ("", ""));
        assert_eq!(branch_glyphs(&root, false), ("", ""));

        let child = root
            .children()
            .next()
            .expect("translation_unit has a child");
        assert_eq!(branch_glyphs(&child, true), ("   ", "╰─ "));
        assert_eq!(branch_glyphs(&child, false), ("│  ", "├─ "));
    }

    #[test]
    fn dump_output_matches_expected_tree() {
        // Byte-exact guard that the split preserves the rendered tree.
        // `NoColor` discards color directives, so the captured bytes are
        // the plain text a user sees (the colored CLI output stripped of
        // ANSI). Expected values were captured from the pre-split code.
        let code = b"int a = 42;\n";
        let parser = CppParser::new(code.to_vec(), &PathBuf::from("t.c"), None);
        let root = parser.get_root();

        let no_start: Option<usize> = None;
        let no_end: Option<usize> = None;
        let mut sink = NoColor::new(Vec::new());
        {
            let mut state = DumpState {
                code,
                line_start: &no_start,
                line_end: &no_end,
                stdout: &mut sink,
            };
            dump_tree_helper(&mut state, &root, "", true, -1).expect("dump to in-memory sink");
        }
        let rendered = String::from_utf8(sink.into_inner()).expect("dump output is utf-8");

        let expected = concat!(
            "{translation_unit:308} from (1, 1) to (2, 1) \n",
            "╰─ {declaration:344} from (1, 1) to (1, 12) : int a = 42; \n",
            "   ├─ {primitive_type:96} from (1, 1) to (1, 4) : int \n",
            "   ├─ {init_declarator:383} from (1, 5) to (1, 11) : a = 42 \n",
            "   │  ├─ {identifier:1} from (1, 5) to (1, 6) : a \n",
            "   │  ├─ {=:74} from (1, 7) to (1, 8) : = \n",
            "   │  ╰─ {number_literal:158} from (1, 9) to (1, 11) : 42 \n",
            "   ╰─ {;:42} from (1, 11) to (1, 12) : ; \n",
        );
        assert_eq!(rendered, expected);
    }

    #[test]
    fn dump_output_line_range_filters_rows() {
        // A tight `[2, 2]` range hides every node whose start row is 1,
        // exercising `line_in_range` end to end through the walk.
        let code = b"int a = 1;\nint b = 2;\n";
        let parser = CppParser::new(code.to_vec(), &PathBuf::from("t.c"), None);
        let root = parser.get_root();

        let start: Option<usize> = Some(2);
        let end: Option<usize> = Some(2);
        let mut sink = NoColor::new(Vec::new());
        {
            let mut state = DumpState {
                code,
                line_start: &start,
                line_end: &end,
                stdout: &mut sink,
            };
            dump_tree_helper(&mut state, &root, "", true, -1).expect("dump to in-memory sink");
        }
        let rendered = String::from_utf8(sink.into_inner()).expect("dump output is utf-8");

        // Row-1 nodes (`int a = 1;` and the root, which starts on row 1)
        // are filtered out; only row-2 nodes survive.
        assert!(
            !rendered.contains("(1, "),
            "row-1 nodes should be hidden:\n{rendered}"
        );
        assert!(
            rendered.contains("int b = 2;"),
            "row-2 declaration should show:\n{rendered}"
        );
    }

    #[test]
    fn dump_output_depth_limits_recursion() {
        // `bca find` dumps with depth=1 (src/find.rs) to show only the
        // matched node, not its subtree. depth=1 renders the node and stops
        // before its children; depth=0 renders nothing. This is the only
        // positive-depth path in production, and it is what the `depth - 1`
        // decrement in `dump_children` guards — pin it explicitly.
        let code = b"int a = 42;\n";
        let parser = CppParser::new(code.to_vec(), &PathBuf::from("t.c"), None);
        let root = parser.get_root();
        let no_start: Option<usize> = None;
        let no_end: Option<usize> = None;

        // depth = 1: the root renders, but recursion stops before children.
        let mut sink = NoColor::new(Vec::new());
        {
            let mut state = DumpState {
                code,
                line_start: &no_start,
                line_end: &no_end,
                stdout: &mut sink,
            };
            dump_tree_helper(&mut state, &root, "", true, 1).expect("dump to in-memory sink");
        }
        let rendered = String::from_utf8(sink.into_inner()).expect("dump output is utf-8");
        assert!(
            rendered.contains("{translation_unit:"),
            "depth=1 should render the root:\n{rendered}"
        );
        assert!(
            !rendered.contains("{declaration:"),
            "depth=1 must not recurse into children:\n{rendered}"
        );
        assert_eq!(
            rendered.lines().count(),
            1,
            "depth=1 renders exactly one node:\n{rendered}"
        );

        // depth = 0: nothing renders at all.
        let mut sink_zero = NoColor::new(Vec::new());
        {
            let mut state = DumpState {
                code,
                line_start: &no_start,
                line_end: &no_end,
                stdout: &mut sink_zero,
            };
            dump_tree_helper(&mut state, &root, "", true, 0).expect("dump to in-memory sink");
        }
        assert!(sink_zero.into_inner().is_empty(), "depth=0 renders nothing");
    }
}
