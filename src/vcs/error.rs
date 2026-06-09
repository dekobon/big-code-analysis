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
    /// The bus-factor coverage threshold is outside the open interval
    /// `(0, 1)` (issue #332).
    InvalidBusFactorThreshold(String),
    /// Blaming a file for per-function attribution failed (issue #329).
    Blame(String),
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
            Self::InvalidBusFactorThreshold(reason) => {
                write!(f, "invalid bus-factor threshold: {reason}")
            }
            Self::Blame(reason) => write!(f, "failed to blame file: {reason}"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
#[path = "error_tests.rs"]
mod tests;
