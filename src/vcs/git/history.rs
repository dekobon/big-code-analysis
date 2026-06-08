// bca: suppress-file(halstead, nargs, exit)
// File-level halstead/nargs/exit are many-fn aggregation artifacts (the
// gix rev-walk + per-commit diff plumbing, with many `?` error maps),
// not per-function logic complexity (cognitive/cyclomatic stay enforced).

//! The commit-history walk: rev-walk the target ref within the long
//! window, diff each commit against its first parent, and fold per-file
//! churn / author / classification signals into the accumulators.

use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use gix::diff::Rewrites;
use gix::revision::walk::Sorting;
use gix::traverse::commit::simple::CommitTimeOrder;

use super::identity::ParticipantResolver;
use super::repo::bstr_to_path;
use super::{diff_err, walk_err};
use crate::vcs::classify::{self, Classification};
use crate::vcs::error::Error;
use crate::vcs::identity::{AuthorId, BotFilter};
use crate::vcs::options::Options;
use crate::vcs::stats::{Accumulator, ChangeRecord};

/// Walk history from `tip` and fold every in-window commit into the
/// per-file accumulators that were pre-seeded from the target tree.
///
/// # Errors
///
/// Returns a backend [`Error`] variant if the rev-walk, a tree lookup,
/// or a diff fails.
pub(crate) fn walk_history(
    repo: &gix::Repository,
    tip: gix::ObjectId,
    options: &Options,
    now: i64,
    accumulators: &mut HashMap<PathBuf, Accumulator>,
) -> Result<(), Error> {
    let long_boundary = now - options.long_window_secs;
    let recent_boundary = now - options.recent_window_secs;

    let mailmap = repo.open_mailmap();
    let bots = if options.exclude_bots {
        Some(BotFilter::new(&options.bot_pattern)?)
    } else {
        None
    };
    let resolver = ParticipantResolver::new(&mailmap, bots.as_ref());

    let rewrites = options.follow_renames.then(Rewrites::default);
    let mut cache = repo.diff_resource_cache_for_tree_diff().map_err(diff_err)?;

    // Rename map: a historical path → the more-recent path it became.
    // Resolving the chain yields the file's name at the target ref so
    // edits under former names attribute to the current file.
    //
    // Limitation: the alias is populated lazily as renames are met in
    // the walk's commit-time (newest-first) order, which approximates
    // topological order. If a rename commit carries an *older*
    // committer date than an edit to its pre-rename path (possible after
    // `--date`-rewriting, cherry-pick, or genuine clock skew — normal
    // rebases keep committer dates monotonic), that older-named edit is
    // visited first and attributes under the former name (dropped, or
    // split under `--include-deleted`), undercounting the renamed file.
    // Exact rename-following would require a topological walk, which
    // would forgo the commit-time cutoff that prunes out-of-window
    // history; the approximation is the deliberate v1 trade-off.
    let mut alias: HashMap<PathBuf, PathBuf> = HashMap::new();

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
        let commit = info.object().map_err(walk_err)?;

        // Clamp future-dated commits (clock skew) to `now`.
        let commit_time = commit_seconds(&info, &commit)?.min(now);
        if commit_time < long_boundary {
            continue;
        }

        process_commit(
            repo,
            &commit,
            commit_time,
            recent_boundary,
            options,
            &resolver,
            rewrites,
            &mut cache,
            &mut alias,
            accumulators,
        )?;

        // The resource cache only grows; clear it each commit to keep
        // memory bounded across a long history (gix docs guidance).
        cache.clear_resource_cache();
    }

    Ok(())
}

/// Fold one walked commit into the accumulators: skip merges (unless
/// opted in) and bot-only commits, classify the message, then diff
/// against the first parent. Split out of [`walk_history`] so neither
/// function carries the whole walk's branching.
#[allow(clippy::too_many_arguments)]
fn process_commit(
    repo: &gix::Repository,
    commit: &gix::Commit<'_>,
    commit_time: i64,
    recent_boundary: i64,
    options: &Options,
    resolver: &ParticipantResolver<'_>,
    rewrites: Option<Rewrites>,
    cache: &mut gix::diff::blob::Platform,
    alias: &mut HashMap<PathBuf, PathBuf>,
    accumulators: &mut HashMap<PathBuf, Accumulator>,
) -> Result<(), Error> {
    // Only the first parent (for the diff base) and whether more than
    // one exists (the merge check) are needed, so avoid collecting the
    // parent ids into a `Vec` for every walked commit.
    let mut parent_ids = commit.parent_ids().map(gix::Id::detach);
    let first_parent = parent_ids.next();
    let is_merge = parent_ids.next().is_some();
    if is_merge && !options.include_merges {
        return Ok(());
    }

    // Participants first: a commit authored solely by filtered bots
    // contributes nothing, so skip it before the (costly) diff.
    let authors = resolver.participants(commit)?;
    if authors.is_empty() {
        return Ok(());
    }

    let class = classify_commit(commit)?;
    let in_recent = commit_time >= recent_boundary;

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

    let ctx = CommitContext {
        commit_time,
        in_recent,
        class,
        authors: &authors,
    };
    diff_commit(
        &parent_tree,
        &commit_tree,
        cache,
        rewrites,
        alias,
        options,
        accumulators,
        &ctx,
    )
}

/// Per-commit facts shared by every file the commit touched.
struct CommitContext<'a> {
    commit_time: i64,
    in_recent: bool,
    class: Classification,
    authors: &'a [AuthorId],
}

/// Diff `parent_tree → commit_tree` and fold each changed file into its
/// accumulator.
#[allow(clippy::too_many_arguments)]
fn diff_commit(
    parent_tree: &gix::Tree<'_>,
    commit_tree: &gix::Tree<'_>,
    cache: &mut gix::diff::blob::Platform,
    rewrites: Option<Rewrites>,
    alias: &mut HashMap<PathBuf, PathBuf>,
    options: &Options,
    accumulators: &mut HashMap<PathBuf, Accumulator>,
    ctx: &CommitContext<'_>,
) -> Result<(), Error> {
    use gix::object::tree::diff::Change;

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
            // On a rename, register the alias (former name → current
            // path) so older commits under the former name attribute
            // forward. Resolving after the insert is equivalent: the
            // inserted key is the *source*, which never aliases the
            // current location being resolved here.
            if let Change::Rewrite {
                source_location, ..
            } = &change
            {
                let source_path = bstr_to_path(source_location)?;
                alias.insert(source_path, location_path.clone());
            }
            // Resolve the file's canonical (target-ref) path. Borrows
            // through the alias chain — no allocation on the common
            // no-rename path (the map is empty until a rewrite is seen).
            let canonical = resolve_alias(alias, &location_path);

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

            let accumulator = match accumulators.get_mut(canonical.as_ref()) {
                Some(acc) => acc,
                None if options.include_deleted => accumulators
                    .entry(canonical.into_owned())
                    .or_insert_with(|| Accumulator::new(0)),
                // A file not present at the target ref and not opted in
                // via --include-deleted.
                None => return Ok(ControlFlow::Continue(())),
            };
            accumulator.record(&ChangeRecord {
                churn,
                commit_time: ctx.commit_time,
                in_recent: ctx.in_recent,
                class: ctx.class,
                authors: ctx.authors,
            });

            Ok(ControlFlow::Continue(()))
        })
        .map_err(diff_err)?;

    Ok(())
}

/// Follow the rename-alias chain from a historical path to the path the
/// file carries at the target ref. The depth guard defends against a
/// pathological cycle in malformed history.
fn resolve_alias<'a>(alias: &'a HashMap<PathBuf, PathBuf>, path: &'a Path) -> Cow<'a, Path> {
    let mut current: &'a Path = path;
    for _ in 0..MAX_ALIAS_DEPTH {
        match alias.get(current) {
            Some(next) => current = next.as_path(),
            None => break,
        }
    }
    // Always borrowed: both `path` and the alias values outlive `'a`, so
    // no `PathBuf` is allocated unless a caller later needs ownership.
    Cow::Borrowed(current)
}

/// Upper bound on rename-alias chain length; far beyond any real
/// rename history, so hitting it implies a cycle.
const MAX_ALIAS_DEPTH: usize = 10_000;

/// Commit timestamp in Unix seconds, preferring the walk's pre-decoded
/// value and falling back to decoding the commit.
fn commit_seconds(
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
