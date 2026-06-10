// bca: suppress-file(halstead, nargs, nexits)
// File-level halstead/nargs/exit are many-fn aggregation artifacts (the
// timeline walk + per-point build loop with `?` error maps), not
// per-function logic complexity (cognitive/cyclomatic stay enforced).

//! The `vcs-git` backend for the historical metric trend (issue #333).
//!
//! [`build_trend`] samples the change-history metrics at several points in
//! time. The naïve approach — set `--as-of` to a past time but keep
//! walking from `HEAD` — is wrong: the walk would still count commits made
//! *after* that point (they are reachable from `HEAD`, and the future-date
//! clamp folds them onto `now`). So each point instead resolves the
//! mainline tip *at or before* that timestamp from a single first-parent
//! walk, then runs an ordinary [`build`] anchored at that historical
//! commit with `as_of` set to the point. That yields a faithful snapshot:
//! the tree, the windows, and the in-window history all match what the
//! repository looked like at that moment.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gix::revision::walk::Sorting;
use gix::traverse::commit::simple::CommitTimeOrder;

use super::history::commit_seconds;
use super::repo::{self, OpenRepo};
use super::{build, current_unix_seconds, walk_err};
use crate::vcs::error::Error;
use crate::vcs::options::Options;
use crate::vcs::stats::Stats;
use crate::vcs::trend::{self, Trend};

/// Build a historical [`Trend`] rooted at `root`.
///
/// `base` carries the windows / bot / merge / rename / formula knobs
/// shared by every sampled point (its `reference` selects the mainline to
/// follow, and its `as_of`, if set, anchors the most-recent point);
/// `points` and `span_secs` define the sampling grid. See the module
/// docs for why each point re-anchors at a historical commit.
///
/// # Errors
///
/// Returns [`Error::InvalidTrend`] when `points` is out of range,
/// [`Error::NotARepository`] / [`Error::ResolveRef`] when the repository
/// or base reference cannot be resolved, or a walk/diff variant when a
/// sampled snapshot's history walk fails.
pub(crate) fn build_trend(
    root: &Path,
    base: &Options,
    points: usize,
    span_secs: i64,
) -> Result<Trend, Error> {
    trend::validate_points(points)?;
    let end = base.as_of.unwrap_or_else(current_unix_seconds);
    let stamps = trend::timestamps(end, span_secs, points);

    // One first-parent walk of the base reference yields every mainline
    // tip and its time, so each point is an O(commits) lookup rather than
    // its own walk.
    let OpenRepo { repo, .. } = repo::open(root)?;
    // Take the owned id so the borrowing `Commit` is dropped before the
    // repo handle is.
    let tip = repo::resolve_commit(&repo, &base.reference)?.id;
    let timeline = first_parent_timeline(&repo, tip)?;
    // Free the timeline repo's object cache before the per-point builds
    // each open their own handle.
    drop(repo);

    let mut per_point: Vec<HashMap<PathBuf, Stats>> = Vec::with_capacity(stamps.len());
    let mut truncated = false;
    for &at in &stamps {
        let Some(oid) = tip_at_or_before(&timeline, at) else {
            // No commit at or before this point: the repository did not
            // exist yet, so every file stays `None` here.
            per_point.push(HashMap::new());
            continue;
        };
        let index = build(root, &snapshot_options(base, oid, at))?;
        truncated |= index.truncated_shallow_clone();
        per_point.push(index.into_files());
    }

    Ok(Trend::from_snapshots(
        stamps,
        per_point,
        base.long_window_days(),
        base.recent_window_days(),
        truncated,
    ))
}

/// Derive the per-point [`Options`] from `base`: anchor the walk at the
/// historical tip `oid`, set the reference "now" to the point `at`, and
/// drop the bus-factor aggregate (not part of the per-file series).
fn snapshot_options(base: &Options, oid: gix::ObjectId, at: i64) -> Options {
    let mut options = base.clone();
    options.reference = oid.to_hex().to_string();
    options.as_of = Some(at);
    options.compute_bus_factor = false;
    options
}

/// Collect `(commit_time, id)` for every first-parent ancestor of `tip`,
/// newest-first. This is the mainline timeline the per-point tip lookups
/// search.
fn first_parent_timeline(
    repo: &gix::Repository,
    tip: gix::ObjectId,
) -> Result<Vec<(i64, gix::ObjectId)>, Error> {
    let walk = repo
        .rev_walk([tip])
        .first_parent_only()
        .sorting(Sorting::ByCommitTime(CommitTimeOrder::NewestFirst))
        .all()
        .map_err(walk_err)?;
    let mut timeline = Vec::new();
    for info in walk {
        let info = info.map_err(walk_err)?;
        // `Info::commit_time` is not populated for this traversal, so the
        // commit must be decoded to read its time; `commit_seconds` uses
        // the pre-decoded value when present and decodes otherwise.
        let commit = info.object().map_err(walk_err)?;
        timeline.push((commit_seconds(&info, &commit)?, info.id));
    }
    Ok(timeline)
}

/// The id of the commit with the greatest commit time at or before `at`,
/// or `None` when every commit is newer (the repository did not exist yet
/// at that point). A scan (not a binary search) so out-of-order commit
/// times from clock skew or history rewriting cannot misselect the tip.
fn tip_at_or_before(timeline: &[(i64, gix::ObjectId)], at: i64) -> Option<gix::ObjectId> {
    timeline
        .iter()
        .filter(|&&(time, _)| time <= at)
        .max_by_key(|&&(time, _)| time)
        .map(|&(_, oid)| oid)
}

#[cfg(test)]
#[path = "trend_tests.rs"]
mod tests;
