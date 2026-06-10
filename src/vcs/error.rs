// bca: suppress-file(halstead)
// File-level halstead is a many-arm aggregation artifact (the `Display`
// match grows one arm per variant), not per-function logic complexity
// (cognitive/cyclomatic stay enforced) — mirrors the sibling vcs modules.

//! Error type for the change-history (VCS) metrics pipeline.
//!
//! Generic over the backend: backend-specific failures (a `gix` open
//! error, a rev-walk failure, a blob-diff failure) are mapped to
//! string-carrying variants here rather than leaking the backend's
//! concrete error types into the generic surface. This keeps the
//! generic module tree free of any `gix` reference so a future
//! backend (`vcs-hg`, `vcs-jj`) can reuse it unchanged (issue #335).

use std::path::PathBuf;

/// Error returned by [`build_history_index`](crate::vcs::build_history_index)
/// and the surrounding VCS pipeline.
///
/// `#[non_exhaustive]` so new variants land additively as backends and
/// edge cases accrue; match with a trailing `_` arm to stay
/// forward-compatible.
#[non_exhaustive]
#[derive(Debug)]
pub enum Error {
    /// The supplied path is not inside the working tree of any
    /// supported VCS. Distinct from a repository with no tracked text
    /// files at the target ref, which succeeds with an empty index. (A
    /// freshly-initialised repository whose `HEAD` is unborn instead
    /// surfaces as [`Error::ResolveRef`], since there is no commit to
    /// resolve.)
    NotARepository(PathBuf),
    /// Opening or discovering the repository failed for a reason other
    /// than "no repository here" (corrupt repo, permission denied, …).
    OpenRepository(String),
    /// The `--ref` revision could not be resolved to a commit.
    ResolveRef {
        /// The revision spec the caller supplied (e.g. `HEAD`, a SHA).
        reference: String,
        /// Backend-rendered explanation of why resolution failed.
        reason: String,
    },
    /// Walking commit history failed.
    Walk(String),
    /// Computing a tree-to-tree or blob diff failed.
    Diff(String),
    /// Loading or applying the repository `.mailmap` failed.
    Mailmap(String),
    /// The bot-exclusion pattern is not a valid regular expression.
    InvalidBotPattern(String),
    /// A configured time window could not be parsed.
    InvalidWindow(String),
    /// The `--as-of` timestamp could not be parsed.
    InvalidTimestamp(String),
    /// The risk-formula name is not one of `weighted` / `percentile`.
    InvalidFormula(String),
    /// The file-type scope could not be parsed: an empty value, or a
    /// custom extension list that normalised to nothing (issue #576).
    InvalidFileTypeScope(String),
    /// The bus-factor coverage threshold is outside the open interval
    /// `(0, 1)` (issue #332).
    InvalidBusFactorThreshold(String),
    /// The historical-trend parameters are out of range — the point
    /// count is below the two-point minimum or above
    /// [`MAX_TREND_POINTS`](crate::vcs::trend::MAX_TREND_POINTS) (issue
    /// #333).
    InvalidTrend(String),
    /// Blaming a file for per-function attribution failed (issue #329).
    Blame(String),
    /// An arbitrary unified diff supplied to
    /// [`score_diff`](crate::vcs::score_diff) could not be parsed (issue
    /// #580). A client-input error: the diff was malformed (a hunk header
    /// the parser could not read, a body line outside any hunk, …).
    InvalidDiff(String),
    /// Reading, writing, or clearing the persistent history cache failed
    /// (issue #334). A *missing* or *corrupt* cache entry is not an error —
    /// it is silently ignored and the history is recomputed — so this
    /// variant is reserved for genuine I/O failures the caller asked to
    /// surface (e.g. `--clear-cache` on an unwritable directory).
    Cache(String),
}

impl Error {
    /// Whether this error was caused by client-supplied input (a bad
    /// path, revision, window, timestamp, formula, pattern, threshold,
    /// trend parameter, file-type scope, or diff) as opposed to an
    /// environment or backend failure (opening the repository, walking
    /// history, diffing, `.mailmap`, blame, or the persistent cache).
    ///
    /// Front ends use this to choose a status: a web boundary maps
    /// client-input errors to `400 Bad Request` and the rest to
    /// `500 Internal Server Error` (see `vcs_error_response` in the web
    /// crate).
    ///
    /// The match is intentionally exhaustive (no wildcard arm): adding a
    /// new [`Error`] variant is a compile error here until it is
    /// classified, which prevents the silent fall-through that twice
    /// mis-mapped client-input variants to `500` (`InvalidFileTypeScope`,
    /// `InvalidDiff`; see issue #641).
    #[must_use]
    pub fn is_client_input(&self) -> bool {
        match self {
            Self::NotARepository(_)
            | Self::ResolveRef { .. }
            | Self::InvalidBotPattern(_)
            | Self::InvalidWindow(_)
            | Self::InvalidTimestamp(_)
            | Self::InvalidFormula(_)
            | Self::InvalidFileTypeScope(_)
            | Self::InvalidBusFactorThreshold(_)
            | Self::InvalidTrend(_)
            | Self::InvalidDiff(_) => true,
            Self::OpenRepository(_)
            | Self::Walk(_)
            | Self::Diff(_)
            | Self::Mailmap(_)
            | Self::Blame(_)
            | Self::Cache(_) => false,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotARepository(path) => {
                write!(
                    f,
                    "{} is not inside a supported version-control working tree",
                    path.display()
                )
            }
            Self::OpenRepository(reason) => write!(f, "failed to open repository: {reason}"),
            Self::ResolveRef { reference, reason } => {
                write!(f, "failed to resolve revision {reference:?}: {reason}")
            }
            Self::Walk(reason) => write!(f, "failed to walk commit history: {reason}"),
            Self::Diff(reason) => write!(f, "failed to compute diff: {reason}"),
            Self::Mailmap(reason) => write!(f, "failed to apply .mailmap: {reason}"),
            Self::InvalidBotPattern(reason) => write!(f, "invalid bot pattern: {reason}"),
            Self::InvalidWindow(reason) => write!(f, "invalid time window: {reason}"),
            Self::InvalidTimestamp(reason) => write!(f, "invalid timestamp: {reason}"),
            Self::InvalidFormula(name) => write!(
                f,
                "unknown risk formula {name:?} (expected `weighted` or `percentile`)"
            ),
            Self::InvalidFileTypeScope(reason) => write!(f, "invalid file-type scope: {reason}"),
            Self::InvalidBusFactorThreshold(reason) => {
                write!(f, "invalid bus-factor threshold: {reason}")
            }
            Self::InvalidTrend(reason) => write!(f, "invalid trend parameters: {reason}"),
            Self::Blame(reason) => write!(f, "failed to blame file: {reason}"),
            Self::InvalidDiff(reason) => write!(f, "invalid unified diff: {reason}"),
            Self::Cache(reason) => write!(f, "history cache error: {reason}"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
#[path = "error_tests.rs"]
mod tests;
