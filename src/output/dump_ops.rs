// bca: suppress-file(halstead, nargs, nexits)
// Terminal operator-dump serializer; the offenders are mechanical-writer
// aggregation artifacts, not per-function logic complexity.

use termcolor::{Color, StandardStream, WriteColor};

use crate::ops::Ops;
use crate::output::ColorMode;

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
    dump_space(ops, "", true, &mut stdout)?;
    color(&mut stdout, Color::White)?;

    Ok(())
}

/// Dump the `Ops` space tree with an explicit work stack rather than
/// recursion, so a pathologically deep space nesting (closures within
/// closures) cannot overflow the thread stack at dump time — an
/// uncatchable abort, forbidden by the no-panic rule (#700). Traversal
/// order and per-node glyphs are byte-identical to the prior recursive
/// form.
fn dump_space(
    space: &Ops,
    prefix: &str,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    let mut stack: Vec<(&Ops, String, bool)> = vec![(space, prefix.to_owned(), last)];

    while let Some((space, prefix, last)) = stack.pop() {
        let (pref_child, pref) = if last { ("   ", "`- ") } else { ("|  ", "|- ") };

        color(stdout, Color::Blue)?;
        write!(stdout, "{prefix}{pref}")?;

        intense_color(stdout, Color::Yellow)?;
        write!(stdout, "{}: ", space.kind)?;

        intense_color(stdout, Color::Cyan)?;
        write!(stdout, "{}", space.name.as_ref().map_or("", |name| name))?;

        intense_color(stdout, Color::Red)?;
        writeln!(stdout, " (@{})", space.start_line)?;

        let child_prefix = format!("{prefix}{pref_child}");
        dump_space_ops(space, &child_prefix, space.spaces.is_empty(), stdout)?;

        // Push children in reverse so `pop()` visits them in source
        // order; the final child carries `last = true` for the closing
        // `` `- `` glyph, matching the recursive `split_last` form.
        let count = space.spaces.len();
        for (i, child) in space.spaces.iter().enumerate().rev() {
            stack.push((child, child_prefix.clone(), i + 1 == count));
        }
    }

    Ok(())
}

fn dump_space_ops(
    ops: &Ops,
    prefix: &str,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    // `operands` always follows `operators` within a space's op block, so
    // `operators` is never the last child and must render the mid-child
    // connector (`|-`), regardless of whether the block itself is the
    // last child of the space. Passing the block's `last` to both made
    // `operators` draw the closing `` `- `` glyph and mis-indent its
    // operand subtree (#700). Only `operands` inherits the block's
    // `last`.
    dump_ops_values("operators", &ops.operators, prefix, false, stdout)?;
    dump_ops_values("operands", &ops.operands, prefix, last, stdout)
}

fn dump_ops_values(
    name: &str,
    ops: &[String],
    prefix: &str,
    last: bool,
    stdout: &mut dyn WriteColor,
) -> std::io::Result<()> {
    let (pref_child, pref) = if last { ("   ", "`- ") } else { ("|  ", "|- ") };

    color(stdout, Color::Blue)?;
    write!(stdout, "{prefix}{pref}")?;

    intense_color(stdout, Color::Green)?;
    writeln!(stdout, "{name}")?;

    let Some((last_op, rest)) = ops.split_last() else {
        return Ok(());
    };

    let prefix = format!("{prefix}{pref_child}");
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
        dump_space(ops, "", true, &mut sink).expect("dump to in-memory sink");
        String::from_utf8(sink.into_inner()).expect("utf-8 dump")
    }

    #[test]
    fn dump_ops_empty_operators_and_operands_does_not_panic() {
        // Regression: `ops.len() - 1` underflowed (usize) when ops was empty,
        // then `ops.last().unwrap()` panicked. A space with no Halstead
        // operators or operands is a realistic input.
        assert!(dump_ops(&leaf_ops(vec![], vec![])).is_ok());
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
                let mut sink = NoColor::new(Vec::new());
                let ok = dump_space(&root, "", true, &mut sink).is_ok();
                // Flatten the chain before it drops: `Ops`'s derived
                // `Drop` recurses through `spaces`, so a deep tree would
                // overflow the small stack on teardown and mask the dump
                // result. Hoisting each level's children out turns the
                // drop into an iterative one.
                let mut node = root;
                while let Some(child) = node.spaces.pop() {
                    node = child;
                }
                ok
            })
            .expect("spawn dump thread");
        assert!(
            handle.join().expect("dump thread must not overflow"),
            "deep space nesting must dump successfully"
        );
    }
}
