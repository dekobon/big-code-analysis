// bca: suppress-file(halstead, nargs, exit)
// File-level halstead/nargs/exit are many-fn aggregation artifacts (the
// open/resolve/enumerate/finalize pipeline), not per-function logic
// complexity (cognitive/cyclomatic stay enforced).

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
mod history;
mod identity;
mod jit;
mod repo;
mod trend;

pub use blame::{LineSpan, PerFunctionBlame};
pub(crate) use cached::build_cached;
pub(crate) use jit::score_commit;
pub(crate) use trend::build_trend;

use std::collections::HashSet;
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

    // Resolve the ref to a commit, peeling through any tag.
    let commit = repo::resolve_commit(&repo, &options.reference)?;
    let target_tree = commit.tree().map_err(walk_err)?;

    // Seed file set (path → SLOC) at the target ref.
    let seed = repo::enumerate_target_files(&repo, &target_tree)?;

    let now = options.as_of.unwrap_or_else(current_unix_seconds);
    // Uncached: walk the whole long window (no splice points) and replay
    // the resulting event log — the same fold a cache hit takes, so the
    // two cannot diverge.
    let (events, _) = history::collect_events(&repo, commit.id, options, now, &HashSet::new())?;
    let out = replay::replay(seed, &events, options, now);
    Ok(HistoryIndex::new(out.files, workdir, shallow).with_bus_factor(out.bus_factor))
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
    gix::date::parse(input, Some(SystemTime::now()))
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
