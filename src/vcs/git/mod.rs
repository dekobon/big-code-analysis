// bca: suppress-file(halstead, nargs, exit)
// File-level halstead/nargs/exit are many-fn aggregation artifacts (the
// open/resolve/enumerate/finalize pipeline), not per-function logic
// complexity (cognitive/cyclomatic stay enforced).

//! The `vcs-git` backend: a `gix`-powered history walk.
//!
//! `build` is the single entry point `build_history_index` delegates
//! to. It opens the repository, resolves the target ref, enumerates the
//! tracked text files at that ref (seeding one accumulator per file),
//! walks history once to fold in per-file signals, then finalises each
//! accumulator into a [`Stats`] record.

mod history;
mod identity;
mod repo;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::vcs::HistoryIndex;
use crate::vcs::error::Error;
use crate::vcs::options::{Options, RiskFormula};
use crate::vcs::score;
use crate::vcs::stats::{Accumulator, Stats};

/// Object-cache budget for the walk. Tree diffs look up the same blobs
/// repeatedly; a few MiB of cache turns an O(commits²)-ish blob-decode
/// pattern into something tractable (gix docs guidance). Only applied
/// when the repository config has not already set one.
const OBJECT_CACHE_BYTES: usize = 8 * 1024 * 1024;

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
    let resolve_err = |e: &dyn std::fmt::Display| Error::ResolveRef {
        reference: options.reference.clone(),
        reason: e.to_string(),
    };
    let tip = repo
        .rev_parse_single(options.reference.as_bytes())
        .map_err(|e| resolve_err(&e))?;
    let commit = tip
        .object()
        .map_err(|e| resolve_err(&e))?
        .peel_to_commit()
        .map_err(|e| resolve_err(&e))?;
    let target_tree = commit.tree().map_err(walk_err)?;

    // Seed one accumulator per tracked text file at the target ref.
    let mut accumulators: HashMap<PathBuf, Accumulator> =
        repo::enumerate_target_files(&repo, &target_tree)?
            .into_iter()
            .map(|(path, sloc)| (path, Accumulator::new(sloc)))
            .collect();

    let now = options.as_of.unwrap_or_else(current_unix_seconds);
    history::walk_history(&repo, commit.id, options, now, &mut accumulators)?;

    let files = finalize(accumulators, options, now);
    Ok(HistoryIndex::new(files, workdir, shallow))
}

/// Finalise every accumulator into a [`Stats`] record, applying the
/// percentile re-ranking pass when that formula is selected.
fn finalize(
    accumulators: HashMap<PathBuf, Accumulator>,
    options: &Options,
    now: i64,
) -> HashMap<PathBuf, Stats> {
    let (paths, mut stats): (Vec<PathBuf>, Vec<Stats>) = accumulators
        .into_iter()
        .map(|(path, acc)| (path, acc.finalize(now, options)))
        .unzip();

    if options.risk_formula == RiskFormula::Percentile {
        score::apply_percentile(&mut stats);
    }

    paths.into_iter().zip(stats).collect()
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
fn current_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}
