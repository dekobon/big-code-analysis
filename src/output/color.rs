//! Color-mode selection for the terminal dump serializers.
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

use termcolor::ColorChoice;

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
    /// construct a [`termcolor::StandardStream`].
    pub(crate) fn to_color_choice(self) -> ColorChoice {
        match self {
            Self::Auto => ColorChoice::Auto,
            Self::Always => ColorChoice::Always,
            Self::Never => ColorChoice::Never,
        }
    }
}
