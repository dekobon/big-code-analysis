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
//! `classify`, `score`, `hotspot`, `jit`, and `build_history_index`)
//! carries no backend reference, so a future backend (Mercurial,
//! Jujutsu, …; issue #335) reuses it unchanged. Backend-specific code
//! lives under the `git` module behind the `vcs-git` Cargo feature.
//!
//! Two scoring granularities are offered: `build_history_index` ranks
//! *files* at a ref (issue #328), while `score_commit` scores a single
//! *commit* for just-in-time defect-induction risk (issue #331).
//!
//! v1 deliberately omits a `Backend` trait: with a single backend it
//! would be premature abstraction. `build_history_index` delegates to
//! the one available backend; the trait is extracted when a second
//! backend lands.

pub mod bus_factor;
pub mod cache;
pub mod classify;
pub mod entropy;
pub mod error;
pub mod hotspot;
pub mod identity;
pub mod jit;
pub mod options;
pub mod score;
pub mod stats;
pub mod trend;

pub(crate) mod replay;

#[cfg(feature = "vcs-git")]
pub mod git;

pub use bus_factor::{
    BUS_FACTOR_SCHEMA_VERSION, BusFactor, DirectoryBusFactor, GroupBusFactor, VcsAggregate,
};
pub use cache::{CACHE_SCHEMA_VERSION, CacheConfig};
pub use error::Error;
pub use identity::AuthorHashKey;
pub use jit::{
    JIT_SCHEMA_VERSION, JIT_SCORE_VERSION, JitCommit, JitContributions, JitDiffContributions,
    JitDiffReport, JitDiffusion, JitExperience, JitFeatures, JitHistory, JitPurpose, JitReport,
    JitSize, JitSource,
};
pub use options::{FileTypeScope, Options, RiskFormula, parse_window};
pub use stats::Stats;
pub use trend::{TREND_SCHEMA_VERSION, Trend, TrendDelta, TrendDeltas};

/// Per-function change-history attribution (issue #329), surfaced when a
/// front end opts into per-function VCS metrics. See [`PerFunctionBlame`].
#[cfg(feature = "vcs-git")]
pub use git::{BlameSession, LineSpan, PerFunctionBlame};

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The result of one history walk: per-file [`Stats`] keyed by
/// repository-relative path, plus walk-level metadata.
#[derive(Clone, Debug, Default)]
pub struct HistoryIndex {
    files: HashMap<PathBuf, Stats>,
    workdir: Option<PathBuf>,
    truncated_shallow_clone: bool,
    bus_factor: Option<bus_factor::BusFactor>,
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
            bus_factor: None,
        }
    }

    /// Attach the directory- / repo-level bus-factor aggregate (issue
    /// #332). A builder rather than a `new` parameter so the established
    /// constructor signature stays source-compatible for downstream
    /// backends; the aggregate is computed only when a front end opts in
    /// via [`Options::compute_bus_factor`].
    #[must_use]
    pub fn with_bus_factor(mut self, bus_factor: Option<bus_factor::BusFactor>) -> Self {
        self.bus_factor = bus_factor;
        self
    }

    /// The bus-factor aggregate, if it was computed for this walk.
    #[must_use]
    pub fn bus_factor(&self) -> Option<&bus_factor::BusFactor> {
        self.bus_factor.as_ref()
    }

    /// The walk's whole-repo aggregates wrapped in the top-level
    /// [`bus_factor::VcsAggregate`] object the front ends
    /// emit, or `None` when no aggregate was computed. The single
    /// projection shared by the CLI / web / Python surfaces so the
    /// `vcs_aggregate` shape cannot drift between them.
    #[must_use]
    pub fn vcs_aggregate(&self) -> Option<bus_factor::VcsAggregate> {
        self.bus_factor
            .clone()
            .map(|bus_factor| bus_factor::VcsAggregate { bus_factor })
    }

    /// Look up stats by repository-relative path.
    #[must_use]
    pub fn get(&self, repo_relative: &Path) -> Option<&Stats> {
        self.files.get(repo_relative)
    }

    /// Look up stats for an absolute filesystem path by stripping the
    /// working-tree prefix. Returns `None` for paths outside the work
    /// tree or when the repository is bare (no work tree).
    ///
    /// `absolute` must be in the **same canonical form** as the workdir
    /// the index was built with (the `gix`-backed builder stores an
    /// already-canonical workdir, and the CLI canonicalizes its inputs to
    /// match). A non-canonical caller — a symlinked or `..`-laden path that
    /// does not share the stored prefix — silently yields `None` rather
    /// than an error, so a future backend or external caller passing raw
    /// paths must canonicalize first.
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

/// Like [`build_history_index`], but reuse and update the persistent
/// change-history cache per `config` (issue #334).
///
/// On an unchanged tree this replays a cached event log instead of
/// re-walking; when `HEAD` has advanced it walks only the new commits and
/// splices them onto the cached tail. The result is bit-identical to an
/// uncached [`build_history_index`] at the same reference time — the cache
/// is a pure optimization. A missing or corrupt entry is silently
/// recomputed; an entry is ignored when the schema, score, or option
/// fingerprint differs (window changes force a fresh walk). With
/// [`CacheConfig::enabled`] `false` this degrades to a plain walk (still
/// honouring [`CacheConfig::clear`]).
///
/// # Errors
///
/// The same variants as [`build_history_index`], plus [`Error::Cache`]
/// when `--clear-cache` is requested but the cache directory cannot be
/// removed. A failure to *write* a fresh entry is logged, not returned.
#[cfg(feature = "vcs-git")]
pub fn build_history_index_cached(
    root: &Path,
    options: &Options,
    config: &CacheConfig,
) -> Result<HistoryIndex, Error> {
    git::build_cached(root, options, config)
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

/// Score a single commit for just-in-time defect-induction risk
/// (issue #331).
///
/// `spec` is any revision spelling the backend resolves to a commit
/// (`HEAD`, a SHA, a tag, `main~3`, …). The commit is scored against its
/// first parent; the touched files' priors and the author's experience
/// are measured from the history *before* it, windowed by `options`.
/// Returns a [`JitReport`] with the feature breakdown, per-group
/// contributions, and the ordinal composite [`JitReport::risk_score`].
///
/// # Errors
///
/// Returns [`Error::NotARepository`] when `root` is not inside a
/// supported VCS working tree, [`Error::ResolveRef`] when `spec` does not
/// resolve to a commit, or a walk/diff variant when the history walk
/// itself fails.
#[cfg(feature = "vcs-git")]
pub fn score_commit(root: &Path, spec: &str, options: &Options) -> Result<jit::JitReport, Error> {
    git::score_commit(root, spec, options)
}

/// Score an arbitrary unified `diff` for just-in-time defect-induction risk
/// (issue #580).
///
/// Unlike [`score_commit`], a bare diff carries **no author, parent, or
/// file history**, so only the *size* and *diffusion* feature groups are
/// computable. The result is a partial [`jit::JitDiffReport`] whose
/// history / experience / purpose groups are **absent from the type**
/// (not present as zero), and whose
/// [`partial_risk_score`](jit::JitDiffReport::partial_risk_score) is **not
/// comparable** to a commit score — rank diffs against other diffs only.
/// See [`jit::JitDiffReport`] for the full contract.
///
/// `diff` must be a git-style unified diff carrying `diff --git` file
/// headers (as produced by `git diff` / `git format-patch`), with one or
/// more file stanzas. Plain `diff -u` output without those headers and
/// combined / merge diffs (`git diff --cc`, `@@@` headers) are not
/// supported. No repository access is needed; `options` does not
/// participate (a bare diff has nothing to window).
///
/// # Errors
///
/// Returns [`Error::InvalidDiff`] when the diff is structurally malformed
/// (a bad `@@` hunk header, or a `+`/`-` body line outside any hunk) or
/// carries diff content with no `diff --git` file header at all (plain
/// `diff -u` or a combined/merge diff).
#[cfg(feature = "vcs-git")]
pub fn score_diff(diff: &str) -> Result<jit::JitDiffReport, Error> {
    git::score_diff(diff)
}

/// Sample the change-history metrics at `points` evenly-spaced moments
/// across `span_secs`, ending at `options.as_of` (or wall-clock now),
/// building a [`trend::Trend`] time series (issue #333).
///
/// Each point re-anchors at the mainline tip that existed at or before
/// that moment, so the result is a faithful historical snapshot rather
/// than today's tree windowed differently — see [`trend`] for the schema
/// and the cross-snapshot rename limitation. `options` supplies the
/// windows / bot / merge / rename / formula knobs shared by every point;
/// its `reference` selects which mainline to follow.
///
/// # Errors
///
/// Returns [`Error::InvalidTrend`] when `points` is outside
/// `[MIN_TREND_POINTS, MAX_TREND_POINTS]`
/// ([`trend::MIN_TREND_POINTS`] / [`trend::MAX_TREND_POINTS`]),
/// [`Error::NotARepository`] when `root` is not a working tree,
/// [`Error::ResolveRef`] when the base reference does not resolve, or a
/// walk/diff variant when a sampled snapshot fails.
#[cfg(feature = "vcs-git")]
pub fn build_trend(
    root: &Path,
    options: &Options,
    points: usize,
    span_secs: i64,
) -> Result<trend::Trend, Error> {
    git::build_trend(root, options, points, span_secs)
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

/// Discover the working-tree root of the repository containing `path`.
///
/// Returns the canonicalised work-tree directory of the repository that
/// encloses `path` (a file or directory), or `None` when `path` is not
/// inside a repository or the repository is bare (no work tree). Repository
/// discovery walks upward from `path` the same way `git` itself does.
///
/// Front ends use this to coalesce a batch of files onto the repository
/// each belongs to — two files in different subdirectories of the same
/// checkout resolve to the **same** root, so a per-repo [`HistoryIndex`]
/// (or [`PerFunctionBlame`] engine) can be built once and shared across
/// them rather than rebuilt per directory (issue #670).
///
/// ```no_run
/// use std::path::Path;
/// // Two files in different subdirectories of the *same* checkout both
/// // resolve to that checkout's work-tree root: `workdir_root` walks
/// // upward from each file and lands on the same `Some(root)`. (Shown
/// // with absolute paths under a real checkout; `no_run` because no such
/// // repository exists at doctest time.)
/// let a = big_code_analysis::vcs::workdir_root(Path::new("/checkout/src/a.rs"));
/// let b = big_code_analysis::vcs::workdir_root(Path::new("/checkout/tests/b.rs"));
/// // When `/checkout` is a git work tree, both are `Some("/checkout")`, so a
/// // per-repo index can be built once and shared across the batch.
/// if let (Some(root_a), Some(root_b)) = (a, b) {
///     assert_eq!(root_a, root_b);
/// }
/// ```
#[cfg(feature = "vcs-git")]
#[must_use]
pub fn workdir_root(path: &Path) -> Option<PathBuf> {
    git::workdir_root(path)
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
