// bca: suppress-file(halstead, nargs, nexits)
// File-level halstead/nargs/exit are many-fn aggregation artifacts (the
// serde event types + the directory/IO helpers, each with early-return
// guards), not per-function logic complexity (cognitive/cyclomatic stay
// enforced).

//! Persistent change-history cache, keyed by `HEAD` SHA and repo identity
//! (issue #334).
//!
//! Re-walking the in-window history on every `bca vcs` invocation is the
//! dominant cost on large repositories, and CI re-runs differ only by the
//! commits pushed since the last run. This module persists the **raw,
//! pre-finalize event log** of a walk so a later run can *replay* it (the
//! `replay` module) instead of re-walking, and *extend* it by walking only
//! the new commits.
//!
//! # Why an event log rather than the finished [`HistoryIndex`](super::HistoryIndex)
//!
//! [`Stats`](super::stats::Stats) is the collapsed, *now-relative* output
//! of [`Accumulator::finalize`](super::stats::Accumulator::finalize):
//! the time windows are already applied and the per-commit detail is
//! gone. It therefore cannot be merged with newer commits, nor
//! re-windowed when wall-clock `now` advances. The cache instead stores
//! one `CommitEvent` per in-window commit — the same data the walk
//! folds — so replay reconstructs the index at the *current* `now`
//! (correct windowing, no staleness) and incremental update is a plain
//! splice of newer events onto cached ones.
//!
//! # Author privacy
//!
//! Authors are stored only as their SHA-256 [`hashed`](super::identity::AuthorId::hashed)
//! digests, never plaintext: the cache must not be a side channel that
//! writes raw author emails to disk. Replay reconstructs identities with
//! [`AuthorId::from_digest`](super::identity::AuthorId::from_digest),
//! which preserves author counts, ownership, and the emitted hashes
//! bit-for-bit (the digest is injective for practical purposes).
//!
//! # Invalidation
//!
//! A cached entry is honoured only when its [`CACHE_SCHEMA_VERSION`],
//! [`VCS_SCHEMA_VERSION`], [`RISK_SCORE_VERSION`], and the
//! `fingerprint` of the walk-affecting options all match the current
//! run; otherwise it is ignored and the history recomputed. Window
//! changes alter the fingerprint, so they force a fresh walk, as the
//! issue specifies. A corrupt or unreadable entry is silently ignored,
//! never fatal.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use super::classify::Classification;
use super::error::Error;
use super::identity::AuthorId;
use super::options::Options;
use super::score::RISK_SCORE_VERSION;
use super::stats::VCS_SCHEMA_VERSION;

/// On-disk format version for the cache. Bump on any change to the
/// `HistoryCache` / `CommitEvent` shape; an older entry is then
/// ignored rather than mis-parsed.
pub const CACHE_SCHEMA_VERSION: u32 = 1;

/// Front-end control over the persistent cache for one build.
///
/// The default both enables caching and uses the platform default
/// directory, so a front end opts *out* (rather than in) — matching the
/// issue's `--no-cache` / `--clear-cache` flags.
#[derive(Clone, Debug)]
// Sealed like `Options`: external front ends construct via
// `CacheConfig::default()` + field assignment so additive knobs stay
// non-breaking (see STABILITY.md).
#[non_exhaustive]
pub struct CacheConfig {
    /// Read from and write to the cache. `false` forces a fresh walk and
    /// skips persistence (`--no-cache`).
    pub enabled: bool,
    /// Remove this repository's cache directory before building
    /// (`--clear-cache`). Honoured even when `enabled` is `false`.
    pub clear: bool,
    /// Cache root directory. `None` selects the platform default (`default_cache_dir`)
    /// (`--cache-dir` overrides it).
    pub dir: Option<PathBuf>,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            clear: false,
            dir: None,
        }
    }
}

/// One commit's raw, pre-finalize contribution to the history, as the
/// walk observed it. Stored newest-first so a replay folds commits in the
/// same order a fresh walk would — keeping the floating-point entropy
/// sums bit-identical.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct CommitEvent {
    /// Commit object id (hex), used to splice an incremental walk onto the
    /// cached tail and to de-duplicate across the splice boundary.
    pub oid: String,
    /// Raw committer time in Unix seconds (unclamped). Replay clamps it to
    /// the *current* `now` so a future-dated commit reads as "today" under
    /// the new reference time, exactly as the live walk does.
    pub time: i64,
    /// Participating author identities as SHA-256 digests — never
    /// plaintext (see the module docs).
    pub authors: Vec<String>,
    /// The commit message matched a bug-fix keyword.
    #[serde(default, skip_serializing_if = "is_false")]
    pub bug_fix: bool,
    /// The commit message matched a security-fix keyword.
    #[serde(default, skip_serializing_if = "is_false")]
    pub security_fix: bool,
    /// The commit is a revert / rollback.
    #[serde(default, skip_serializing_if = "is_false")]
    pub revert: bool,
    /// Rename edges this commit introduced, as `(source, destination)`
    /// repository-relative paths. Replayed newest-first to rebuild the
    /// alias chain that attributes pre-rename edits to the current path —
    /// the one signal that must survive across an incremental boundary
    /// (a rename in a *new* commit re-homes edits in *cached* ones).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub renames: Vec<(PathBuf, PathBuf)>,
    /// Each touched text file's **location path at this commit** (before
    /// alias resolution) and its added+deleted line churn.
    pub touched: Vec<(PathBuf, u64)>,
}

/// Serde skip predicate: a `false` flag is omitted to keep the cache
/// compact (most commits match no keyword class).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
}

impl CommitEvent {
    /// Reconstruct the participating identities from their stored digests.
    #[must_use]
    pub(crate) fn author_ids(&self) -> Vec<AuthorId> {
        self.authors
            .iter()
            .map(|digest| AuthorId::from_digest(digest.clone()))
            .collect()
    }

    /// The commit's keyword classification, rebuilt from the stored flags.
    #[must_use]
    pub(crate) fn classification(&self) -> Classification {
        Classification {
            bug_fix: self.bug_fix,
            security_fix: self.security_fix,
            revert: self.revert,
        }
    }
}

/// A persisted history walk: the event log plus the version stamps and
/// fingerprint that decide whether it may be reused.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct HistoryCache {
    /// On-disk format version ([`CACHE_SCHEMA_VERSION`]).
    pub cache_schema_version: u32,
    /// Output-shape version the events were produced under.
    pub vcs_schema_version: u32,
    /// Composite-formula version the events were produced under.
    pub risk_score_version: u32,
    /// [`fingerprint`] of the walk-affecting options.
    pub options_fingerprint: u64,
    /// The `HEAD` (or `--ref`) object id the walk reached, hex-encoded.
    pub head_sha: String,
    /// The long-window cutoff (`now − long_window`) the events were walked
    /// to. A later run whose own cutoff is *older* than this (wall-clock
    /// time ran backwards) cannot reuse the entry — its window reaches
    /// past the cached tail — and falls back to a fresh walk.
    pub walk_long_boundary: i64,
    /// Whether the walk was truncated by a shallow clone.
    #[serde(default)]
    pub truncated_shallow_clone: bool,
    /// The per-commit event log, newest-first.
    pub events: Vec<CommitEvent>,
}

impl HistoryCache {
    /// Whether this entry may be reused by a run with the given option
    /// fingerprint: the format, schema, score, and option versions must
    /// all match the current build.
    #[must_use]
    pub(crate) fn is_compatible(&self, options_fingerprint: u64) -> bool {
        self.cache_schema_version == CACHE_SCHEMA_VERSION
            && self.vcs_schema_version == VCS_SCHEMA_VERSION
            && self.risk_score_version == RISK_SCORE_VERSION
            && self.options_fingerprint == options_fingerprint
    }
}

/// A stable 64-bit fingerprint of every option that changes *which
/// commits the walk visits or how they are recorded* — the window
/// lengths, traversal mode, merge/rename/bot toggles, the bot pattern,
/// and the `--as-of` reference time.
///
/// Finalization-only knobs (`--risk-formula`, `--emit-author-details`,
/// `--include-deleted`, the bus-factor options) are deliberately excluded:
/// they are applied at replay, so changing one reuses the same event log.
/// The file-type scope (`--file-types`, #576) is excluded for the same
/// reason — the cached event log spans every touched file regardless of
/// scope, and the scope is re-applied to the freshly-enumerated seed and
/// at replay, so an entry stays reusable across scopes. The revision
/// spelling is excluded too — the resolved [`head_sha`] keys the entry,
/// so two refs naming the same commit share it.
///
/// [`head_sha`]: HistoryCache::head_sha
///
/// `DefaultHasher` is created with fixed keys, so the digest is stable
/// across processes built with the same toolchain (its `SipHasher13`
/// algorithm is not guaranteed stable across Rust releases — a bump
/// shifts every fingerprint, which costs a benign cold walk, never a
/// wrong hit). [`CACHE_SCHEMA_VERSION`] guards the *meaning* of the
/// inputs, so the fingerprint need only be self-consistent within one
/// format version.
#[must_use]
pub(crate) fn fingerprint(options: &Options) -> u64 {
    let mut hasher = DefaultHasher::new();
    options.long_window_secs.hash(&mut hasher);
    options.recent_window_secs.hash(&mut hasher);
    options.full_history.hash(&mut hasher);
    options.include_merges.hash(&mut hasher);
    options.follow_renames.hash(&mut hasher);
    options.exclude_bots.hash(&mut hasher);
    options.bot_pattern.hash(&mut hasher);
    options.as_of.hash(&mut hasher);
    hasher.finish()
}

/// The default cache root: `$XDG_CACHE_HOME/big-code-analysis/vcs`, or the
/// platform equivalent (`%LOCALAPPDATA%` on Windows, `~/.cache` as the
/// POSIX fallback). `None` when no home/cache location can be resolved, in
/// which case caching is simply disabled rather than erroring.
#[must_use]
pub(crate) fn default_cache_dir() -> Option<PathBuf> {
    let suffix = Path::new("big-code-analysis").join("vcs");
    if let Some(xdg) = non_empty_env("XDG_CACHE_HOME") {
        return Some(PathBuf::from(xdg).join(&suffix));
    }
    #[cfg(windows)]
    if let Some(local) = non_empty_env("LOCALAPPDATA") {
        return Some(PathBuf::from(local).join(&suffix));
    }
    let home = non_empty_env("HOME")?;
    Some(PathBuf::from(home).join(".cache").join(&suffix))
}

/// Read an environment variable, treating an unset *or empty* value as
/// absent (an empty `HOME` is as useless as a missing one).
fn non_empty_env(key: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(key).filter(|value| !value.is_empty())
}

/// The per-repository sub-directory under `cache_root`, named by a hash of
/// the repository's canonical path so distinct working trees never share a
/// directory (and the same tree is stable across runs).
#[must_use]
pub(crate) fn repo_dir(cache_root: &Path, repo_canonical: &Path) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    repo_canonical.hash(&mut hasher);
    cache_root.join(format!("{:016x}", hasher.finish()))
}

/// The cache file for one `HEAD` SHA within a repository directory.
#[must_use]
pub(crate) fn entry_path(repo_dir: &Path, head_sha: &str) -> PathBuf {
    repo_dir.join(format!("{head_sha}.json"))
}

/// Load a single cache entry, returning `None` for a missing, unreadable,
/// or corrupt file (all non-fatal — the history is simply recomputed).
#[must_use]
pub(crate) fn load(path: &Path) -> Option<HistoryCache> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Load every entry in `repo_dir` whose versions and option fingerprint
/// match the current run, paired with its file path. Used to find a prior
/// entry whose `HEAD` is an ancestor of the current one for an incremental
/// walk. A non-existent directory yields an empty list.
#[must_use]
pub(crate) fn load_compatible(
    repo_dir: &Path,
    options_fingerprint: u64,
) -> Vec<(PathBuf, HistoryCache)> {
    let Ok(entries) = std::fs::read_dir(repo_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json")
            && let Some(cache) = load(&path)
            && cache.is_compatible(options_fingerprint)
        {
            out.push((path, cache));
        }
    }
    out
}

/// Monotonic counter making concurrent temp-file names unique within a
/// process; combined with the PID it avoids two writers colliding on the
/// same temporary path before the atomic rename.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Persist a cache entry atomically: write to a uniquely-named temporary
/// file in the destination directory, then rename it over the target so a
/// concurrent reader never observes a half-written file.
///
/// # Errors
///
/// Returns [`Error::Cache`] if the directory cannot be created or the
/// file cannot be written or renamed.
pub(crate) fn write_atomic(path: &Path, cache: &HistoryCache) -> Result<(), Error> {
    let dir = path
        .parent()
        .ok_or_else(|| Error::Cache(format!("cache path {} has no parent", path.display())))?;
    std::fs::create_dir_all(dir).map_err(|e| cache_io_err("create cache directory", dir, &e))?;

    let unique = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{}.{unique}.tmp", std::process::id()));
    let json =
        serde_json::to_vec(cache).map_err(|e| Error::Cache(format!("serializing cache: {e}")))?;
    std::fs::write(&tmp, &json).map_err(|e| cache_io_err("write cache temp file", &tmp, &e))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        // Best-effort cleanup of the orphaned temp file on a failed rename.
        let _ = std::fs::remove_file(&tmp);
        cache_io_err("rename cache file", path, &e)
    })
}

/// Remove a repository's entire cache directory (`--clear-cache`). A
/// missing directory is success, not an error.
///
/// # Errors
///
/// Returns [`Error::Cache`] if the directory exists but cannot be removed.
pub(crate) fn clear_repo(repo_dir: &Path) -> Result<(), Error> {
    match std::fs::remove_dir_all(repo_dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(cache_io_err("clear cache directory", repo_dir, &e)),
    }
}

/// Build an [`Error::Cache`] naming the failed operation and path.
fn cache_io_err(action: &str, path: &Path, error: &std::io::Error) -> Error {
    Error::Cache(format!("{action} {}: {error}", path.display()))
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;
