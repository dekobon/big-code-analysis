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
    /// The opt-in author-hash key is unusable — empty, or supplied
    /// without `--emit-author-details` (which it has no effect without)
    /// (issue #956).
    InvalidAuthorHashKey(String),
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

/// Emits [`Error::is_client_input`] and [`Error::client_input_samples`]
/// from a single variant list.
///
/// Both halves are generated from the same `client_input` group, so they
/// cannot drift. That coupling is the point. `is_client_input` alone is
/// already compile-forced — its generated match has no wildcard arm, so
/// adding an [`Error`] variant fails to build until it is listed in one
/// group or the other (issue #641). But a front end that turns a
/// client-input variant into its own machine-readable token cannot be
/// compile-forced in the same way: [`Error`] is `#[non_exhaustive]`, so a
/// match in any *other* crate must carry a wildcard and the compiler has
/// nothing to complain about. `InvalidAuthorHashKey` reached the web
/// crate's `vcs_error_kind` wildcard for exactly that reason and was
/// reported to clients as an internal server fault (issue #1245).
///
/// Listing a variant under `client_input` therefore also yields a
/// constructed sample of it, and the web crate's token guard iterates
/// those samples rather than a hand-written copy of the list. Adding a
/// twelfth client-input variant is a compile error here; fixing that
/// compile error is what makes the token guard fail until a token is
/// chosen.
///
/// `client_input` takes `pat_param`, not `pat`, so that one entry cannot
/// quietly stand for two variants. `pat` admits a top-level or-pattern,
/// and `Self::InvalidWindow(_) | Self::InvalidTimestamp(_) => <one
/// sample>` would keep the match exhaustive and correct while yielding a
/// sample for only the first — reopening #1245 for the second. That is
/// the shape a maintainer reaches for, because it is how the arms were
/// written before this macro existed. `pat_param` rejects the `|`
/// outright ("no rules expected `|`"). The `environment` group keeps
/// `pat` because it produces no samples and nothing depends on its
/// entries being one-to-one.
macro_rules! classify_error_variants {
    (
        client_input { $($client:pat_param => $sample:expr),+ $(,)? }
        environment { $($environment:pat),+ $(,)? }
    ) => {
        impl Error {
            /// Whether this error was caused by client-supplied input (a bad
            /// path, revision, window, timestamp, formula, pattern, threshold,
            /// author-hash key, trend parameter, file-type scope, or diff) as
            /// opposed to an environment or backend failure (opening the
            /// repository, walking history, diffing, `.mailmap`, blame, or the
            /// persistent cache).
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
                    $($client => true,)+
                    $($environment => false,)+
                }
            }

            /// One constructed sample per client-input variant, in
            /// declaration order.
            ///
            /// Test support for front ends that must map every
            /// client-input variant onto something of their own — a
            /// machine-readable error token, a help string, an exit code.
            /// `#[non_exhaustive]` denies them an exhaustive match, so
            /// iterating these samples is the only way such a mapping can
            /// be checked for completeness (issue #1245).
            ///
            /// Not part of the stability contract: `#[doc(hidden)]`, and
            /// the payloads are placeholders whose exact text may change
            /// at any time. Assert on the *variant*, never on a sample's
            /// rendered message.
            #[doc(hidden)]
            #[must_use]
            pub fn client_input_samples() -> Vec<Self> {
                vec![$($sample),+]
            }
        }
    };
}

// Adding an entry to `client_input` is a cross-crate obligation, not a
// local one: every client-input variant owes the web surface its own
// `error_kind` token (`vcs_error_kind` in `big-code-analysis-web`), plus
// a line in the vocabulary lists in `STABILITY.md` and the book. Nothing
// here can enforce that — `Error` is `#[non_exhaustive]`, so the web
// match must carry a wildcard — which is why the sample below exists for
// the guard test to iterate. One entry per variant; the samples are
// throwaway payloads, deliberately unlike any real message.
classify_error_variants! {
    client_input {
        Self::NotARepository(_) => Self::NotARepository(PathBuf::from("/not-a-repository")),
        Self::ResolveRef { .. } => Self::ResolveRef {
            reference: "HEAD".to_owned(),
            reason: "unborn branch".to_owned(),
        },
        Self::InvalidBotPattern(_) => Self::InvalidBotPattern("[".to_owned()),
        Self::InvalidWindow(_) => Self::InvalidWindow("banana".to_owned()),
        Self::InvalidTimestamp(_) => Self::InvalidTimestamp("yesterday".to_owned()),
        Self::InvalidFormula(_) => Self::InvalidFormula("astrology".to_owned()),
        Self::InvalidFileTypeScope(_) => Self::InvalidFileTypeScope(String::new()),
        Self::InvalidBusFactorThreshold(_) => Self::InvalidBusFactorThreshold("1.5".to_owned()),
        Self::InvalidAuthorHashKey(_) => Self::InvalidAuthorHashKey("hunter2".to_owned()),
        Self::InvalidTrend(_) => Self::InvalidTrend("1".to_owned()),
        Self::InvalidDiff(_) => Self::InvalidDiff("not a unified diff".to_owned()),
    }
    environment {
        Self::OpenRepository(_),
        Self::Walk(_),
        Self::Diff(_),
        Self::Mailmap(_),
        Self::Blame(_),
        Self::Cache(_),
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // bca: suppress(cyclomatic)
        // Exhaustive one-write-per-variant Display match: cyclomatic here
        // is the variant count, not branching logic, and the arms mirror
        // the compile-enforced `Error` enum one-to-one. Splitting it into
        // sub-matches would be an arbitrary partition with no semantic
        // boundary that would drift out of sync with the enum.
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
            Self::InvalidAuthorHashKey(reason) => {
                write!(f, "invalid author-hash key: {reason}")
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
