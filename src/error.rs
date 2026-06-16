// bca: suppress-file(halstead)
// This module is nothing but error-enum definitions and their idiomatic
// `Display`/`Error`/`From` impls. The file-level Halstead effort is the sum
// over many individually-trivial `match` arms (one short string per
// variant); no single function here is hard to follow. Splitting the error
// types across files to dodge the aggregate would scatter the error taxonomy
// for no reader benefit.

//! Error type returned from the library's top-level entry points.
//!
//! Prior to this module, every entry point returned `Option<…>` and
//! collapsed parse failure, empty input, and disabled-language builds
//! into a single `None`. [`MetricsError`] distinguishes those cases so
//! library consumers can react appropriately (e.g. report a disabled
//! language distinctly from an empty parse).
//!
//! New variants may be added in future minor versions, so consumers
//! must include a `_` arm when matching exhaustively — this is enforced
//! by the [`#[non_exhaustive]`][non_exhaustive] attribute on the enum.
//!
//! [non_exhaustive]: https://doc.rust-lang.org/reference/attributes/type_system.html#the-non_exhaustive-attribute

use crate::LANG;

/// Error returned by the library's metric-computation entry points.
///
/// # Stability
///
/// The variant set is additive: new variants may be introduced in
/// minor versions, so the enum is marked `#[non_exhaustive]`. Existing
/// variants will not be removed without a major version bump. The
/// [`std::error::Error`] and [`std::fmt::Display`] impls are part of
/// the stable surface; the exact wording of the `Display` output is
/// not.
///
/// # Examples
///
/// The [`MetricsError::EmptyRoot`] variant is reserved for a future
/// walker change (see its documentation). The
/// [`MetricsError::LanguageDisabled`] variant is the one actually
/// produced today: every dispatch entry point emits it when the
/// caller selects a
/// [`LANG`] whose per-language Cargo feature is not enabled in the
/// current build (see #252). The example exercises the happy path
/// and demonstrates the exhaustive-with-`_` match shape that callers
/// should adopt to stay forward-compatible with future variants.
///
/// ```
/// use big_code_analysis::{analyze, MetricsError, MetricsOptions, Source, LANG};
///
/// let source = Source::new(LANG::Cpp, b"int a = 42;");
/// let result = analyze(source, MetricsOptions::default());
///
/// // Today this call succeeds; the match below documents the shape
/// // callers must adopt so adding a future variant is non-breaking.
/// assert!(result.is_ok());
///
/// match result {
///     Ok(_space) => {}
///     Err(MetricsError::EmptyRoot) => {
///         // Reserved: walker produced no top-level FuncSpace.
///     }
///     Err(MetricsError::LanguageDisabled(_lang)) => {
///         // The `LANG` variant the caller asked for has no grammar
///         // crate compiled in for this build (per-language feature
///         // disabled — see the `[features]` table in Cargo.toml).
///     }
///     // `MetricsError` is `#[non_exhaustive]`; new variants may be added.
///     Err(_) => {}
/// }
/// ```
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricsError {
    /// The walker produced no top-level [`FuncSpace`][crate::FuncSpace].
    ///
    /// Reserved — not produced today. `metrics_with_options` always
    /// pushes a synthetic top-level [`SpaceKind::Unit`][crate::SpaceKind]
    /// `FuncSpace` onto its state stack before walking the AST, so
    /// every parse — including empty input, whitespace-only input,
    /// and comment-only input — currently returns
    /// `Ok(FuncSpace { kind: Unit, .. })`. The variant is kept for a
    /// future walker change that lets the state stack legitimately
    /// drain to empty (e.g. an option that suppresses the synthetic
    /// root for sources with no parseable structure).
    ///
    /// [`FuncSpace`]: crate::FuncSpace
    EmptyRoot,
    /// The requested [`LANG`] is not enabled in this build.
    ///
    /// Produced by every dispatch entry point
    /// ([`crate::analyze`], [`crate::Ast::parse`],
    /// [`crate::Ast::from_tree_sitter`], and
    /// [`crate::LANG::tree_sitter_language`])
    /// when the caller selects a [`LANG`] variant whose per-language
    /// Cargo feature is not enabled in the current build — see the
    /// `[features]` table in the root `Cargo.toml` for the list.
    /// The default feature set (`default = ["all-languages"]`) keeps
    /// every grammar compiled in, matching the library's historical
    /// behaviour; callers that opt into a narrower set with
    /// `--no-default-features --features rust,…` are the only ones
    /// that observe this variant.
    LanguageDisabled(LANG),
}

impl std::fmt::Display for MetricsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyRoot => {
                f.write_str("no top-level FuncSpace could be produced from the source AST")
            }
            Self::LanguageDisabled(lang) => {
                write!(f, "language {} is not enabled in this build", lang.name())
            }
        }
    }
}

impl std::error::Error for MetricsError {}

/// Error returned by [`Ast::from_path`][crate::Ast::from_path].
///
/// `from_path` reads, language-detects, and parses a file in one call, so it
/// can fail in more ways than the in-memory [`Ast::parse`][crate::Ast::parse]
/// (which only reports [`MetricsError`]). Unlike [`analyze`][crate::analyze],
/// `from_path` does not silently skip files: every reason it cannot produce a
/// tree surfaces as a distinct variant so the caller — who asked for *this*
/// file's tree — learns why.
///
/// The enum is `#[non_exhaustive]`; match with a trailing `_` arm to stay
/// forward-compatible.
#[non_exhaustive]
#[derive(Debug)]
pub enum FromPathError {
    /// The file could not be read (a genuine I/O fault: missing file,
    /// permission denied, hardware error). Carries the underlying
    /// [`std::io::Error`].
    Io(std::io::Error),
    /// The path is not valid UTF-8. The path doubles as the resulting
    /// [`FuncSpace`][crate::FuncSpace] name (an identifier used as a map key
    /// and in JSON output), so a lossy conversion is rejected rather than
    /// silently corrupting correlation — mirroring `analyze`'s strict
    /// default.
    NonUtf8Path,
    /// The file is empty, too small, binary, or encoded in an unsupported
    /// encoding (UTF-16, invalid UTF-8) — the same files
    /// [`analyze`][crate::analyze] skips. `from_path` reuses the library's
    /// text reader for byte-exact metric parity with `analyze`, so these
    /// inputs cannot yield a tree.
    Unreadable,
    /// No language is registered for the path (unknown extension and no
    /// recognizable shebang / mode line).
    UnknownLanguage,
    /// The detected language's per-language Cargo feature is not enabled in
    /// this build (carries the [`MetricsError::LanguageDisabled`] raised by
    /// [`Ast::parse`][crate::Ast::parse]).
    Parse(MetricsError),
}

impl std::fmt::Display for FromPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "could not read file: {e}"),
            Self::NonUtf8Path => f.write_str("path is not valid UTF-8"),
            Self::Unreadable => {
                f.write_str("file is empty, binary, or not valid UTF-8 source text")
            }
            Self::UnknownLanguage => f.write_str("no language is registered for this path"),
            Self::Parse(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FromPathError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Parse(e) => Some(e),
            _ => None,
        }
    }
}

impl From<MetricsError> for FromPathError {
    fn from(e: MetricsError) -> Self {
        Self::Parse(e)
    }
}
