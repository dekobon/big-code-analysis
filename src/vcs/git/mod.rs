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
//!
//! Per-function attribution (issue #329) is a separate, blame-based
//! path: see [`PerFunctionBlame`].

mod blame;
mod history;
mod identity;
mod jit;
mod repo;
mod trend;

pub use blame::{LineSpan, PerFunctionBlame};
pub(crate) use jit::score_commit;
pub(crate) use trend::build_trend;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::vcs::HistoryIndex;
use crate::vcs::bus_factor;
use crate::vcs::entropy::CochangeGraph;
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
    let commit = repo::resolve_commit(&repo, &options.reference)?;
    let target_tree = commit.tree().map_err(walk_err)?;

    // Seed one accumulator per tracked text file at the target ref.
    let mut accumulators: HashMap<PathBuf, Accumulator> =
        repo::enumerate_target_files(&repo, &target_tree)?
            .into_iter()
            .map(|(path, sloc)| (path, Accumulator::new(sloc)))
            .collect();

    let now = options.as_of.unwrap_or_else(current_unix_seconds);
    let graph = history::walk_history(&repo, commit.id, options, now, &mut accumulators)?;

    // The bus factor needs per-file authorship, which `finalize` discards;
    // collect it first, and only when opted in (the repeated JIT-prior and
    // per-file-injection walks leave the flag off, so they pay nothing).
    let bus_factor = options
        .compute_bus_factor
        .then(|| bus_factor_aggregate(&accumulators, options))
        .flatten();

    let files = finalize(accumulators, &graph, options, now);
    Ok(HistoryIndex::new(files, workdir, shallow).with_bus_factor(bus_factor))
}

/// Build the directory- / repo-level bus-factor aggregate from the
/// per-file accumulators (issue #332). Files with no in-window authorship
/// contribute no signal and are excluded from the denominator; when *no*
/// file has any (an empty repo, or one whose only commits were bot-only or
/// out-of-window) the aggregate is `None` so front ends omit the
/// `vcs_aggregate` block rather than emit a meaningless "0 over 0 files".
fn bus_factor_aggregate(
    accumulators: &HashMap<PathBuf, Accumulator>,
    options: &Options,
) -> Option<bus_factor::BusFactor> {
    let authorship: Vec<bus_factor::FileAuthorship> = accumulators
        .iter()
        .filter_map(|(path, acc)| {
            acc.authorship()
                .map(|contributions| bus_factor::FileAuthorship {
                    path: path.clone(),
                    contributions,
                })
        })
        .collect();
    if authorship.is_empty() {
        return None;
    }
    Some(bus_factor::compute(
        &authorship,
        options.bus_factor_threshold,
        options.emit_author_details,
    ))
}

/// Finalise every accumulator into a [`Stats`] record, joining in each
/// file's co-change graph entropy (the one signal the per-file
/// accumulator cannot compute alone) and applying the percentile
/// re-ranking pass when that formula is selected.
fn finalize(
    accumulators: HashMap<PathBuf, Accumulator>,
    graph: &CochangeGraph,
    options: &Options,
    now: i64,
) -> HashMap<PathBuf, Stats> {
    let (paths, mut stats): (Vec<PathBuf>, Vec<Stats>) = accumulators
        .into_iter()
        .map(|(path, acc)| {
            let (cochange_long, cochange_recent) = graph.entropy(&path);
            let stats = acc.finalize(now, options, cochange_long, cochange_recent);
            (path, stats)
        })
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
pub(super) fn current_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
