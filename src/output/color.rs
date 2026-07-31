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

use std::io::{StdoutLock, Write};

use termcolor::{Buffer, BufferWriter, ColorChoice, ColorSpec, WriteColor};

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

/// Bytes a render may accumulate in memory before the buffer is emitted
/// to stdout and reset.
///
/// The cap exists because the rendered document is not proportional to
/// the source: [`crate::dump_node`]'s walk is `O(nodes × depth)`, so a
/// deeply nested file renders far larger than it reads. A 16 KB C file
/// of 8,000 nested parentheses renders 545 MB, which an unbounded
/// buffer would hold resident all at once.
///
/// 64 KiB is where per-write overhead is already amortized — the same
/// threshold and reasoning as the CLI's own output buffer — so a larger
/// cap buys no syscalls back, only resident bytes. A 545 MB document
/// costs ~8,300 `write(2)` calls at this size, against the ~1.5 million
/// the unbuffered `StandardStream` form issued for 23 MB.
const STDOUT_CHUNK_BYTES: usize = 64 * 1_024;

/// The destination [`print_to_stdout`] renders through.
///
/// Exists so tests can substitute a sink that counts emissions and
/// captures bytes: [`BufferWriter`] can only be constructed over the
/// process's real stdout or stderr, which left the whole buffered path
/// untestable when it shipped.
pub(crate) trait ColorSink {
    /// A fresh buffer carrying this sink's color capability.
    fn new_buffer(&self) -> Buffer;

    /// Write one finished buffer to the destination.
    fn emit(&self, buffer: &Buffer) -> std::io::Result<()>;

    /// Exclude other writers for the span of a chunked document.
    ///
    /// Only the real stdout sink has anything to exclude; the returned
    /// guard is held from the first chunk to the last so a document too
    /// large for one buffer still lands contiguously.
    fn exclusive(&self) -> Option<StdoutLock<'static>> {
        None
    }
}

/// The process's stdout as a [`ColorSink`].
///
/// A newtype rather than an impl on [`BufferWriter`] itself, because
/// [`ColorSink::exclusive`] below hands back the *stdout* lock and that
/// is only the right guard for a writer pointed at stdout.
/// `BufferWriter::stderr` has the identical type, so an impl on the bare
/// type would silently give a stderr writer stdout's lock — excluding
/// the wrong writers and leaving two stderr renders free to interleave.
struct StdoutSink(BufferWriter);

impl ColorSink for StdoutSink {
    fn new_buffer(&self) -> Buffer {
        self.0.buffer()
    }

    fn emit(&self, buffer: &Buffer) -> std::io::Result<()> {
        self.0.print(buffer)
    }

    fn exclusive(&self) -> Option<StdoutLock<'static>> {
        // `Stdout::lock` is reentrant, so the nested lock `print` takes
        // per chunk — and the one `bca dump` already holds around its
        // banner — is fine on this thread while excluding every other.
        Some(std::io::stdout().lock())
    }
}

/// A [`WriteColor`] that accumulates into a [`Buffer`] and hands it to a
/// [`ColorSink`] every [`STDOUT_CHUNK_BYTES`].
struct ChunkedSink<'s, S: ColorSink> {
    sink: &'s S,
    buffer: Buffer,
    /// Taken at the first emission, released when the render ends.
    exclusive: Option<StdoutLock<'static>>,
}

impl<S: ColorSink> ChunkedSink<'_, S> {
    /// Emit whatever has accumulated and reset the buffer.
    ///
    /// The buffer is cleared even when the emission failed, so a
    /// half-written chunk is never offered to the sink twice.
    fn emit_pending(&mut self) -> std::io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        if self.exclusive.is_none() {
            self.exclusive = self.sink.exclusive();
        }
        let emitted = self.sink.emit(&self.buffer);
        self.buffer.clear();
        emitted
    }
}

impl<S: ColorSink> Write for ChunkedSink<'_, S> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let written = self.buffer.write(buf)?;
        if self.buffer.len() >= STDOUT_CHUNK_BYTES {
            self.emit_pending()?;
        }
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.emit_pending()
    }
}

impl<S: ColorSink> WriteColor for ChunkedSink<'_, S> {
    fn supports_color(&self) -> bool {
        self.buffer.supports_color()
    }

    fn set_color(&mut self, spec: &ColorSpec) -> std::io::Result<()> {
        self.buffer.set_color(spec)
    }

    fn reset(&mut self) -> std::io::Result<()> {
        self.buffer.reset()
    }
}

/// Render a tree with `render` through a bounded in-memory buffer,
/// emitting it to stdout in at most [`STDOUT_CHUNK_BYTES`] chunks.
///
/// This is the shared stdout seam for all four terminal dump entry
/// points. It replaces a `StandardStream` lock, which is a `LineWriter`:
/// every `writeln!` in the walk cost its own `write(2)` — a whole-repo
/// `bca metrics` text dump measured 1.5 million of them for 23 MB of
/// output. Buffering issues one `write_all` per chunk instead.
///
/// Two properties of the write-through shape are preserved deliberately:
///
/// - **Atomicity.** `StandardStream::lock` held the stdout lock for the
///   whole walk, so a parallel walk never interleaved two files' trees.
///   A document that fits in one chunk is written under the single lock
///   `print` takes; one that does not holds [`ColorSink::exclusive`]
///   from its first chunk to its last. Either way no other worker's
///   output can land inside a file's. Note the CLI adds its own,
///   stronger, guard on top for `bca dump` / `bca find`: it holds the
///   stdout lock across the per-file banner *and* the tree, which
///   serializes rendering across workers there regardless of document
///   size — matching what the write-through form did.
/// - **Error propagation.** Rendering targets memory and so cannot fail
///   on I/O; the pending buffer is emitted before a renderer error is
///   returned, so a partial tree still reaches the terminal exactly as
///   it did when the walk wrote through. The real I/O failure (a broken
///   pipe from `bca metrics | head`, a full disk) surfaces from the
///   emission.
///
/// # Memory
///
/// Resident output is bounded by [`STDOUT_CHUNK_BYTES`] per worker
/// (plus the largest single `write` call, a line), independent of both
/// the input size and the rendered size. That bound is the point: the
/// rendered document is emphatically *not* proportional to the source it
/// describes. Measured ratios of rendered bytes to source bytes for
/// `bca dump` run 10–22× on ordinary code (`src/metrics/cognitive.rs`,
/// 333 KB → 3.5 MB; a 437 KB C++ translation unit → 9.8 MB) and are
/// unbounded on pathological nesting, where the `O(nodes × depth)` walk
/// turns 16 KB of source into 545 MB of tree.
pub(crate) fn print_to_stdout<F>(color_mode: ColorMode, render: F) -> std::io::Result<()>
where
    F: FnOnce(&mut dyn WriteColor) -> std::io::Result<()>,
{
    render_chunked(
        &StdoutSink(BufferWriter::stdout(color_mode.to_color_choice())),
        render,
    )
}

/// [`print_to_stdout`] with the destination injected — see
/// [`ColorSink`].
fn render_chunked<S, F>(sink: &S, render: F) -> std::io::Result<()>
where
    S: ColorSink,
    F: FnOnce(&mut dyn WriteColor) -> std::io::Result<()>,
{
    let mut chunked = ChunkedSink {
        sink,
        buffer: sink.new_buffer(),
        exclusive: None,
    };
    let rendered = render(&mut chunked);
    chunked.emit_pending()?;
    rendered
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fmt::Write as _;

    use super::*;

    /// Counts emissions and records their sizes, so a test can assert
    /// how much a render held resident rather than only what it wrote.
    #[derive(Default)]
    struct CountingSink {
        chunks: RefCell<Vec<usize>>,
        bytes: RefCell<Vec<u8>>,
    }

    impl ColorSink for CountingSink {
        fn new_buffer(&self) -> Buffer {
            Buffer::no_color()
        }

        fn emit(&self, buffer: &Buffer) -> std::io::Result<()> {
            self.chunks.borrow_mut().push(buffer.len());
            self.bytes.borrow_mut().extend_from_slice(buffer.as_slice());
            Ok(())
        }
    }

    /// A sink whose every emission fails, standing in for a broken pipe
    /// or a full disk.
    struct FailingSink;

    impl ColorSink for FailingSink {
        fn new_buffer(&self) -> Buffer {
            Buffer::no_color()
        }

        fn emit(&self, _buffer: &Buffer) -> std::io::Result<()> {
            Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
        }
    }

    /// The buffering is the point: a document that fits under the cap
    /// costs exactly one write, not one per `writeln!`. Reverting
    /// `render_chunked` to a write-through sink turns the count into
    /// `LINES`.
    #[test]
    fn a_small_render_costs_one_emission() {
        const LINES: usize = 500;
        let sink = CountingSink::default();

        render_chunked(&sink, |out| {
            for i in 0..LINES {
                writeln!(out, "line {i}")?;
            }
            Ok(())
        })
        .expect("the counting sink never fails");

        assert_eq!(sink.chunks.borrow().len(), 1);
        let expected: String = (0..LINES).fold(String::new(), |mut acc, i| {
            let _ = writeln!(acc, "line {i}");
            acc
        });
        assert_eq!(sink.bytes.borrow().as_slice(), expected.as_bytes());
    }

    /// The cap is the memory bound: no chunk may exceed it by more than
    /// the single `write` that tripped it, however large the document
    /// grows. Deleting the `emit_pending` call from
    /// `ChunkedSink::write` makes this one emission of ~4 MB.
    #[test]
    fn a_large_render_stays_bounded_by_the_chunk_size() {
        // Enough to fill the cap many times over without being slow.
        const LINE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde\n";
        const LINES: usize = 64 * 1_024;

        let sink = CountingSink::default();
        render_chunked(&sink, |out| {
            for _ in 0..LINES {
                out.write_all(LINE.as_bytes())?;
            }
            Ok(())
        })
        .expect("the counting sink never fails");

        let chunks = sink.chunks.borrow();
        assert!(chunks.len() > 1, "expected chunking, got {}", chunks.len());
        let largest = chunks.iter().copied().max().unwrap_or_default();
        assert!(
            largest <= STDOUT_CHUNK_BYTES + LINE.len(),
            "chunk of {largest} B exceeds the {STDOUT_CHUNK_BYTES} B cap"
        );
        assert_eq!(sink.bytes.borrow().len(), LINES * LINE.len());
    }

    /// A renderer that gives up partway must still get what it produced
    /// onto the terminal — the write-through form had already printed
    /// those lines by the time it failed.
    #[test]
    fn a_render_error_still_emits_the_partial_buffer() {
        let sink = CountingSink::default();

        let err = render_chunked(&sink, |out| {
            writeln!(out, "rendered before the failure")?;
            Err(std::io::Error::from(std::io::ErrorKind::InvalidData))
        })
        .expect_err("the render error propagates");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(
            sink.bytes.borrow().as_slice(),
            b"rendered before the failure\n"
        );
    }

    /// An emission failure reaches the caller rather than being lost
    /// with the buffer.
    #[test]
    fn an_emission_error_reaches_the_caller() {
        let err = render_chunked(&FailingSink, |out| writeln!(out, "anything"))
            .expect_err("the sink always fails");

        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
    }

    /// Nothing written means nothing emitted — an empty `bca find`
    /// result must not take the stdout lock or push a zero-byte write.
    #[test]
    fn an_empty_render_emits_nothing() {
        let sink = CountingSink::default();
        render_chunked(&sink, |_| Ok(())).expect("no output, no failure");
        assert!(sink.chunks.borrow().is_empty());
    }
}
