//! The `vcs-git` backend: a `gix`-powered history walk.
//!
//! `build` is the single entry point `build_history_index` delegates
//! to. It opens the repository, resolves the target ref, enumerates the
//! tracked text files at that ref, walks history once into a raw
//! `CommitEvent` log, then replays that
//! log into per-file [`Stats`](crate::vcs::stats::Stats) (the `replay`
//! module). Routing the walk through the same replay a cache hit uses is
//! what keeps the two bit-identical (issue #334); `build_cached` adds the
//! persistent-cache layer on top.
//!
//! Per-function attribution (issue #329) is a separate, blame-based
//! path: see [`PerFunctionBlame`].

mod blame;
mod cached;
mod diff_parse;
mod history;
mod identity;
mod jit;
mod repo;
mod trend;

pub use blame::{BlameSession, LineSpan, PerFunctionBlame};
pub(crate) use cached::build_cached;
pub(crate) use diff_parse::score_diff;
pub(crate) use jit::score_commit;
pub(crate) use repo::workdir_root;
pub(crate) use trend::build_trend;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::vcs::HistoryIndex;
use crate::vcs::error::Error;
use crate::vcs::options::Options;
use crate::vcs::replay;

/// Object-cache budget for the walk. Tree diffs look up the same blobs
/// repeatedly; a few MiB of cache turns an O(commits²)-ish blob-decode
/// pattern into something tractable (gix docs guidance). Only applied
/// when the repository config has not already set one.
pub(super) const OBJECT_CACHE_BYTES: usize = 8 * 1024 * 1024;

/// Walk git history rooted at `root` and build a [`HistoryIndex`].
///
/// # Errors
///
/// See [`build_history_index`](crate::vcs::build_history_index).
pub(crate) fn build(root: &Path, options: &Options) -> Result<HistoryIndex, Error> {
    let repo::OpenRepo {
        mut repo,
        workdir,
        shallow,
    } = repo::open(root)?;
    repo.object_cache_size_if_unset(OBJECT_CACHE_BYTES);

    // Resolve the walk anchor. With `--as-of`, re-anchor at the mainline
    // tip at-or-before that time (issue #648): a plain HEAD-anchored walk
    // re-bases only the window arithmetic, so commits in the *future* of
    // `as_of` (reachable from HEAD) still slip into the windowed counts,
    // contradicting the "reproducible snapshot" the flag documents. This
    // mirrors `vcs trend` (#333), reusing its `tip_at_or_before` over the
    // reference's first-parent timeline. Without `--as-of`, anchor at the
    // resolved reference tip directly.
    let Some(commit) = resolve_anchor(&repo, options)? else {
        // `as_of` predates the first commit on the reference's mainline:
        // the repository did not exist yet at that point, so the snapshot
        // is empty — handled gracefully, not as an error (matching trend).
        return Ok(HistoryIndex::new(HashMap::new(), workdir, shallow));
    };
    let target_tree = commit.tree().map_err(walk_err)?;

    // Seed file set (path → SLOC) at the target ref, scoped to the
    // requested file types (issue #576).
    let seed = repo::enumerate_target_files(&repo, &target_tree, &options.file_types)?;

    let now = options.as_of.unwrap_or_else(current_unix_seconds);
    // Uncached: walk the whole long window (no splice points) and replay
    // the resulting event log — the same fold a cache hit takes, so the
    // two cannot diverge.
    let (events, _) = history::collect_events(&repo, commit.id, options, now, &HashSet::new())?;
    let out = replay::replay(seed, &events, options, now);
    Ok(HistoryIndex::new(out.files, workdir, shallow).with_bus_factor(out.bus_factor))
}

/// Resolve the commit the walk should anchor at.
///
/// Without `--as-of`, this is the resolved reference tip. With `--as-of`,
/// it is the mainline (first-parent) tip at-or-before that timestamp, so
/// the windowed counts, the seeded file set, and the SLOC all reflect the
/// repository as it stood at that moment rather than at HEAD (issue #648).
/// Returns `Ok(None)` when `--as-of` predates the first commit on the
/// reference's mainline (the empty-snapshot case).
fn resolve_anchor<'repo>(
    repo: &'repo gix::Repository,
    options: &Options,
) -> Result<Option<gix::Commit<'repo>>, Error> {
    let tip = repo::resolve_commit(repo, &options.reference)?;
    let Some(as_of) = options.as_of else {
        return Ok(Some(tip));
    };
    let timeline = trend::first_parent_timeline(repo, tip.id)?;
    let Some(anchor) = trend::tip_at_or_before(&timeline, as_of) else {
        return Ok(None);
    };
    // Re-resolve the historical tip to an owned `Commit`; it lies on the
    // reference's first-parent mainline by construction.
    Ok(Some(repo::resolve_commit(
        repo,
        &anchor.to_hex().to_string(),
    )?))
}

/// Map any backend error into [`Error::Walk`] — the catch-all for
/// rev-walk, object-lookup, and tree-decode failures. Shared by the
/// backend submodules to keep the `?`-heavy gix plumbing terse.
pub(super) fn walk_err(e: impl std::fmt::Display) -> Error {
    Error::Walk(e.to_string())
}

/// Map any backend error into [`Error::Diff`] — tree-to-tree and
/// blob-diff failures.
pub(super) fn diff_err(e: impl std::fmt::Display) -> Error {
    Error::Diff(e.to_string())
}

/// Parse an `--as-of` timestamp (RFC 3339 / ISO 8601 / `@unix` / git
/// date spellings) into Unix seconds via gix's date parser.
///
/// # Errors
///
/// Returns [`Error::InvalidTimestamp`] when the input is unparseable.
pub(crate) fn parse_timestamp(input: &str) -> Result<i64, Error> {
    // Accept a bare `@<unix>` epoch directly — gix's date parser does
    // not recognise that spelling, but it is a convenient reproducible
    // form for `--as-of`.
    if let Some(epoch) = input.strip_prefix('@') {
        return epoch
            .parse::<i64>()
            .map_err(|_| Error::InvalidTimestamp(format!("{input:?}: not a Unix timestamp")));
    }
    gix::date::parse(input, Some(gix::date::Zoned::now()))
        .map(|time| time.seconds)
        .map_err(|e| Error::InvalidTimestamp(format!("{input:?}: {e}")))
}

/// Wall-clock time as Unix seconds, saturating rather than panicking if
/// the system clock predates the epoch.
pub(super) fn current_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
