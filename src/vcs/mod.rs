// bca: suppress-file(halstead, nargs)
// File-level halstead/nargs are many-fn aggregation artifacts (the
// HistoryIndex accessors + entry points), not per-function logic
// complexity (cognitive/cyclomatic stay enforced).

//! Change-history (VCS) metrics: per-file signals derived from version
//! control history rather than the AST.
//!
//! This is the project's first metric family that is language-agnostic
//! and not AST-derived (issue #328). It surfaces files most likely to
//! harbour bugs or vulnerabilities using the signals the empirical
//! literature most consistently backs — recent churn, commit frequency,
//! author count and ownership dilution, bug- and security-fix history —
//! combined into an ordinal `risk_score`.
//!
//! # Layout
//!
//! The generic surface (`error`, `options`, `stats`, `identity`,
//! `classify`, `score`, `hotspot`, and `build_history_index`) carries
//! no backend reference, so a future backend (Mercurial, Jujutsu, …;
//! issue #335) reuses it unchanged. Backend-specific code lives under
//! the `git` module behind the `vcs-git` Cargo feature.
//!
//! v1 deliberately omits a `Backend` trait: with a single backend it
//! would be premature abstraction. `build_history_index` delegates to
//! the one available backend; the trait is extracted when a second
//! backend lands.

pub mod classify;
pub mod error;
pub mod hotspot;
pub mod identity;
pub mod options;
pub mod score;
pub mod stats;

#[cfg(feature = "vcs-git")]
pub mod git;

pub use error::Error;
pub use options::{Options, RiskFormula, parse_window};
pub use stats::Stats;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The result of one history walk: per-file [`Stats`] keyed by
/// repository-relative path, plus walk-level metadata.
#[derive(Clone, Debug, Default)]
pub struct HistoryIndex {
    files: HashMap<PathBuf, Stats>,
    workdir: Option<PathBuf>,
    truncated_shallow_clone: bool,
}

impl HistoryIndex {
    /// Construct an index from its parts. Used by backends.
    #[must_use]
    pub fn new(
        files: HashMap<PathBuf, Stats>,
        workdir: Option<PathBuf>,
        truncated_shallow_clone: bool,
    ) -> Self {
        Self {
            files,
            workdir,
            truncated_shallow_clone,
        }
    }

    /// Look up stats by repository-relative path.
    #[must_use]
    pub fn get(&self, repo_relative: &Path) -> Option<&Stats> {
        self.files.get(repo_relative)
    }

    /// Look up stats for an absolute filesystem path by stripping the
    /// working-tree prefix. Returns `None` for paths outside the work
    /// tree or when the repository is bare (no work tree).
    #[must_use]
    pub fn get_for_path(&self, absolute: &Path) -> Option<&Stats> {
        let workdir = self.workdir.as_deref()?;
        let relative = absolute.strip_prefix(workdir).ok()?;
        self.files.get(relative)
    }

    /// Iterate `(repo-relative path, stats)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&PathBuf, &Stats)> {
        self.files.iter()
    }

    /// Number of files in the index.
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Whether the index is empty (empty repo, or no tracked text
    /// files at the target ref).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// `true` when the repository is a shallow clone and history was
    /// therefore truncated; the front end surfaces this as a warning.
    #[must_use]
    pub fn truncated_shallow_clone(&self) -> bool {
        self.truncated_shallow_clone
    }

    /// The repository working-tree root, if any (`None` for bare repos).
    #[must_use]
    pub fn workdir(&self) -> Option<&Path> {
        self.workdir.as_deref()
    }

    /// Consume the index, yielding its per-file map.
    #[must_use]
    pub fn into_files(self) -> HashMap<PathBuf, Stats> {
        self.files
    }
}

/// Walk the change history rooted at `root` and build a per-file
/// [`HistoryIndex`].
///
/// Runs **once** per invocation (before any AST walk): walking history
/// per file would be catastrophic on large repositories. The single
/// available backend is selected automatically by probing the working
/// tree.
///
/// # Errors
///
/// Returns [`Error::NotARepository`] when `root` is not inside a
/// supported VCS working tree, or a backend-specific variant when the
/// walk itself fails.
#[cfg(feature = "vcs-git")]
pub fn build_history_index(root: &Path, options: &Options) -> Result<HistoryIndex, Error> {
    git::build(root, options)
}

/// Parse an `--as-of` timestamp into Unix seconds.
///
/// Accepts RFC 3339 / ISO 8601, a bare `@<unix>` epoch, and the git
/// date spellings gix understands. Front ends use this to fill
/// [`Options::as_of`] for reproducible snapshots.
///
/// # Errors
///
/// Returns [`Error::InvalidTimestamp`] when the input is unparseable.
#[cfg(feature = "vcs-git")]
pub fn parse_timestamp(input: &str) -> Result<i64, Error> {
    git::parse_timestamp(input)
}

/// Rank `entries` by descending risk score, breaking ties on the file
/// path (ascending), then truncate to the top `top` (`0` = keep all).
///
/// The single definition of the `bca vcs` / `POST /vcs` / `vcs_metrics`
/// output ordering, shared by all three front ends so the float-compare
/// and tie-break contract cannot drift between them. `key` extracts the
/// `(path, risk_score)` pair from each entry, so callers keep their own
/// per-crate entry types.
pub fn rank_by_risk<T>(entries: &mut Vec<T>, top: usize, key: impl Fn(&T) -> (&str, f64)) {
    entries.sort_by(|a, b| {
        let (path_a, risk_a) = key(a);
        let (path_b, risk_b) = key(b);
        // Descending risk; NaN (never produced today) sorts as equal so
        // the path tie-break still yields a stable, deterministic order.
        risk_b
            .partial_cmp(&risk_a)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| path_a.cmp(path_b))
    });
    if top > 0 && entries.len() > top {
        entries.truncate(top);
    }
}
