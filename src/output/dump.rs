// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::enum_glob_use, clippy::ref_option, clippy::wildcard_imports)]

use std::ops::{Range, RangeBounds};

use termcolor::{Color, NoColor, WriteColor};

use crate::Ast;
use crate::node::Node;
use crate::output::ColorMode;
use crate::output::color::print_to_stdout;
use crate::output::color::{color, intense_color};

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
/// use big_code_analysis::{dump_node, Ast, LANG, Source};
///
/// let source = b"int a = 42;";
/// let ast = Ast::parse(Source::new(LANG::Cpp, source))
///     .expect("cpp feature enabled");
/// let root = ast.root_node();
///
/// // Dump the AST from the first line of code in a file to the last one
/// dump_node(ast.source(), &root, -1, None, None).unwrap();
/// ```
///
/// # Panics
///
/// Panics if `code` is not the exact source `node` was parsed from.
/// `node`'s byte range is used to slice `code`, so a `node` taken from a
/// different (or smaller) tree indexes out of bounds. Always pair a node
/// with the source it came from — e.g. `ast.source()` and a node obtained
/// from the *same* [`crate::Ast`] (`ast.root_node()` or a descendant).
pub fn dump_node(
    code: &[u8],
    node: &Node,
    depth: i32,
    line_start: Option<usize>,
    line_end: Option<usize>,
) -> std::io::Result<()> {
    dump_node_with_color(code, node, depth, line_start, line_end, ColorMode::Always)
}

/// Like [`dump_node`], but the caller selects the [`ColorMode`].
///
/// `bca` resolves a `--color` flag, the `NO_COLOR` convention, and
/// stdout tty detection into a mode and passes it here so piped output
/// is escape-free by default. The bare [`dump_node`] keeps the
/// historical always-colored behavior for backward compatibility.
///
/// # Errors
///
/// Propagates any [`std::io::Error`] produced by the color-aware
/// writer that backs `stdout` (broken pipe, write failure, …).
///
/// # Panics
///
/// Panics if `code` is not the exact source `node` was parsed from.
/// `node`'s byte range is used to slice `code`, so a `node` taken from a
/// different (or smaller) tree indexes out of bounds. Always pair a node
/// with the source it came from — e.g. `ast.source()` and a node obtained
/// from the *same* [`crate::Ast`] (`ast.root_node()` or a descendant).
pub fn dump_node_with_color(
    code: &[u8],
    node: &Node,
    depth: i32,
    line_start: Option<usize>,
    line_end: Option<usize>,
    color_mode: ColorMode,
) -> std::io::Result<()> {
    // bca: suppress(nargs)
    // `nargs` sums the six published parameters with the rendering
    // closure's writer, which no caller supplies: `function_args_max`
    // is still 6, and the signature is frozen by the stability contract.
    // This trips the soft tier only — at the hard limit of 7 the count
    // of 7 is not `> 7` — so a reader checking against the hard gate
    // will find the marker apparently dead. It is not; deleting it
    // reddens `make self-scan-headroom`.
    print_to_stdout(color_mode, |stdout| {
        let mut state = DumpState {
            code,
            line_start: &line_start,
            line_end: &line_end,
            stdout,
        };
        let ret = dump_tree_helper(&mut state, node, depth);

        color(state.stdout, Color::White)?;

        ret
    })
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

/// One pending node in the iterative AST walk: the node, the length its
/// ancestors' box-drawing prefix has in the shared prefix buffer, the
/// connector glyph it renders with, and the remaining depth budget.
///
/// The prefix is stored as a *length* rather than an owned `String`
/// (#1054). Prefixes only grow by appending as the walk descends, so
/// `prefix_len` is non-decreasing from the bottom of the stack to the
/// top: every frame popped before this one truncates to `prefix_len` or
/// beyond, so the first `prefix_len` bytes of the shared buffer stay
/// exactly this node's prefix until it is popped. One owned prefix per
/// frame made a depth-`d` chain cost O(d²) resident bytes plus an O(d)
/// copy per node.
struct Frame<'a> {
    node: Node<'a>,
    prefix_len: usize,
    connector: Connector,
    depth: i32,
}

/// Which box-drawing connector a node renders with.
///
/// Deriving this when the node is *pushed* keeps [`Node::parent`] — an
/// O(depth) walk in tree-sitter — off the per-node path (#1054): only the
/// node a walk starts from can lack a parent, and every other node is
/// reached as a child of a node already on the stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Connector {
    /// No parent: renders flush left, with no glyphs of its own.
    Flush,
    /// Last child of its parent, so nothing continues below it.
    Last,
    /// Has at least one following sibling, whose line the trailing bar
    /// must reach.
    Inner,
}

impl Connector {
    /// The `(child, own)` prefix pair: what this node contributes to its
    /// children's indentation, and the glyph run on its own line.
    const fn glyphs(self) -> (&'static str, &'static str) {
        match self {
            Self::Flush => ("", ""),
            Self::Last => ("   ", "╰─ "),
            Self::Inner => ("│  ", "├─ "),
        }
    }
}

/// The connector for the node a walk starts from — the only node whose
/// parent has to be looked up. A start node with a parent renders as a
/// last child, matching the `last = true` the recursive form passed for
/// the root. One lookup per dump is `O(depth)`; one per match was what
/// made `bca find` quadratic, so [`render_found`] decides the same
/// question against the tree's root instead (#1416).
fn start_connector(node: &Node) -> Connector {
    if node.parent().is_none() {
        Connector::Flush
    } else {
        Connector::Last
    }
}

/// Render the subtree rooted at `node` with an explicit work stack rather
/// than recursion. A pathologically deep AST (thousands of nested
/// expressions, which the iterative *builder* in tree-sitter accepts
/// without bound) would otherwise overflow the thread stack at dump time
/// — an uncatchable abort, forbidden by the no-panic rule. The traversal
/// order, per-node glyphs, and depth semantics are byte-identical to the
/// prior recursive form (#700).
///
/// The indentation prefix lives in one buffer that is pushed on descent
/// and truncated back on the next visit, so the walk costs O(depth)
/// bytes rather than O(depth²) (#1054). The emitted output stays
/// O(nodes × depth) — every rendered line contains its own indentation,
/// which is inherent to the tree drawing.
fn dump_tree_helper<'a>(state: &mut DumpState, node: &Node<'a>, depth: i32) -> std::io::Result<()> {
    let mut prefix = String::new();
    // One cursor for the whole dump, not one per node: this visits every
    // node in the file, and `Node::children` would build and free a
    // `TreeCursor` at each (#1112, `Node::children_with`).
    let mut cursor = node.cursor();
    let mut stack: Vec<Frame<'a>> = vec![Frame {
        node: *node,
        prefix_len: 0,
        connector: start_connector(node),
        depth,
    }];

    while let Some(frame) = stack.pop() {
        if frame.depth == 0 {
            continue;
        }

        // Truncating on every visit — not on the way back up — is what
        // lets a frame carry a bare length: whatever a sibling's subtree
        // appended is dropped here. Every recorded length came from
        // `String::len` after appending a whole glyph run, so it is
        // always a char boundary.
        prefix.truncate(frame.prefix_len);
        let (pref_child, pref) = frame.connector.glyphs();

        if line_in_range(frame.node.start_row() + 1, state.line_start, state.line_end) {
            write_node_line(state.stdout, state.code, &frame.node, &prefix, pref)?;
        }

        // Leaves are roughly half the nodes and `child_count` is O(1),
        // so check it before reseating the cursor for the child walk.
        if frame.node.child_count() == 0 {
            continue;
        }

        prefix.push_str(pref_child);
        push_children(
            &mut stack,
            frame.node.children_with(&mut cursor),
            prefix.len(),
            frame.depth - 1,
        );
    }

    Ok(())
}

/// Queue `children` so `pop()` visits them in source order, matching the
/// recursive form's pre-order traversal. The frames go on in source order
/// and the new tail is then reversed in place, so the walk needs no
/// separate staging buffer and copies each node once.
///
/// Last-child detection uses the child actually walked last, not
/// `Node::child_count`: [`crate::node::Children`] is cursor-driven and
/// documents that the two can disagree on a malformed tree, in which case
/// counting from `child_count` would leave the real last child rendering
/// as `├─` with a dangling bar below it.
fn push_children<'a>(
    stack: &mut Vec<Frame<'a>>,
    children: impl Iterator<Item = Node<'a>>,
    prefix_len: usize,
    depth: i32,
) {
    let first_pushed = stack.len();
    stack.extend(children.map(|node| Frame {
        node,
        prefix_len,
        connector: Connector::Inner,
        depth,
    }));
    stack[first_pushed..].reverse();
    // After the reversal the source-order-last child sits at the bottom
    // of the new tail, so it is popped last and closes the subtree.
    if let Some(last_child) = stack.get_mut(first_pushed) {
        last_child.connector = Connector::Last;
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
fn write_node_snippet(
    stdout: &mut dyn WriteColor,
    code: &[u8],
    node: &Node,
) -> std::io::Result<()> {
    if node.start_row() != node.end_row() {
        return Ok(());
    }
    write_snippet(
        stdout,
        code,
        std::iter::once(node.start_byte()..node.end_byte()),
    )
}

/// What a `bca find` snippet prints in place of a match nested inside
/// it, whose text the nested match's own line prints (#1416).
const ELIDED: &str = "…";

/// The `: text` tail of a node line, for `pieces` of `code` with
/// [`ELIDED`] between consecutive ones. A dump passes the node's whole
/// span as one piece.
///
/// The trailing space follows the text only when every piece is UTF-8;
/// non-UTF-8 bytes are written raw with none, as they always were —
/// regression guard `dump_node_non_utf8_source_emits_the_raw_snippet`.
/// Each piece is checked on its own, so the cost is the bytes printed
/// rather than the span they sit in.
fn write_snippet(
    stdout: &mut dyn WriteColor,
    code: &[u8],
    pieces: impl IntoIterator<Item = Range<usize>>,
) -> std::io::Result<()> {
    write_snippet_separator(stdout, ": ")?;
    let mut utf8 = true;
    for (nth, piece) in pieces.into_iter().enumerate() {
        if nth > 0 {
            write_snippet_separator(stdout, ELIDED)?;
        }
        let bytes = &code[piece];
        utf8 &= str::from_utf8(bytes).is_ok();
        stdout.write_all(bytes)?;
    }
    if utf8 { stdout.write_all(b" ") } else { Ok(()) }
}

/// The renderer's own punctuation inside a snippet (`: `, [`ELIDED`]),
/// in white, leaving the snippet colour set for the source that follows.
fn write_snippet_separator(stdout: &mut dyn WriteColor, text: &str) -> std::io::Result<()> {
    paint(stdout, Color::White, format_args!("{text}"))?;
    intense_color(stdout, Color::Red)
}

/// Render every node `bca find` matched to stdout, one line per match.
///
/// `found` is what [`Ast::find`] returned for `ast`, in its order, and
/// `lines` filters matches by their 1-based start row, as
/// `--line-start` / `--line-end` do. A line is what [`dump_node`] at
/// depth 1 renders for the match, so a match no other match is nested
/// in prints byte-identically to that, colour included. The two
/// differences are what keep the render linear in the depth the matches
/// nest to (#1416):
///
/// - The connector is decided against `ast`'s root, where `dump_node`
///   asks [`Node::parent`], which climbs from the root and is `O(depth)`.
///   One climb per match was 98% of `bca find`'s time on 4 000 nested
///   matches.
/// - A match nested inside a single-row match prints its text on its
///   own line, so the outer match prints [`ELIDED`] in its place:
///   `f(g(1))` renders as `f(…)` followed by `g(1)`. Printing each
///   match's whole span instead made the output quadratic in the
///   nesting depth: 16 MB for an 8 KB file.
///
/// Hidden because it exists for the CLI, like `format_suggestion`; it
/// is outside the stability contract.
///
/// # Errors
///
/// Propagates any [`std::io::Error`] from the stdout writer.
///
/// # Panics
///
/// Panics if a node in `found` was not parsed from `ast`, for the reason
/// given on [`dump_node`].
#[doc(hidden)]
pub fn dump_found_with_color(
    ast: &Ast,
    found: &[Node],
    lines: impl RangeBounds<usize>,
    color_mode: ColorMode,
) -> std::io::Result<()> {
    print_to_stdout(color_mode, |stdout| render_found(stdout, ast, found, lines))
}

/// [`dump_found_with_color`] into `out`, uncoloured. Exists so the
/// scaling bench can time the renderer the CLI runs; equally hidden.
///
/// # Errors
///
/// Propagates any [`std::io::Error`] from `out`.
///
/// # Panics
///
/// As [`dump_found_with_color`].
#[doc(hidden)]
pub fn write_found(
    out: &mut dyn std::io::Write,
    ast: &Ast,
    found: &[Node],
    lines: impl RangeBounds<usize>,
) -> std::io::Result<()> {
    render_found(&mut NoColor::new(out), ast, found, lines)
}

fn render_found(
    stdout: &mut dyn WriteColor,
    ast: &Ast,
    found: &[Node],
    lines: impl RangeBounds<usize>,
) -> std::io::Result<()> {
    let code = ast.source();
    let root = ast.root_node().id();
    let spans: Vec<Range<usize>> = found
        .iter()
        .map(|node| node.start_byte()..node.end_byte())
        .collect();
    let next_outside = next_outside(&spans);
    for (at, node) in found.iter().enumerate() {
        if lines.contains(&(node.start_row() + 1)) {
            let connector = if node.id() == root {
                Connector::Flush
            } else {
                Connector::Last
            };
            write_node_header(stdout, node, "", connector.glyphs().1)?;
            write_node_location(stdout, node)?;
            if node.start_row() == node.end_row() {
                write_snippet(stdout, code, own_text(&spans, &next_outside, at))?;
            }
            writeln!(stdout)?;
        }
        // `dump_node_with_color` ends each call on white, filtered out
        // or not; keeping it keeps the coloured bytes identical.
        color(stdout, Color::White)?;
    }
    Ok(())
}

/// For each span, the index of the first later span that starts at or
/// after its end, so the matches nested in `spans[at]` are
/// `at + 1..next_outside[at]`.
///
/// One pass over a stack of the spans still open. In [`Ast::find`]'s
/// pre-order the spans nest or follow one another, so the stack's ends
/// shrink towards the top and a span closes the moment a later one
/// starts past it. Spans in any other order get some answer, never a
/// panic; [`own_text`] does not trust it.
fn next_outside(spans: &[Range<usize>]) -> Vec<usize> {
    let mut next = vec![spans.len(); spans.len()];
    let mut open: Vec<usize> = Vec::new();
    for (at, span) in spans.iter().enumerate() {
        while let Some(&top) = open.last()
            && spans[top].end <= span.start
        {
            next[top] = at;
            open.pop();
        }
        open.push(at);
    }
    next
}

/// The parts of `spans[at]` that no match nested in it covers, in
/// source order: one piece more than the matches elided from it.
///
/// Steps from one directly nested match to the next through
/// [`next_outside`], skipping what is nested deeper, so each match is
/// visited once as the child of the match it sits in and the render
/// stays linear. A zero-width match prints nothing and so elides
/// nothing. A span that is not inside what remains of `spans[at]` —
/// impossible in a tree, which nests or orders its nodes — ends the
/// elision, and the rest prints verbatim.
fn own_text<'s>(
    spans: &'s [Range<usize>],
    next_outside: &'s [usize],
    at: usize,
) -> impl Iterator<Item = Range<usize>> + 's {
    let outer = spans[at].clone();
    let mut cursor = outer.start;
    let mut next = at + 1;
    let mut done = false;
    std::iter::from_fn(move || {
        if done {
            return None;
        }
        while let Some(inner) = spans.get(next).filter(|inner| inner.start < outer.end) {
            if inner.start < cursor || inner.end > outer.end {
                break;
            }
            next = next_outside[next];
            if inner.is_empty() {
                continue;
            }
            let piece = cursor..inner.start;
            cursor = inner.end;
            return Some(piece);
        }
        done = true;
        Some(cursor..outer.end)
    })
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
    clippy::too_many_lines,
    // A snippet of one piece is a one-element list of ranges, not a
    // range that wants collecting.
    clippy::single_range_in_vec_init
)]
mod tests {
    use std::path::PathBuf;

    use termcolor::{Ansi, NoColor};

    use crate::output::test_support::assert_io_error_propagates_at_every_write;
    use crate::{CppParser, ParserTrait};

    use super::*;

    #[cfg(feature = "cpp")]
    #[test]
    fn dump_node_non_utf8_source_emits_the_raw_snippet() {
        // Regression: `stdout.write_all(code).unwrap()` panicked when the
        // raw-bytes fallback branch was taken for non-UTF-8 source
        // content. Reaching the assertion at all covers the panic; the
        // assertion itself covers the other half — that the fallback
        // *writes* the bytes. A bare `is_ok()` here passed even with the
        // fallback arm stubbed out to `Ok(())`, silently dropping the
        // snippet it exists to render.
        let code = b"char c = '\xff';";
        let path = PathBuf::from("test.c");
        let parser = CppParser::new(code.to_vec(), &path, None);
        let out = render_raw(code, &parser.root(), -1, None, None);
        assert!(
            out.contains(&0xff),
            "the non-UTF-8 snippet must reach the output: {out:?}"
        );
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
    fn connector_glyphs_are_stable() {
        // The rendered tree is these three pairs and nothing else; the
        // byte-exact walk tests below depend on them verbatim.
        assert_eq!(Connector::Flush.glyphs(), ("", ""));
        assert_eq!(Connector::Last.glyphs(), ("   ", "╰─ "));
        assert_eq!(Connector::Inner.glyphs(), ("│  ", "├─ "));
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn start_connector_distinguishes_parentless_from_parented() {
        // `start_connector` is the walk's only `Node::parent` call
        // (#1054), so it carries the whole "is this flush-left?"
        // decision. A parentless node renders flush left; a start node
        // that *does* have a parent — one `dump_node` is handed from
        // mid-tree — renders as a last child.
        let code = b"int a = 42;\n";
        let parser = CppParser::new(code.to_vec(), &PathBuf::from("t.c"), None);
        let root = parser.root();
        assert_eq!(start_connector(&root), Connector::Flush);

        let child = root
            .children()
            .next()
            .expect("translation_unit has a child");
        assert_eq!(start_connector(&child), Connector::Last);
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn dump_output_matches_expected_tree() {
        // Byte-exact guard that the split preserves the rendered tree.
        // `NoColor` discards color directives, so the captured bytes are
        // the plain text a user sees (the colored CLI output stripped of
        // ANSI). Expected values were captured from the pre-split code.
        let code = b"int a = 42;\n";
        let parser = CppParser::new(code.to_vec(), &PathBuf::from("t.c"), None);

        let expected = concat!(
            "{translation_unit:219} from (1, 1) to (2, 1) \n",
            "╰─ {declaration:255} from (1, 1) to (1, 12) : int a = 42; \n",
            "   ├─ {primitive_type:96} from (1, 1) to (1, 4) : int \n",
            "   ├─ {init_declarator:294} from (1, 5) to (1, 11) : a = 42 \n",
            "   │  ├─ {identifier:1} from (1, 5) to (1, 6) : a \n",
            "   │  ├─ {=:74} from (1, 7) to (1, 8) : = \n",
            "   │  ╰─ {number_literal:158} from (1, 9) to (1, 11) : 42 \n",
            "   ╰─ {;:42} from (1, 11) to (1, 12) : ; \n",
        );
        assert_eq!(render(code, &parser.root(), -1), expected);
    }

    /// Render `node` to an in-memory sink under the given line filter and
    /// return the raw bytes. Not necessarily UTF-8: a non-UTF-8 source
    /// snippet is written through verbatim by `write_node_snippet`.
    #[cfg(feature = "cpp")]
    fn render_raw(
        code: &[u8],
        node: &Node,
        depth: i32,
        line_start: Option<usize>,
        line_end: Option<usize>,
    ) -> Vec<u8> {
        let mut sink = NoColor::new(Vec::new());
        {
            let mut state = DumpState {
                code,
                line_start: &line_start,
                line_end: &line_end,
                stdout: &mut sink,
            };
            dump_tree_helper(&mut state, node, depth).expect("dump to in-memory sink");
        }
        sink.into_inner()
    }

    /// The renderer visits every node in the file, so it must hold one
    /// cursor for the whole dump rather than build one per interior
    /// node (#1112).
    ///
    /// This lives here, not beside the other `child_scan_cursors`
    /// assertions in `node.rs`, because `dump_tree_helper` is private to
    /// this module — which is exactly why the guard was missing until
    /// the counter's accessor was widened to `pub(crate)`. Reverting
    /// `children_with` to `children` here compiled clean and failed
    /// nothing.
    ///
    /// The counter records in `Node::children`, the allocating form, so
    /// a hoisted cursor records **zero** and a per-node one records once
    /// per interior node. The exact zero is the discriminator; a
    /// fraction-of-nodes bound would hold for either on a small fixture.
    #[cfg(feature = "cpp")]
    #[test]
    fn dump_holds_one_cursor_for_the_whole_tree() {
        let parser = CppParser::new(
            b"int f(int a) { if (a) { return a + 1; } return 0; }\n".to_vec(),
            &PathBuf::from("f.cpp"),
            None,
        );
        let root = parser.root();
        let nodes = root.preorder().count();
        assert!(nodes > 20, "fixture is too small to prove much");

        let before = crate::node::child_scan_cursors::observed();
        let rendered = render_range(parser.code(), &root, -1, None, None);
        let scans = crate::node::child_scan_cursors::observed() - before;

        assert!(
            rendered.contains("if_statement"),
            "the fixture must actually render a tree"
        );
        assert_eq!(
            scans, 0,
            "the dump built {scans} cursors over {nodes} nodes; it holds one for \
             the whole render (#1112)"
        );
    }

    /// [`render_raw`] as text, for the (usual) UTF-8 case.
    #[cfg(feature = "cpp")]
    fn render_range(
        code: &[u8],
        node: &Node,
        depth: i32,
        line_start: Option<usize>,
        line_end: Option<usize>,
    ) -> String {
        String::from_utf8(render_raw(code, node, depth, line_start, line_end))
            .expect("dump output is utf-8")
    }

    /// [`render_range`] with the filter disabled — the `bca dump` default.
    #[cfg(feature = "cpp")]
    fn render(code: &[u8], node: &Node, depth: i32) -> String {
        render_range(code, node, depth, None, None)
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn dump_output_restores_prefix_after_nested_subtree() {
        // The walk keeps one shared prefix buffer that is appended to on
        // descent and truncated back on the next visit (#1054), so a
        // sibling that follows a deeper subtree is where a truncation
        // bug shows up. Here `+` and the second `number_literal` follow
        // the three-level `parenthesized_expression` subtree, and the
        // trailing `;` follows the whole `init_declarator` subtree —
        // each must resume its own level's indentation exactly.
        //
        // Expected text was captured from the pre-#1054 binary (`bca
        // dump --color never`) on this same source, so it pins byte
        // identity across the change rather than re-recording whatever
        // the new walk emits.
        let code = b"int a = (1) + 2;\n";
        let parser = CppParser::new(code.to_vec(), &PathBuf::from("t.c"), None);
        let rendered = render(code, &parser.root(), -1);

        let expected = concat!(
            "{translation_unit:219} from (1, 1) to (2, 1) \n",
            "╰─ {declaration:255} from (1, 1) to (1, 17) : int a = (1) + 2; \n",
            "   ├─ {primitive_type:96} from (1, 1) to (1, 4) : int \n",
            "   ├─ {init_declarator:294} from (1, 5) to (1, 16) : a = (1) + 2 \n",
            "   │  ├─ {identifier:1} from (1, 5) to (1, 6) : a \n",
            "   │  ├─ {=:74} from (1, 7) to (1, 8) : = \n",
            "   │  ╰─ {binary_expression:341} from (1, 9) to (1, 16) : (1) + 2 \n",
            "   │     ├─ {parenthesized_expression:363} from (1, 9) to (1, 12) : (1) \n",
            "   │     │  ├─ {(:5} from (1, 9) to (1, 10) : ( \n",
            "   │     │  ├─ {number_literal:158} from (1, 10) to (1, 11) : 1 \n",
            "   │     │  ╰─ {):8} from (1, 11) to (1, 12) : ) \n",
            "   │     ├─ {+:25} from (1, 13) to (1, 14) : + \n",
            "   │     ╰─ {number_literal:158} from (1, 15) to (1, 16) : 2 \n",
            "   ╰─ {;:42} from (1, 16) to (1, 17) : ; \n",
        );
        assert_eq!(rendered, expected);
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn dump_output_from_a_parented_start_node_indents_as_a_last_child() {
        // A start node handed from mid-tree has a parent, so it renders
        // `╰─` with its subtree indented under it. `start_connector` is
        // the only place that distinction is made now that the walk
        // carries connectors on the stack (#1054) — this pins it end to
        // end.
        let code = b"int a = (1) + 2;\n";
        let parser = CppParser::new(code.to_vec(), &PathBuf::from("t.c"), None);
        let root = parser.root();
        let paren = root
            .descendants_by_kind(&["parenthesized_expression"])
            .into_iter()
            .next()
            .expect("source has a parenthesized expression");

        let expected = concat!(
            "╰─ {parenthesized_expression:363} from (1, 9) to (1, 12) : (1) \n",
            "   ├─ {(:5} from (1, 9) to (1, 10) : ( \n",
            "   ├─ {number_literal:158} from (1, 10) to (1, 11) : 1 \n",
            "   ╰─ {):8} from (1, 11) to (1, 12) : ) \n",
        );
        assert_eq!(render(code, &paren, -1), expected);

        // Depth 1 renders the node alone, still as a last child: the
        // line `render_found` gives a match nothing is nested in.
        assert_eq!(
            render(code, &paren, 1),
            "╰─ {parenthesized_expression:363} from (1, 9) to (1, 12) : (1) \n"
        );
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn dump_output_line_range_filters_rows() {
        // A tight `[2, 2]` range hides every node whose start row is 1,
        // exercising `line_in_range` end to end through the walk.
        let code = b"int a = 1;\nint b = 2;\n";
        let parser = CppParser::new(code.to_vec(), &PathBuf::from("t.c"), None);
        let rendered = render_range(code, &parser.root(), -1, Some(2), Some(2));

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

    #[cfg(feature = "cpp")]
    #[test]
    fn deeply_nested_ast_dumps_without_stack_overflow() {
        // The dump walk is iterative (#700): a pathologically deep AST —
        // here ~4000 nested parentheses, which tree-sitter builds with an
        // iterative parser — must dump without overflowing the thread
        // stack. The pre-fix recursive `dump_tree_helper` aborted the
        // process here; an abort is uncatchable, so reaching the
        // assertion at all is the regression guard. Run on a small-stack
        // thread so a latent re-introduction of recursion fails loudly
        // rather than relying on the (large) test-runner stack.
        const DEPTH: usize = 4_000;
        let mut src = Vec::with_capacity(DEPTH * 2 + 8);
        src.extend_from_slice(b"int a = ");
        src.extend(std::iter::repeat_n(b'(', DEPTH));
        src.push(b'1');
        src.extend(std::iter::repeat_n(b')', DEPTH));
        src.extend_from_slice(b";\n");

        let handle = std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(move || {
                let parser = CppParser::new(src.clone(), &PathBuf::from("deep.c"), None);
                let root = parser.root();
                let no_start: Option<usize> = None;
                let no_end: Option<usize> = None;
                // Discard the bytes rather than buffering them: every
                // line of a depth-4000 chain carries ~3 x depth bytes of
                // indentation, so a `Vec` sink held ~140 MB for a test
                // that only asserts the walk completes.
                let mut sink = NoColor::new(std::io::sink());
                let mut state = DumpState {
                    code: &src,
                    line_start: &no_start,
                    line_end: &no_end,
                    stdout: &mut sink,
                };
                dump_tree_helper(&mut state, &root, -1).is_ok()
            })
            .expect("spawn dump thread");
        assert!(
            handle
                .join()
                .expect("dump thread must not overflow the stack"),
            "deep AST must dump successfully"
        );
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn dump_output_depth_limits_recursion() {
        // depth=1 renders the node and stops before its children;
        // depth=0 renders nothing. `bca find` passed depth=1 per match
        // until it moved to `render_found` (#1416), so no production
        // caller passes a positive depth now, but `dump_node` still
        // takes one, and it is what the `depth - 1` decrement in
        // `dump_tree_helper`'s iterative walk guards — pin it explicitly.
        let code = b"int a = 42;\n";
        let parser = CppParser::new(code.to_vec(), &PathBuf::from("t.c"), None);
        let root = parser.root();

        // depth = 1: the root renders, but the walk stops before children.
        let rendered = render(code, &root, 1);
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
        assert!(render(code, &root, 0).is_empty(), "depth=0 renders nothing");
    }

    /// Every write position in the AST walk surfaces an I/O error, and
    /// the walk stops there.
    ///
    /// `dump_node` documents that it propagates any `std::io::Error` the
    /// writer produces — `bca dump | head` closes the pipe mid-stream —
    /// but every existing test writes into an infallible `Vec`, leaving
    /// the failure half of each `?` in `dump_tree_helper`, `paint`,
    /// `write_node_line`, `write_node_header`, `write_node_location`, and
    /// `write_node_snippet` unexercised.
    ///
    /// The fixture is deliberately the smallest tree that still nests:
    /// the sweep re-runs the whole dump once per write position, so cost
    /// is quadratic in the node count.
    #[cfg(feature = "cpp")]
    #[test]
    fn every_write_position_propagates_an_io_error() {
        let code = b"int a = 42;\n";
        let parser = CppParser::new(code.to_vec(), &PathBuf::from("t.c"), None);
        let root = parser.root();
        let (line_start, line_end) = (None, None);

        // 40: the eight nodes of this tree cost several operations each
        // (connector, header, location, snippet). A floor well under the
        // real count catches a fixture that collapsed to a leaf without
        // churning on an exact number.
        assert_io_error_propagates_at_every_write(40, |sink| {
            let mut state = DumpState {
                code,
                line_start: &line_start,
                line_end: &line_end,
                stdout: sink,
            };
            dump_tree_helper(&mut state, &root, -1)
        });
    }

    /// `code` parsed as `lang`.
    #[cfg(any(feature = "cpp", feature = "tcl"))]
    fn parse_as(lang: crate::LANG, code: &[u8]) -> Ast {
        Ast::parse(crate::Source::new(lang, code)).expect("language feature enabled")
    }

    /// What `bca find -t <filters>` matches in `ast`.
    #[cfg(any(feature = "cpp", feature = "tcl"))]
    fn found<'a>(ast: &'a Ast, filters: &[&str]) -> Vec<Node<'a>> {
        let filters: Vec<String> = filters.iter().map(|f| (*f).to_owned()).collect();
        ast.find(&filters).expect("find is infallible")
    }

    /// `bca find -t <filters>` on `ast` as the CLI renders it, uncoloured.
    #[cfg(any(feature = "cpp", feature = "tcl"))]
    fn found_text(ast: &Ast, filters: &[&str]) -> String {
        let mut out = Vec::new();
        write_found(&mut out, ast, &found(ast, filters), ..).expect("render to memory");
        String::from_utf8(out).expect("utf-8 fixture")
    }

    /// What the CLI printed before #1416: one `dump_node_with_color`
    /// call per match at depth 1, each a walk from the match ending on
    /// white. Rebuilt from the same private pieces that call is made of,
    /// into `sink`, as the oracle for the unnested case.
    #[cfg(feature = "cpp")]
    fn per_match_dump(
        sink: &mut dyn WriteColor,
        code: &[u8],
        found: &[Node],
        (line_start, line_end): (Option<usize>, Option<usize>),
    ) {
        for node in found {
            let mut state = DumpState {
                code,
                line_start: &line_start,
                line_end: &line_end,
                stdout: &mut *sink,
            };
            dump_tree_helper(&mut state, node, 1).expect("dump to memory");
            color(state.stdout, Color::White).expect("dump to memory");
        }
    }

    /// A match no other single-row match nests in prints exactly what
    /// the per-match dump printed, colour escapes included: the root
    /// flush left, a multi-row match with matches inside it (it prints
    /// no text, so nothing is elided), leaves, and a non-UTF-8 literal
    /// written raw. Under the line filter too, which the two paths spell
    /// differently. The old path is the specification here, so the test
    /// asks it rather than restating its output.
    #[cfg(feature = "cpp")]
    #[test]
    fn found_render_matches_the_per_match_dump_when_no_match_nests() {
        use std::ops::Bound::{Included, Unbounded};

        let code = b"int a = 1;\nint f(int b) {\n  return g(b, 2);\n}\nchar c = '\xff';\n";
        let ast = parse_as(crate::LANG::Cpp, code);
        let found = found(
            &ast,
            &[
                "translation_unit",
                "compound_statement",
                "identifier",
                "number_literal",
                "char_literal",
            ],
        );
        assert!(found.len() >= 9, "the fixture must match widely: {found:?}");
        assert_eq!(found[0].id(), ast.root_node().id(), "the root is a match");

        for (lines, bounds) in [
            ((None, None), (Unbounded, Unbounded)),
            ((Some(2), Some(3)), (Included(2), Included(3))),
        ] {
            let (mut new, mut old) = (NoColor::new(Vec::new()), NoColor::new(Vec::new()));
            render_found(&mut new, &ast, &found, bounds).expect("render to memory");
            per_match_dump(&mut old, code, &found, lines);
            assert_eq!(new.get_ref(), old.get_ref(), "plain output under {lines:?}");

            let (mut new, mut old) = (Ansi::new(Vec::new()), Ansi::new(Vec::new()));
            render_found(&mut new, &ast, &found, bounds).expect("render to memory");
            per_match_dump(&mut old, code, &found, lines);
            assert!(new.get_ref().contains(&0x1b), "the colour sink colours");
            assert_eq!(
                new.get_ref(),
                old.get_ref(),
                "coloured output under {lines:?}"
            );
        }

        let mut out = Vec::new();
        write_found(&mut out, &ast, &found, ..).expect("render to memory");
        assert!(out.contains(&0xff), "the raw literal renders");
        let text = String::from_utf8_lossy(&out);
        assert!(
            text.starts_with("{translation_unit:"),
            "the root renders flush:\n{text}"
        );
        assert!(
            text.contains("╰─ {identifier:1} from (3, 10) to (3, 11) : g \n"),
            "{text}"
        );
    }

    /// Each match nested in a single-row match prints on its own line,
    /// and the match around it prints `…` in its place — once per
    /// directly nested match, never for one nested deeper, which is
    /// `h(1)` inside `g(…)` here: stepping to `k(2)` without skipping it
    /// would read `h` as a sibling of `g` and stop eliding at it.
    #[cfg(feature = "cpp")]
    #[test]
    fn found_render_elides_each_nested_match_from_the_one_around_it() {
        let code = b"int a = f(g(h(1)), k(2));\n";
        let ast = parse_as(crate::LANG::Cpp, code);
        assert_eq!(found(&ast, &["call_expression"]).len(), 4);
        assert_eq!(
            found_text(&ast, &["call_expression"]),
            concat!(
                "╰─ {call_expression:350} from (1, 9) to (1, 25) : f(…, …) \n",
                "╰─ {call_expression:350} from (1, 11) to (1, 18) : g(…) \n",
                "╰─ {call_expression:350} from (1, 13) to (1, 17) : h(1) \n",
                "╰─ {call_expression:350} from (1, 20) to (1, 24) : k(2) \n",
            )
        );
    }

    /// The shape #1416 measured: one `braced_word` per brace level, so
    /// the per-match dump printed `depth²` bytes. Every level but the
    /// innermost now prints its braces around one `…`.
    #[cfg(feature = "tcl")]
    #[test]
    fn found_render_elides_nested_braced_words() {
        let ast = parse_as(crate::LANG::Tcl, b"x {{{a b}}}\n");
        assert_eq!(
            found_text(&ast, &["braced_word"]),
            concat!(
                "╰─ {braced_word:88} from (1, 3) to (1, 12) : {…} \n",
                "╰─ {braced_word:88} from (1, 4) to (1, 11) : {…} \n",
                "╰─ {braced_word:88} from (1, 5) to (1, 10) : {a b} \n",
            )
        );
    }

    /// The non-UTF-8 rule — raw bytes, no trailing space — is decided on
    /// the bytes a line prints, not on the span it elides: a line whose
    /// own pieces are text keeps its space though a nested match is not,
    /// and the other way round.
    #[cfg(feature = "cpp")]
    #[test]
    fn found_render_decides_utf8_on_the_printed_pieces() {
        let code = b"int a = f('\xff', g(1));\nint b = f(g('\xff'));\n";
        let ast = parse_as(crate::LANG::Cpp, code);
        let found = found(&ast, &["call_expression"]);
        let mut sink = NoColor::new(Vec::new());
        write_found(sink.get_mut(), &ast, &found, ..).expect("render to memory");
        let lines: Vec<&[u8]> = sink.get_ref().split_inclusive(|b| *b == b'\n').collect();
        let tail = |line: &[u8]| {
            let at = line
                .windows(3)
                .position(|w| w == b" : ")
                .expect("a snippet");
            line[at + 3..].to_vec()
        };
        let elided = |before: &[u8], after: &[u8]| [before, ELIDED.as_bytes(), after].concat();
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert_eq!(tail(lines[0]), elided(b"f('\xff', ", b")\n"));
        assert_eq!(tail(lines[1]), b"g(1) \n");
        assert_eq!(tail(lines[2]), elided(b"f(", b") \n"));
        assert_eq!(tail(lines[3]), b"g('\xff')\n");
    }

    /// Rendering every match asks no node for its parent. The per-match
    /// dump asked each one, which climbs from the root: `O(depth)` per
    /// match, 98% of `bca find`'s time on #1416's 4 000-deep input. The
    /// second half shows the counter sees that path, so the zero above
    /// it is not a counter that never moves.
    #[cfg(feature = "cpp")]
    #[test]
    fn found_render_climbs_to_no_parent() {
        let code = b"int a = f(g(h(1)), k(2));\n";
        let ast = parse_as(crate::LANG::Cpp, code);
        let found = found(&ast, &["translation_unit", "call_expression"]);
        assert_eq!(found.len(), 5);
        let climbs = crate::node::node_resolved_parent_lookups::observed;

        let before = climbs();
        write_found(&mut std::io::sink(), &ast, &found, ..).expect("render to a sink");
        assert_eq!(climbs() - before, 0, "the find render climbed to a parent");

        let before = climbs();
        per_match_dump(
            &mut NoColor::new(std::io::sink()),
            code,
            &found,
            (None, None),
        );
        assert_eq!(
            climbs() - before,
            found.len(),
            "the per-match dump asks each match"
        );
    }

    /// Under `-t all` every byte of a one-row file is printed exactly
    /// once: by the deepest match holding it, and by none around it.
    /// The per-match dump printed each byte once per match above it.
    #[cfg(feature = "cpp")]
    #[test]
    fn found_render_prints_each_byte_of_one_row_once() {
        let code = b"int a = f(g(1), 2);";
        let ast = parse_as(crate::LANG::Cpp, code);
        let found = found(&ast, &["all"]);
        assert!(found.iter().all(|node| node.start_row() == node.end_row()));
        let spans: Vec<Range<usize>> = found
            .iter()
            .map(|node| node.start_byte()..node.end_byte())
            .collect();
        let next = next_outside(&spans);
        let printed: usize = (0..spans.len())
            .flat_map(|at| own_text(&spans, &next, at))
            .map(|piece| piece.len())
            .sum();
        assert_eq!(printed, code.len());
        let spanned: usize = spans.iter().map(ExactSizeIterator::len).sum();
        assert!(spanned > 3 * code.len(), "the fixture must nest: {spanned}");
    }

    #[test]
    fn next_outside_points_past_everything_nested() {
        // 0..10 holds 0..4 (holding 1..2) and 5..9 (holding 6..7);
        // 10..12 follows it. Pre-order, as `Ast::find` returns them.
        let spans = [0..10, 0..4, 1..2, 5..9, 6..7, 10..12];
        assert_eq!(next_outside(&spans), [5, 3, 3, 5, 5, 6]);
    }

    #[test]
    fn own_text_elides_only_the_directly_nested_spans() {
        let spans = [0..10, 0..4, 1..2, 5..9, 6..7, 10..12];
        let next = next_outside(&spans);
        let own = |at| own_text(&spans, &next, at).collect::<Vec<_>>();
        assert_eq!(own(0), [0..0, 4..5, 9..10]);
        assert_eq!(own(1), [0..1, 2..4]);
        assert_eq!(own(2), [1..2]);
        assert_eq!(own(3), [5..6, 7..9]);
        assert_eq!(own(5), [10..12]);

        // A child with its parent's whole span leaves the parent `…`; a
        // zero-width child, a MISSING token, elides nothing.
        let spans = [0..5, 0..5, 2..2];
        let next = next_outside(&spans);
        assert_eq!(own_text(&spans, &next, 0).collect::<Vec<_>>(), [0..0, 5..5]);
        assert_eq!(own_text(&spans, &next, 1).collect::<Vec<_>>(), [0..5]);
    }

    /// `next_outside` and `own_text` are total: on spans in any order
    /// and overlap, which no tree produces, the pieces still lie inside
    /// the span in order, so slicing the source by them cannot panic.
    /// Every sequence of up to four spans drawn from a small pool.
    #[test]
    fn own_text_stays_inside_its_span_on_any_input() {
        const POOL: [Range<usize>; 6] = [0..4, 1..3, 2..2, 3..6, 0..6, 5..5];
        let mut checked = 0;
        for len in 1..=4_u32 {
            for code in 0..POOL.len().pow(len) {
                let spans: Vec<Range<usize>> = (0..len)
                    .scan(code, |rest, _| {
                        let pick = *rest % POOL.len();
                        *rest /= POOL.len();
                        Some(POOL[pick].clone())
                    })
                    .collect();
                let next = next_outside(&spans);
                for (at, outer) in spans.iter().enumerate() {
                    let mut cursor = outer.start;
                    for piece in own_text(&spans, &next, at) {
                        assert!(
                            cursor <= piece.start
                                && piece.start <= piece.end
                                && piece.end <= outer.end,
                            "{piece:?} escapes {outer:?} after {cursor} in {spans:?}"
                        );
                        cursor = piece.end;
                    }
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 6 + 2 * 36 + 3 * 216 + 4 * 1_296);
    }
}
