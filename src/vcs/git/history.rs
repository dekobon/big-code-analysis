// bca: suppress-file(halstead, nargs, nexits)
// File-level halstead/nargs/exit are many-fn aggregation artifacts (the
// gix rev-walk + per-commit diff plumbing, with many `?` error maps),
// not per-function logic complexity (cognitive/cyclomatic stay enforced).

//! The commit-history walk: rev-walk the target ref within the long
//! window, diff each commit against its first parent, and emit one raw
//! [`CommitEvent`] per in-window commit.
//!
//! The walk no longer folds signals inline; it produces the pre-finalize
//! event log that [`replay`](crate::vcs::replay) reduces to per-file
//! [`Stats`](crate::vcs::stats::Stats). Routing both the uncached build
//! and a cache hit through the *same* replay is what makes a hit
//! bit-identical to a fresh walk (issue #334). Storing raw events (raw
//! committer time, pre-alias location paths, rename edges) — rather than
//! folded accumulators — is also what lets an incremental walk splice new
//! commits onto a cached tail and re-window at the current `now`.

use std::collections::HashSet;
use std::ops::ControlFlow;
use std::path::PathBuf;

use gix::diff::Rewrites;
use gix::revision::walk::Sorting;
use gix::traverse::commit::simple::CommitTimeOrder;

use super::identity::ParticipantResolver;
use super::repo::bstr_to_path;
use super::{diff_err, walk_err};
use crate::vcs::cache::CommitEvent;
use crate::vcs::classify::{self, Classification};
use crate::vcs::error::Error;
use crate::vcs::identity::{AuthorId, BotFilter};
use crate::vcs::options::Options;

/// Rename edges (`source → destination`) a commit introduced.
type RenameEdges = Vec<(PathBuf, PathBuf)>;
/// Touched text files as `(location path at the commit, churn)` pairs.
type TouchedFiles = Vec<(PathBuf, u64)>;

/// Walk history from `tip`, emitting one [`CommitEvent`] per in-window,
/// non-merge, non-bot-only commit, newest-first.
///
/// `stop_oids` lets an incremental walk halt as soon as it reaches a
/// commit already covered by a cached tail: when a visited commit's id is
/// in the set, the walk stops *before* recording it and returns that id as
/// the splice point. An empty set walks the whole long window (the value
/// returned in `.1` is then `None`).
///
/// # Errors
///
/// Returns a backend [`Error`] variant if the rev-walk, a tree lookup,
/// or a diff fails.
pub(crate) fn collect_events(
    repo: &gix::Repository,
    tip: gix::ObjectId,
    options: &Options,
    now: i64,
    stop_oids: &HashSet<gix::ObjectId>,
) -> Result<(Vec<CommitEvent>, Option<gix::ObjectId>), Error> {
    // Saturating: a garbage extreme `--as-of` (e.g. i64::MIN) must not
    // overflow the cutoff subtraction. Saturating to i64::MIN yields an
    // unbounded-past boundary — every commit timestamp is included, the
    // safe total behavior for a degenerate clock.
    let long_boundary = now.saturating_sub(options.long_window_secs);

    let mailmap = repo.open_mailmap();
    let bots = if options.exclude_bots {
        Some(BotFilter::new(&options.bot_pattern)?)
    } else {
        None
    };
    let resolver = ParticipantResolver::new(&mailmap, bots.as_ref());

    let rewrites = options.follow_renames.then(Rewrites::default);
    let mut cache = repo.diff_resource_cache_for_tree_diff().map_err(diff_err)?;

    let mut events = Vec::new();

    let mut platform = repo.rev_walk([tip]);
    if !options.full_history {
        platform = platform.first_parent_only();
    }
    // The cutoff prunes commits older than the long window during
    // traversal; the in-loop guard re-checks inclusively for the
    // boundary commit and any out-of-order timestamps.
    let walk = platform
        .sorting(Sorting::ByCommitTimeCutoff {
            order: CommitTimeOrder::NewestFirst,
            seconds: long_boundary,
        })
        .all()
        .map_err(walk_err)?;

    for info in walk {
        let info = info.map_err(walk_err)?;

        // Reached a commit already covered by a cached tail: stop before
        // recording it and report it as the splice point.
        if stop_oids.contains(&info.id) {
            return Ok((events, Some(info.id)));
        }

        let commit = info.object().map_err(walk_err)?;

        // Store the raw committer time; the boundary check clamps a
        // future-dated commit (clock skew) to `now`, but replay re-clamps
        // against its own reference time, so the unclamped value is kept.
        let raw_time = commit_seconds(&info, &commit)?;
        if raw_time.min(now) < long_boundary {
            continue;
        }

        if let Some(event) = process_commit(
            repo, &commit, info.id, raw_time, options, &resolver, rewrites, &mut cache,
        )? {
            events.push(event);
        }

        // The resource cache only grows; clear it each commit to keep
        // memory bounded across a long history (gix docs guidance).
        cache.clear_resource_cache();
    }

    Ok((events, None))
}

/// Turn one walked commit into a [`CommitEvent`], or `None` to drop it:
/// merges (unless opted in) and bot-only commits contribute nothing.
/// Diffs against the first parent and records raw (pre-alias) touched
/// locations plus the rename edges replay needs.
#[allow(clippy::too_many_arguments)]
fn process_commit(
    repo: &gix::Repository,
    commit: &gix::Commit<'_>,
    oid: gix::ObjectId,
    raw_time: i64,
    options: &Options,
    resolver: &ParticipantResolver<'_>,
    rewrites: Option<Rewrites>,
    cache: &mut gix::diff::blob::Platform,
) -> Result<Option<CommitEvent>, Error> {
    // Only the first parent (for the diff base) and whether more than
    // one exists (the merge check) are needed, so avoid collecting the
    // parent ids into a `Vec` for every walked commit.
    let mut parent_ids = commit.parent_ids().map(gix::Id::detach);
    let first_parent = parent_ids.next();
    let is_merge = parent_ids.next().is_some();
    if is_merge && !options.include_merges {
        return Ok(None);
    }

    // Participants first: a commit authored solely by filtered bots
    // contributes nothing, so skip it before the (costly) diff.
    let authors = resolver.participants(commit)?;
    if authors.is_empty() {
        return Ok(None);
    }

    let class = classify_commit(commit)?;

    let commit_tree = commit.tree().map_err(walk_err)?;
    let parent_tree = match first_parent {
        // A parent whose object is absent is a shallow-clone boundary
        // (the grafted parent was not fetched). Treat that commit like a
        // root — diff against the empty tree so its files count as
        // additions — rather than aborting the whole walk on a "not
        // found" error. `truncated_shallow_clone` already flags the
        // result so the counts are understood as lower bounds.
        Some(parent) => match repo.try_find_object(parent).map_err(walk_err)? {
            Some(object) => object
                .peel_to_commit()
                .map_err(walk_err)?
                .tree()
                .map_err(walk_err)?,
            None => repo.empty_tree(),
        },
        None => repo.empty_tree(),
    };

    let (renames, touched) = diff_collect(&parent_tree, &commit_tree, cache, rewrites)?;

    Ok(Some(CommitEvent {
        oid: oid.to_string(),
        time: raw_time,
        // Authors are stored only as their irreversible digests — never
        // plaintext (see the cache module's privacy note).
        authors: authors.iter().map(AuthorId::hashed).collect(),
        bug_fix: class.bug_fix,
        security_fix: class.security_fix,
        revert: class.revert,
        renames,
        touched,
    }))
}

/// Diff `parent_tree → commit_tree`, returning the commit's rename edges
/// (`source → destination`) and every touched text file's **location path
/// at this commit** (pre-alias) with its added+deleted churn.
///
/// Alias resolution is deliberately left to replay: storing raw locations
/// plus the rename edges lets a later incremental walk re-home edits when
/// a *newer* commit renames a file a cached commit had touched.
fn diff_collect(
    parent_tree: &gix::Tree<'_>,
    commit_tree: &gix::Tree<'_>,
    cache: &mut gix::diff::blob::Platform,
    rewrites: Option<Rewrites>,
) -> Result<(RenameEdges, TouchedFiles), Error> {
    use gix::object::tree::diff::Change;

    let mut renames: RenameEdges = Vec::new();
    let mut touched: TouchedFiles = Vec::new();
    parent_tree
        .changes()
        .map_err(diff_err)?
        .options(|opts| {
            opts.track_path();
            opts.track_rewrites(rewrites);
        })
        .for_each_to_obtain_tree(commit_tree, |change| -> Result<ControlFlow<()>, Error> {
            // Only regular file blobs carry line churn. Skipping every
            // non-blob mode drops directories, symlinks (per spec), and
            // submodule gitlinks (commit mode) — diffing the latter as a
            // blob is what failed the callback on repos with submodules.
            if !change.entry_mode().is_blob() {
                return Ok(ControlFlow::Continue(()));
            }

            let location_path = bstr_to_path(change.location())?;
            // On a rename, record the edge (former name → current path) so
            // replay can attribute older commits under the former name
            // forward — including across an incremental splice.
            if let Change::Rewrite {
                source_location, ..
            } = &change
            {
                let source_path = bstr_to_path(source_location)?;
                renames.push((source_path, location_path.clone()));
            }

            // Binary blobs yield no line counts → skip (file excluded).
            let Some(line_stats) = change
                .diff(cache)
                .map_err(diff_err)?
                .line_counts()
                .map_err(diff_err)?
            else {
                return Ok(ControlFlow::Continue(()));
            };
            let churn = u64::from(line_stats.insertions) + u64::from(line_stats.removals);
            touched.push((location_path, churn));

            Ok(ControlFlow::Continue(()))
        })
        .map_err(diff_err)?;

    Ok((renames, touched))
}

/// Commit timestamp in Unix seconds, preferring the walk's pre-decoded
/// value and falling back to decoding the commit. Shared with the
/// single-commit JIT walk in [`super::jit`].
pub(super) fn commit_seconds(
    info: &gix::revision::walk::Info<'_>,
    commit: &gix::Commit<'_>,
) -> Result<i64, Error> {
    match info.commit_time {
        Some(seconds) => Ok(seconds),
        None => commit.time().map(|t| t.seconds).map_err(walk_err),
    }
}

/// Classify the commit message (bug-fix / security-fix / revert).
fn classify_commit(commit: &gix::Commit<'_>) -> Result<Classification, Error> {
    let message = commit.message_raw().map_err(walk_err)?;
    Ok(classify::classify(message))
}
