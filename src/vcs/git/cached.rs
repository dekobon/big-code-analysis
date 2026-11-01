//! Cache-aware history build: reuse a persisted event log, extend it by
//! walking only new commits, or fall back to a fresh walk (issue #334).
//!
//! The flow, given the resolved `HEAD` SHA `H`:
//!
//! 1. **Pure hit** — an entry named `H` exists, its versions/fingerprint
//!    match, and it reaches back far enough: replay it, no walk.
//! 2. **Incremental** (first-parent mode only) — walk from `H` newest-first
//!    until a commit already cached as an ancestor is hit, then splice the
//!    new events onto that cached tail.
//! 3. **Miss / force-push** — `H` diverged from every cached head (the
//!    ancestor was never reached within the window), so the walk covers the
//!    full window on its own; the stale entry is simply overwritten.
//!
//! Every path ends in the same [`replay`](crate::vcs::replay), so a cache
//! hit is bit-identical to a fresh walk, and re-windowing tracks the
//! current `now` rather than freezing at cache-write time.
//!
//! That contract holds only while every input the walk *records* is
//! covered by the entry key or the fingerprint. Three repository-side
//! inputs are neither, and all enter [`cache::fingerprint`] here through
//! [`history::WalkContext::input_digests`], which covers all three cache
//! paths at once — the pure hit, `load_compatible`'s ancestor selection
//! for the incremental splice, and the entry [`persist`] writes back. The
//! `.mailmap` is one: author identities are canonicalised through it at
//! walk time and stored as digests, while an edit to it moves no `HEAD`
//! (issue #1262). The git diff configuration is another: `diff.algorithm`,
//! `core.bigFileThreshold` and the `diff.<driver>` sections' `algorithm`,
//! `binary` and `textconv` configure the
//! blob-diff platform that produces the recorded churn, and a change to
//! them was served stale — and spliced forward under each new head — until
//! `--clear-cache` (issue #1425). The shallow boundary is the third: a
//! still-shallow clone deepened in place (`git fetch --deepen`, a larger
//! `--depth`, an earlier `--shallow-since`) moves neither `HEAD` nor the
//! `is_shallow()` flag [`cache::HistoryCache::is_compatible`] checks, so
//! only a full `--unshallow` (issue #810) retired the entry walked at the
//! old depth; a deepen was a pure hit on it, and the splice re-persisted
//! its truncated tail under each new head. A digest can only be taken
//! from a context's snapshot, platform and shallow-file read, and this
//! build opens exactly one context and walks through it, so an entry's
//! stamp and its events share one read of the mailmap and of the diff
//! configuration (issue #1409). gix's walk reads the shallow file again;
//! [`history::WalkContext`] says why that second read can cost a cold walk
//! but not a stale hit.
//!
//! Fingerprinting the diff configuration was chosen over pinning the
//! algorithm in the walk. A pin would also make churn host-independent,
//! but it covers only `diff.algorithm`: a driver's own `algorithm`,
//! `binary`, and `core.bigFileThreshold` would still need the fingerprint,
//! and it changes the churn recorded for every repository with a
//! non-default algorithm — a definition change for the metric, not a
//! cache fix. The fingerprint only invalidates, so a cached run keeps
//! reporting what `git diff` on that host reports.
//!
//! One walk input remains outside both the key and the fingerprint: the
//! `diff` attribute bindings that route a path to a driver or mark it
//! `-diff`. gix resolves them for every commit from the *current* index
//! and attribute files, so a binding change is served stale until
//! `--clear-cache` whether or not it moves `HEAD`: a staged
//! `.gitattributes` or `info/attributes` edit hits the exact entry, and a
//! committed `.gitattributes` edit splices onto the tail walked under the
//! old bindings. [`history::WalkContext::diff_config_digest`] says why
//! they are left out.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::{OBJECT_CACHE_BYTES, current_unix_seconds, history, repo, walk_err};
use crate::diag::warn;
use crate::vcs::HistoryIndex;
use crate::vcs::cache::{self, CacheConfig, CommitEvent, HistoryCache};
use crate::vcs::error::Error;
use crate::vcs::options::Options;
use crate::vcs::replay;
use crate::vcs::score::RISK_SCORE_VERSION;
use crate::vcs::stats::VCS_SCHEMA_VERSION;

/// Build a [`HistoryIndex`], reusing and updating the persistent cache per
/// `config`.
///
/// # Errors
///
/// Returns the same variants as [`build`](super::build), plus
/// [`Error::Cache`] when `--clear-cache` is requested but the cache
/// directory cannot be removed. A failure to *write* a fresh entry is
/// non-fatal — it is logged and the computed index returned regardless.
pub(crate) fn build_cached(
    root: &Path,
    options: &Options,
    config: &CacheConfig,
) -> Result<HistoryIndex, Error> {
    // bca: suppress(abc)
    // Linear sequence over cache outcomes (disabled / pure hit / miss):
    // each step is already comment-delimited and the per-step work
    // (collect_events, incremental_events, assemble, persist) lives in its
    // own helper. The ABC count is the call/assignment density of wiring
    // those outcomes, not branching depth — cognitive is not flagged.
    // Extracting the pure-hit arm would need ~9 cache-key params (tripping
    // nargs), and a grouping struct introduced only to dodge that would be
    // gaming, not clarity.
    let repo::OpenRepo {
        mut repo,
        workdir,
        shallow,
    } = repo::open(root)?;
    repo.object_cache_size_if_unset(OBJECT_CACHE_BYTES);

    let commit = repo::resolve_commit(&repo, &options.reference)?;

    // An `--as-of` that predates the resolved tip re-anchors the walk at the
    // historical mainline tip at-or-before that time (issue #648). The
    // persistent cache is keyed and seeded on the *current* HEAD, so it
    // cannot serve (nor should it store) such a one-off historical query —
    // route to the re-anchored plain walk. Without this, `bca vcs --as-of`
    // (which the CLI/web/py all drive through this cached entry point) would
    // count commits in the future of `as_of`. An `as_of` at-or-after the tip
    // selects HEAD anyway, so caching stays valid and is left untouched (the
    // reproducibility-pinned cache tests rely on this).
    if let Some(as_of) = options.as_of {
        let head_time = commit.time().map_err(walk_err)?.seconds;
        if as_of < head_time {
            return super::build(root, options);
        }
    }

    let head_oid = commit.id;
    let head_sha = head_oid.to_string();
    let target_tree = commit.tree().map_err(walk_err)?;
    let now = options.as_of.unwrap_or_else(current_unix_seconds);

    // Resolve the per-repository cache directory (explicit `--cache-dir`,
    // else the platform default). Keyed by the canonical working-tree path
    // so distinct checkouts never collide.
    let repo_dir = cache_dir(config, root, workdir.as_deref());

    if config.clear
        && let Some(dir) = &repo_dir
    {
        cache::clear_repo(dir)?;
    }

    let seed = repo::enumerate_target_files(&repo, &target_tree, &options.file_types)?;

    // Opened before the `--no-cache` early return: the walk needs the
    // snapshot and the diff platform either way; only the digests are
    // skipped there.
    let walk = history::WalkContext::open(&repo, options, now)?;

    // Caching disabled, or no cache location available: plain walk + replay.
    let Some(repo_dir) = repo_dir.filter(|_| config.enabled) else {
        let (events, _) = history::collect_events(&walk, head_oid, &HashSet::new())?;
        return Ok(assemble(seed, &events, options, now, workdir, shallow));
    };

    let fingerprint = cache::fingerprint(options, walk.input_digests());
    let long_boundary = walk.long_boundary();

    // Pure hit: an exact, compatible entry that reaches back far enough.
    let exact_path = cache::entry_path(&repo_dir, &head_sha);
    if let Some(entry) = cache::load(&exact_path).filter(|entry| {
        entry.is_compatible(fingerprint, shallow) && entry.walk_long_boundary <= long_boundary
    }) {
        return Ok(assemble(
            seed,
            &entry.events,
            options,
            now,
            workdir,
            shallow,
        ));
    }

    // Miss: walk only the new commits and splice them onto a cached tail.
    let (events, superseded) =
        incremental_events(&walk, &repo_dir, head_oid, &head_sha, fingerprint, shallow)?;

    let index = assemble(seed, &events, options, now, workdir, shallow);

    persist(
        &exact_path,
        &events,
        fingerprint,
        &head_sha,
        long_boundary,
        shallow,
    );
    // The spliced ancestor is now wholly covered by this head's entry;
    // remove it so per-branch cache growth stays bounded.
    if let Some(path) = superseded {
        let _ = std::fs::remove_file(path);
    }

    Ok(index)
}

/// Walk the commits newer than any cached ancestor and splice them onto the
/// cached tail, returning the merged event log and (when a tail was reused)
/// the now-superseded entry's path.
///
/// A full-DAG splice is unsound — a merge can introduce side-branch commits
/// absent from the cached first-parent tail — so `--full-history` collects
/// no candidates and always re-walks the full window.
fn incremental_events(
    walk: &history::WalkContext<'_>,
    repo_dir: &Path,
    head_oid: gix::ObjectId,
    head_sha: &str,
    fingerprint: u64,
    shallow: bool,
) -> Result<(Vec<CommitEvent>, Option<PathBuf>), Error> {
    let long_boundary = walk.long_boundary();
    let candidates = load_candidates(
        repo_dir,
        fingerprint,
        long_boundary,
        walk.options.full_history,
        shallow,
    );
    let stop_oids: HashSet<gix::ObjectId> = candidates.keys().copied().collect();

    let (new_events, stopped) = history::collect_events(walk, head_oid, &stop_oids)?;

    // Splice onto the cached tail when the walk reached a known ancestor.
    let spliced = stopped.and_then(|oid| candidates.get(&oid));
    let superseded = spliced
        .filter(|candidate| candidate.cache.head_sha != head_sha)
        .map(|candidate| candidate.path.clone());

    let mut events = new_events;
    if let Some(candidate) = spliced {
        events.extend(candidate.cache.events.iter().cloned());
    }
    Ok((prune_and_dedup(events, walk.now, long_boundary), superseded))
}

/// A loaded candidate entry plus the path it came from (for supersession).
struct Candidate {
    path: PathBuf,
    cache: HistoryCache,
}

/// Resolve the per-repository cache directory, or `None` when caching has
/// no usable location (no `--cache-dir` and no home/cache env).
fn cache_dir(config: &CacheConfig, root: &Path, workdir: Option<&Path>) -> Option<PathBuf> {
    let cache_root = config.dir.clone().or_else(cache::default_cache_dir)?;
    let canonical = workdir.map_or_else(
        || root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
        Path::to_path_buf,
    );
    Some(cache::repo_dir(&cache_root, &canonical))
}

/// Load the compatible, far-enough-reaching candidate entries keyed by
/// their head object id. Empty for `--full-history` (no incremental splice)
/// or an unparseable head SHA.
fn load_candidates(
    repo_dir: &Path,
    fingerprint: u64,
    long_boundary: i64,
    full_history: bool,
    shallow: bool,
) -> HashMap<gix::ObjectId, Candidate> {
    if full_history {
        return HashMap::new();
    }
    cache::load_compatible(repo_dir, fingerprint, shallow)
        .into_iter()
        // Keep only entries that reach back at least as far as this run's
        // window (a shorter cached reach cannot fill the bottom of it).
        .filter(|(_, cache)| cache.walk_long_boundary <= long_boundary)
        .filter_map(|(path, cache)| {
            let oid = gix::ObjectId::from_hex(cache.head_sha.as_bytes()).ok()?;
            Some((oid, Candidate { path, cache }))
        })
        .collect()
}

/// Replay `events` into a finalized [`HistoryIndex`] with the working-tree
/// metadata the backend holds.
fn assemble(
    seed: HashMap<PathBuf, u64>,
    events: &[CommitEvent],
    options: &Options,
    now: i64,
    workdir: Option<PathBuf>,
    shallow: bool,
) -> HistoryIndex {
    let out = replay::replay(seed, events, options, now);
    HistoryIndex::new(out.files, workdir, shallow).with_bus_factor(out.bus_factor)
}

/// Drop events now outside the long window and de-duplicate by object id
/// (keeping the first, i.e. newest, across a splice boundary).
fn prune_and_dedup(events: Vec<CommitEvent>, now: i64, long_boundary: i64) -> Vec<CommitEvent> {
    let mut seen = HashSet::new();
    events
        .into_iter()
        .filter(|event| event.time.min(now) >= long_boundary)
        .filter(|event| seen.insert(event.oid.clone()))
        .collect()
}

/// Write the fresh entry atomically. A write failure is logged, not
/// propagated — the cache is an optimization, never a correctness gate.
fn persist(
    path: &Path,
    events: &[CommitEvent],
    fingerprint: u64,
    head_sha: &str,
    long_boundary: i64,
    shallow: bool,
) {
    let entry = HistoryCache {
        cache_schema_version: cache::CACHE_SCHEMA_VERSION,
        vcs_schema_version: VCS_SCHEMA_VERSION,
        risk_score_version: RISK_SCORE_VERSION,
        options_fingerprint: fingerprint,
        head_sha: head_sha.to_owned(),
        walk_long_boundary: long_boundary,
        truncated_shallow_clone: shallow,
        events: events.to_vec(),
    };
    if let Err(e) = cache::write_atomic(path, &entry) {
        warn(format_args!("failed to write VCS history cache: {e}"));
    }
}
