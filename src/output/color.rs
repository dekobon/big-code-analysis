//! Color-mode selection and the shared stdout writer for the terminal
//! dump serializers.
//!
//! The library deliberately performs **no** environment or tty
//! inspection itself: a binary embedding `big-code-analysis` owns the
//! policy decision of whether its stdout is a terminal, whether
//! `NO_COLOR` is set, and whether the user passed a `--color` flag. The
//! caller resolves those signals into a [`ColorMode`] and hands it to
//! the `*_with_color` dump entry points; the library only translates
//! that choice into a concrete [`termcolor::ColorChoice`].
//!
//! Keeping the detection out of the library avoids surprising a
//! downstream embedder whose process is not the `bca` CLI (a GUI, an
//! LSP server, a test harness) with implicit reads of `NO_COLOR` or the
//! ambient `TERM`.

use termcolor::{BufferWriter, ColorChoice, WriteColor};

/// Whether the terminal dump serializers ([`crate::dump_root`],
/// [`crate::dump_ops`], [`crate::dump_node`],
/// [`crate::dump_function_spans`]) emit ANSI color escapes.
///
/// This is the library-facing color policy. The CLI resolves user
/// intent (an explicit `--color` flag, the `NO_COLOR` convention, and
/// stdout tty detection) into one of these variants and threads it into
/// the `*_with_color` dump entry points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMode {
    /// Color only when the underlying stream and environment permit it.
    ///
    /// Maps to [`termcolor::ColorChoice::Auto`], which honors `NO_COLOR`
    /// and `TERM=dumb`. Note that `Auto` does **not** itself check
    /// whether stdout is a terminal — a caller that wants
    /// "no color when piped" must resolve a redirected stream to
    /// [`ColorMode::Never`] before constructing the writer (the CLI
    /// does this via `std::io::IsTerminal`).
    #[default]
    Auto,
    /// Always emit color escapes, regardless of stream or environment.
    Always,
    /// Never emit color escapes.
    Never,
}

impl ColorMode {
    /// Translate the policy into the concrete `termcolor` choice used to
    /// construct the stdout writer.
    pub(crate) fn to_color_choice(self) -> ColorChoice {
        match self {
            Self::Auto => ColorChoice::Auto,
            Self::Always => ColorChoice::Always,
            Self::Never => ColorChoice::Never,
        }
    }
}

/// Render a tree with `render` into an in-memory buffer, then emit it to
/// stdout in one atomic write.
///
/// This is the shared stdout seam for all four terminal dump entry
/// points. It replaces a `StandardStream` lock, which is a `LineWriter`:
/// every `writeln!` in the walk cost its own `write(2)` — a whole-repo
/// `bca metrics` text dump measured 1.5 million of them for 23 MB of
/// output. [`BufferWriter::print`] issues a single `write_all` instead.
///
/// Two properties of the previous shape are preserved deliberately:
///
/// - **Atomicity.** `StandardStream::lock` held the stdout lock for the
///   whole walk, so a parallel walk never interleaved two files' trees.
///   `print` takes the same lock and writes the finished buffer under
///   it, so that still holds — and the lock is now held for the write
///   alone rather than the entire render.
/// - **Error propagation.** Rendering targets memory and so cannot fail
///   on I/O; the buffer is printed before a renderer error is returned,
///   so a partial tree still reaches the terminal exactly as it did when
///   the walk wrote through. The real I/O failure (a broken pipe from
///   `bca metrics | head`, a full disk) now surfaces once, from `print`.
///
/// The trade is that one file's rendering is resident before it is
/// printed, where the write-through form held only a line. The buffer is
/// proportional to the source it describes — the widest dump (`bca
/// dump`, the AST) measured 2.9 MB for a 510 KB file — so the whole-walk
/// cost is bounded by the worker count times the largest input, not by
/// the size of the tree. A whole-repo `bca dump` over pdf.js moved from
/// 49 MB to 80 MB resident while its wall time fell from 0.70 s to
/// 0.24 s.
pub(crate) fn print_to_stdout<F>(color_mode: ColorMode, render: F) -> std::io::Result<()>
where
    F: FnOnce(&mut dyn WriteColor) -> std::io::Result<()>,
{
    let writer = BufferWriter::stdout(color_mode.to_color_choice());
    let mut buffer = writer.buffer();
    let rendered = render(&mut buffer);
    writer.print(&buffer)?;
    rendered
}
