// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::enum_glob_use, clippy::ref_option, clippy::wildcard_imports)]

use termcolor::{Color, WriteColor};

use crate::node::Node;
use crate::output::ColorMode;
use crate::output::color::print_to_stdout;
use crate::tools::{color, intense_color};

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
/// last child (`bca find` dumps a matched node this way), matching the
/// `last = true` the recursive form passed for the root.
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
        // so check it before building a cursor for the child walk.
        if frame.node.child_count() == 0 {
            continue;
        }

        prefix.push_str(pref_child);
        push_children(
            &mut stack,
            frame.node.children(),
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
/// Non-UTF-8 spans fall back to raw bytes — regression guard
/// `dump_node_non_utf8_source_emits_the_raw_snippet`.
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

    use crate::output::test_support::assert_io_error_propagates_at_every_write;
    use crate::{CppParser, ParserTrait};

    use super::*;

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

    #[test]
    fn start_connector_distinguishes_parentless_from_parented() {
        // `start_connector` is the walk's only `Node::parent` call
        // (#1054), so it carries the whole "is this flush-left?"
        // decision. A parentless node renders flush left; a start node
        // that *does* have a parent — how `bca find` dumps a matched
        // node — renders as a last child.
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

    /// [`render_raw`] as text, for the (usual) UTF-8 case.
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
    fn render(code: &[u8], node: &Node, depth: i32) -> String {
        render_range(code, node, depth, None, None)
    }

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

    #[test]
    fn dump_output_from_a_parented_start_node_indents_as_a_last_child() {
        // `bca find` dumps the matched node, not the file root, so the
        // start node usually has a parent and renders `╰─` with its
        // subtree indented under it. `start_connector` is the only place
        // that distinction is made now that the walk carries connectors
        // on the stack (#1054) — this pins it end to end.
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

        // Depth 1 is what `bca find` actually passes: the matched node
        // alone, still as a last child.
        assert_eq!(
            render(code, &paren, 1),
            "╰─ {parenthesized_expression:363} from (1, 9) to (1, 12) : (1) \n"
        );
    }

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

    #[test]
    fn dump_output_depth_limits_recursion() {
        // `bca find` dumps with depth=1 (src/find.rs) to show only the
        // matched node, not its subtree. depth=1 renders the node and stops
        // before its children; depth=0 renders nothing. This is the only
        // positive-depth path in production, and it is what the `depth - 1`
        // decrement in `dump_tree_helper`'s iterative walk guards — pin it
        // explicitly.
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
}
