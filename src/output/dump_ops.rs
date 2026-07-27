use termcolor::{Color, StandardStream, WriteColor};

use crate::ops::Ops;
use crate::output::{ColorMode, branch_glyphs};

use crate::tools::{color, intense_color};

/// Dumps all operands and operators of a code.
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
/// use big_code_analysis::{dump_ops, Ast, LANG, Source};
///
/// let source_code = "int a = 42;";
///
/// // Retrieve all operands and operators via the `Ast::ops` seam.
/// let ops = Ast::parse(
///     Source::new(LANG::Cpp, source_code.as_bytes())
///         .with_name(Some("foo.c".to_owned())),
/// )
/// .expect("cpp feature enabled")
/// .ops()
/// .unwrap();
///
/// // Dump all operands and operators
/// dump_ops(&ops).unwrap();
/// ```
pub fn dump_ops(ops: &Ops) -> std::io::Result<()> {
    dump_ops_with_color(ops, ColorMode::Always)
}

/// Like [`dump_ops`], but the caller selects the [`ColorMode`].
///
/// `bca` resolves a `--color` flag, the `NO_COLOR` convention, and
/// stdout tty detection into a mode and passes it here so piped output
/// is escape-free by default. The bare [`dump_ops`] keeps the
/// historical always-colored behavior for backward compatibility.
///
/// # Errors
///
/// Propagates any [`std::io::Error`] produced by the color-aware
/// writer that backs `stdout` (broken pipe, write failure, …).
pub fn dump_ops_with_color(ops: &Ops, color_mode: ColorMode) -> std::io::Result<()> {
    let stdout = StandardStream::stdout(color_mode.to_color_choice());
    let mut stdout = stdout.lock();
    dump_space(ops, &mut stdout)?;
    color(&mut stdout, Color::White)?;

    Ok(())
}

/// One pending space in the walk: the space, the length its indentation
/// prefix has in the shared buffer, and whether it is its parent's last
/// child.
///
/// The prefix is a *length* rather than an owned copy (#1054): prefixes
/// only grow as the walk descends, so the first `prefix_len` bytes stay
/// this space's prefix until it is popped. Owning one prefix per stack
/// entry cost O(depth²) resident bytes on a deep closure nest.
type OpsFrame<'a> = (&'a Ops, usize, bool);

/// Dump the `Ops` space tree with an explicit work stack rather than
/// recursion, so a pathologically deep space nesting (closures within
/// closures) cannot overflow the thread stack at dump time — an
/// uncatchable abort, forbidden by the no-panic rule (#700). Traversal
/// order and per-node glyphs are byte-identical to the prior recursive
/// form.
fn dump_space(space: &Ops, stdout: &mut dyn WriteColor) -> std::io::Result<()> {
    let mut prefix = String::new();
    let mut stack: Vec<OpsFrame> = vec![(space, 0, true)];

    while let Some((space, prefix_len, last)) = stack.pop() {
        // Truncating on every visit — rather than on the way back up —
        // is what lets a frame carry a bare length: whatever a sibling's
        // subtree appended is dropped here. Recorded lengths always sit
        // on a char boundary because only whole glyph runs are appended.
        prefix.truncate(prefix_len);
        let (pref_child, pref) = branch_glyphs(last);

        color(stdout, Color::Blue)?;
        write!(stdout, "{prefix}{pref}")?;

        intense_color(stdout, Color::Yellow)?;
        write!(stdout, "{}: ", space.kind)?;

        intense_color(stdout, Color::Cyan)?;
        write!(stdout, "{}", space.name.as_ref().map_or("", |name| name))?;

        intense_color(stdout, Color::Red)?;
        writeln!(stdout, " (@{})", space.start_line)?;

        prefix.push_str(pref_child);
        let child_prefix_len = prefix.len();
        dump_space_ops(space, &mut prefix, space.spaces.is_empty(), stdout)?;

        // Push children in reverse so `pop()` visits them in source
        // order; the final child carries `last = true` for the closing
        // `` `- `` glyph, matching the recursive `split_last` form.
        let count = space.spaces.len();
        for (i, child) in space.spaces.iter().enumerate().rev() {
            stack.push((child, child_prefix_len, i + 1 == count));
        }
    }

    Ok(())
}

/// Render a space's `operators` / `operands` blocks. `prefix` is the
/// shared indentation buffer; each block extends it in place and the
/// truncation between the two restores the block-level indentation.
fn dump_space_ops(
    ops: &Ops,
    prefix: &mut String,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    let base = prefix.len();
    // `operands` always follows `operators` within a space's op block, so
    // `operators` is never the last child and must render the mid-child
    // connector (`|-`), regardless of whether the block itself is the
    // last child of the space. Passing the block's `last` to both made
    // `operators` draw the closing `` `- `` glyph and mis-indent its
    // operand subtree (#700). Only `operands` inherits the block's
    // `last`.
    dump_ops_values("operators", &ops.operators, prefix, false, stdout)?;
    prefix.truncate(base);
    dump_ops_values("operands", &ops.operands, prefix, last, stdout)
}

/// Render one named op list. Extends `prefix` in place for the list
/// entries and leaves it extended; the caller truncates back (the space
/// walk re-truncates on its next visit anyway).
fn dump_ops_values(
    name: &str,
    ops: &[String],
    prefix: &mut String,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    let (pref_child, pref) = branch_glyphs(last);

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}{pref}")?;

    intense_color(stdout, Color::Green)?;
    writeln!(stdout, "{name}")?;

    let Some((last_op, rest)) = ops.split_last() else {
        return Ok(());
    };

    prefix.push_str(pref_child);
    for op in rest {
        color(stdout, Color::Blue)?;
        write!(stdout, "{prefix}|- ")?;

        color(stdout, Color::White)?;
        writeln!(stdout, "{op}")?;
    }

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}`- ")?;

    color(stdout, Color::White)?;
    writeln!(stdout, "{last_op}")
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
    use super::*;
    use crate::spaces::SpaceKind;
    use termcolor::NoColor;

    fn leaf_ops(operators: Vec<String>, operands: Vec<String>) -> Ops {
        Ops {
            name: Some("unit".to_string()),
            name_was_lossy: false,
            start_line: 1,
            end_line: 1,
            kind: SpaceKind::Unit,
            spaces: vec![],
            operators,
            operands,
        }
    }

    fn render(ops: &Ops) -> String {
        let mut sink = NoColor::new(Vec::new());
        dump_space(ops, &mut sink).expect("dump to in-memory sink");
        String::from_utf8(sink.into_inner()).expect("utf-8 dump")
    }

    #[test]
    fn sibling_space_after_a_nested_one_resumes_its_own_rail() {
        // The walk keeps one shared indentation buffer that is extended
        // on descent and truncated on the next visit (#1054). `after` is
        // a top-level sibling that follows `outer`'s deeper subtree, so a
        // truncation bug leaves it (and its op blocks) indented under
        // `inner`'s rail instead of back at the unit's. Built by hand so
        // the tree shape and the op lists are exactly what the expected
        // rails below spell out, independent of any grammar's parse.
        //
        // The expected rails match what the pre-#1054 binary emits for
        // the equivalent parsed tree (`function outer(){function
        // inner(){}} function after(){}`).
        // `Ops` has an iterative `Drop` (#1056), so struct-update syntax
        // (`..leaf_ops(…)`) cannot move fields out of it; build each node
        // whole.
        let func = |name: &str, line: usize, operators: Vec<String>, spaces: Vec<Ops>| Ops {
            name: Some(name.to_string()),
            name_was_lossy: false,
            start_line: line,
            end_line: line,
            kind: SpaceKind::Function,
            spaces,
            operators,
            operands: vec![],
        };
        let space = Ops {
            name: Some("u".to_string()),
            name_was_lossy: false,
            start_line: 1,
            end_line: 4,
            kind: SpaceKind::Unit,
            spaces: vec![
                func("outer", 1, vec![], vec![func("inner", 2, vec![], vec![])]),
                func("after", 4, vec!["+".to_string()], vec![]),
            ],
            operators: vec![],
            operands: vec![],
        };

        let expected = concat!(
            "`- unit: u (@1)\n",
            "   |- operators\n",
            "   |- operands\n",
            "   |- function: outer (@1)\n",
            "   |  |- operators\n",
            "   |  |- operands\n",
            "   |  `- function: inner (@2)\n",
            "   |     |- operators\n",
            "   |     `- operands\n",
            "   `- function: after (@4)\n",
            "      |- operators\n",
            "      |  `- +\n",
            "      `- operands\n",
        );
        assert_eq!(render(&space), expected);
    }

    #[test]
    fn dump_ops_empty_operators_and_operands_renders_bare_headers() {
        // Regression: `ops.len() - 1` underflowed (usize) when ops was
        // empty, then `ops.last().unwrap()` panicked. A space with no
        // Halstead operators or operands is a realistic input. Asserting
        // the rendered text rather than `dump_ops(..).is_ok()` keeps the
        // no-panic guard while also pinning what an empty block looks
        // like — and keeps the test off the process's real stdout.
        assert_eq!(
            render(&leaf_ops(vec![], vec![])),
            concat!(
                "`- unit: unit (@1)\n",
                "   |- operators\n",
                "   `- operands\n",
            )
        );
    }

    #[test]
    fn operators_render_mid_child_connector_not_last() {
        // `operands` always follows `operators`, so in a leaf space the
        // `operators` line must use the mid-child glyph `|-` and indent
        // its children under `|  `; the closing `` `- `` belongs to
        // `operands`. The pre-fix code passed the block's `last` to both,
        // so `operators` drew `` `- `` and mis-indented its operator
        // subtree under `   ` (#700).
        let ops = leaf_ops(vec!["+".to_string()], vec!["a".to_string()]);
        let out = render(&ops);
        assert!(
            out.contains("|- operators"),
            "operators must use the mid-child connector:\n{out}"
        );
        assert!(
            out.contains("`- operands"),
            "operands must use the last-child connector:\n{out}"
        );
        // The operator leaf indents under `|  ` (operators is not last),
        // not under the `   ` the buggy last-child glyph would produce.
        assert!(
            out.contains("|  `- +"),
            "operator leaf must indent under the mid-child rail:\n{out}"
        );
    }

    #[test]
    fn deeply_nested_spaces_dump_without_stack_overflow() {
        // The space walk is iterative (#700): a deep chain of nested
        // spaces must dump without overflowing the thread stack. Built by
        // hand so the test is grammar-independent; run on a small-stack
        // thread so a recursion regression fails loudly.
        const DEPTH: usize = 8_000;
        let handle = std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(|| {
                let mut root = leaf_ops(vec!["+".to_string()], vec!["a".to_string()]);
                let mut cursor = &mut root;
                for _ in 0..DEPTH {
                    cursor
                        .spaces
                        .push(leaf_ops(vec!["+".to_string()], vec!["a".to_string()]));
                    cursor = cursor.spaces.last_mut().expect("just pushed");
                }
                // Discard the bytes rather than buffering them: every
                // line of a depth-8000 chain carries ~3 x depth bytes of
                // indentation, so a `Vec` sink held ~0.5 GB for a test
                // that only asserts the walk completes.
                let mut sink = NoColor::new(std::io::sink());
                let ok = dump_space(&root, &mut sink).is_ok();
                // `root` drops here without flattening: `Ops`'s `Drop` is
                // iterative as of #1056, so teardown costs no stack depth
                // and cannot mask the dump result.
                ok
            })
            .expect("spawn dump thread");
        assert!(
            handle.join().expect("dump thread must not overflow"),
            "deep space nesting must dump successfully"
        );
    }
}
