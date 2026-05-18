//! Error type returned from the library's top-level entry points.
//!
//! Prior to this module, every entry point returned `Option<…>` and
//! collapsed parse failure, empty input, non-UTF-8 paths, and
//! disabled-language builds into a single `None`. [`MetricsError`]
//! distinguishes those cases so library consumers can react
//! appropriately (e.g. log the parse failure but skip a non-UTF-8
//! path).
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
/// Most variants are reserved for features that have not yet landed
/// (see each variant's documentation for the issue tracking it). The
/// exception is [`MetricsError::LanguageDisabled`], which is
/// produced by every dispatch entry point when the caller selects a
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
///     Err(MetricsError::ParseHasErrors) => {
///         // Reserved: future strict-parsing toggle on `MetricsOptions`.
///     }
///     Err(MetricsError::LanguageDisabled(_lang)) => {
///         // The `LANG` variant the caller asked for has no grammar
///         // crate compiled in for this build (per-language feature
///         // disabled — see the `[features]` table in Cargo.toml).
///     }
///     Err(MetricsError::NonUtf8Path) => {
///         // Reserved: strict-identifier mode (see issue #254).
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
    /// This is the most common failure mode: the source parsed but
    /// the AST contained no node the per-language [`SpaceKind`][crate::SpaceKind]
    /// resolver could promote into a [`FuncSpace`]. Empty input and
    /// inputs whose only content is comments end up here.
    ///
    /// [`FuncSpace`]: crate::FuncSpace
    EmptyRoot,
    /// The requested [`LANG`] is not enabled in this build.
    ///
    /// Produced by every dispatch entry point
    /// ([`crate::analyze`], [`crate::metrics_from_tree`],
    /// [`crate::action`], [`crate::get_ops`], the deprecated
    /// `get_function_spaces*` shims, and [`crate::LANG::get_tree_sitter_language`])
    /// when the caller selects a [`LANG`] variant whose per-language
    /// Cargo feature is not enabled in the current build — see the
    /// `[features]` table in the root `Cargo.toml` for the list.
    /// The default feature set (`default = ["all-languages"]`) keeps
    /// every grammar compiled in, matching the library's historical
    /// behaviour; callers that opt into a narrower set with
    /// `--no-default-features --features rust,…` are the only ones
    /// that observe this variant.
    LanguageDisabled(LANG),
    /// The supplied path could not be losslessly converted to UTF-8.
    ///
    /// Reserved for callers that opt into strict-identifier mode.
    /// As of #254, the recommended [`crate::analyze`] entry point
    /// accepts a [`crate::Source`] with an explicit
    /// `Source::name: Option<String>` so callers never need to round-
    /// trip a non-UTF-8 path through lossy conversion in the first
    /// place. The deprecated path-positional entry points
    /// (`metrics`, [`crate::get_function_spaces`], …) still
    /// fall back to `Path::to_string_lossy`. This variant is not
    /// produced today; it is kept for future strict-identifier
    /// validators that reject lossy names up front.
    NonUtf8Path,
    /// The tree-sitter parse tree contains syntax errors and the
    /// caller opted into strict mode.
    ///
    /// Reserved for a future strict-parsing toggle on
    /// [`MetricsOptions`][crate::MetricsOptions]; the current entry
    /// points still tolerate `ERROR` nodes and compute best-effort
    /// metrics, so this variant is not produced today.
    ParseHasErrors,
}

impl std::fmt::Display for MetricsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyRoot => {
                f.write_str("no top-level FuncSpace could be produced from the source AST")
            }
            Self::LanguageDisabled(lang) => {
                write!(
                    f,
                    "language {} is not enabled in this build",
                    lang.get_name()
                )
            }
            Self::NonUtf8Path => f.write_str("path is not valid UTF-8"),
            Self::ParseHasErrors => f.write_str("tree-sitter parse tree contains syntax errors"),
        }
    }
}

impl std::error::Error for MetricsError {}
