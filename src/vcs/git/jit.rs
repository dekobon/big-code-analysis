// bca: suppress-file(halstead, nargs, nexits)
// File-level halstead/nargs/exit are many-fn aggregation artifacts (the
// gix open/resolve/diff plumbing with many `?` error maps), not
// per-function logic complexity (cognitive/cyclomatic stay enforced) —
// mirrors the sibling `git/` backend files.

//! The `vcs-git` backend for just-in-time (commit-level) risk scoring
//! (issue #331).
//!
//! [`score_commit`] resolves a revision to one commit, diffs it against
//! its first parent for the size/diffusion features, classifies its
//! message, then measures the touched files' priors and the author's
//! experience from the history *before* it. The priors reuse the
//! file-level [`build_history_index`](crate::vcs::build_history_index)
//! walk (rooted at the parent) so the JIT score and the file-level
//! `risk_score` stay computed by one code path; the experience walk is a
//! separate, cheap author-only pass (no diffs).

use std::collections::HashSet;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use gix::ObjectId;
use gix::diff::Rewrites;
use gix::revision::walk::Sorting;
use gix::traverse::commit::simple::CommitTimeOrder;

use super::repo::bstr_to_path;
use super::{current_unix_seconds, diff_err, history, walk_err};
use crate::vcs::classify;
use crate::vcs::entropy::shannon_entropy;
use crate::vcs::error::Error;
use crate::vcs::identity::AuthorId;
use crate::vcs::jit::{
    JIT_SCHEMA_VERSION, JIT_SCORE_VERSION, JitCommit, JitDiffusion, JitExperience, JitFeatures,
    JitHistory, JitPurpose, JitReport, JitSize, JitSource, score,
};
use crate::vcs::options::{Options, RiskFormula};

/// One text file the scored commit touched. Shared with the
/// [`diff_parse`](super::diff_parse) sibling, which builds the same shape
/// from raw diff text — hence the `pub(super)` surface.
#[cfg_attr(test, derive(Debug))]
pub(super) struct Touched {
    /// The file's path in the scored commit's tree (for diffusion and the
    /// new-name side of a rename).
    pub(super) path: PathBuf,
    /// The file's path in the parent tree, for looking up its priors;
    /// `None` for a file added by this commit (no prior history).
    pub(super) parent_path: Option<PathBuf>,
    /// Lines added in this commit.
    pub(super) added: u64,
    /// Lines deleted in this commit.
    pub(super) deleted: u64,
    /// Diff hunks in this commit.
    pub(super) hunks: u32,
}

impl Touched {
    /// Total churn (added + deleted) — the entropy weight and size term.
    fn churn(&self) -> u64 {
        self.added + self.deleted
    }
}

/// Score the commit `spec` resolves to. See
/// [`crate::vcs::score_commit`] for the public contract.
pub(crate) fn score_commit(root: &Path, spec: &str, options: &Options) -> Result<JitReport, Error> {
    let repo = super::repo::open(root)?.repo;
    let commit = super::repo::resolve_commit(&repo, spec)?;

    let now = options.as_of.unwrap_or_else(current_unix_seconds);
    let commit_time = commit.time().map_err(walk_err)?.seconds.min(now);

    let (parent_tree, prior_base, parent_count) = parent_context(&repo, &commit)?;
    let commit_tree = commit.tree().map_err(walk_err)?;

    let ctx = CommitContext {
        commit_time,
        prior_base,
    };
    let (features, purpose) = compute_features(
        root,
        &repo,
        &commit,
        &parent_tree,
        &commit_tree,
        &ctx,
        options,
    )?;
    let (total, contributions) = score(&features, purpose);

    Ok(JitReport {
        jit_schema_version: JIT_SCHEMA_VERSION,
        jit_score_version: JIT_SCORE_VERSION,
        source: JitSource::Commit,
        long_window_days: options.long_window_days(),
        recent_window_days: options.recent_window_days(),
        score: total,
        commit: JitCommit {
            id: commit.id.to_hex().to_string(),
            parent_count,
            is_merge: parent_count > 1,
            purpose,
        },
        features,
        contributions,
    })
}

/// Per-commit timing facts shared by the feature computations, bundled so
/// the helper signatures stay small.
struct CommitContext {
    /// The scored commit's timestamp, clamped for clock skew. Anchors
    /// *both* the file-prior windows and the author-experience windows,
    /// so "prior" always means "before this commit" — independent of the
    /// wall clock, which matters when scoring an old commit or replaying
    /// with `--as-of`.
    commit_time: i64,
    /// First-parent id for the prior / experience walks; `None` for a
    /// root commit or a shallow-clone boundary.
    prior_base: Option<ObjectId>,
}

/// The parent tree to diff against, the first-parent id for the prior /
/// experience walks, and the parent count (for the merge / root flags).
///
/// A root commit (no parent) or a shallow-clone boundary (parent object
/// absent) diffs against the empty tree and has no measurable priors or
/// experience, so its `prior_base` is `None`.
fn parent_context<'repo>(
    repo: &'repo gix::Repository,
    commit: &gix::Commit<'repo>,
) -> Result<(gix::Tree<'repo>, Option<ObjectId>, u32), Error> {
    // Only the first parent (diff base) and the total count (merge / root
    // flags) are needed, so iterate the ids rather than collecting them.
    let mut ids = commit.parent_ids();
    let first_parent = ids.next().map(gix::Id::detach);
    let parent_count =
        u32::try_from(usize::from(first_parent.is_some()) + ids.count()).unwrap_or(u32::MAX);
    let (parent_tree, prior_base) = match first_parent {
        Some(pid) => match repo.try_find_object(pid).map_err(walk_err)? {
            Some(object) => (
                object
                    .peel_to_commit()
                    .map_err(walk_err)?
                    .tree()
                    .map_err(walk_err)?,
                Some(pid),
            ),
            None => (repo.empty_tree(), None),
        },
        None => (repo.empty_tree(), None),
    };
    Ok((parent_tree, prior_base, parent_count))
}

/// Compute the full feature vector and the message-purpose classification
/// for one commit: diff-derived size / diffusion, the touched-file priors,
/// the author's experience, and the bug-fix / security-fix / revert flags.
#[allow(clippy::too_many_arguments)]
fn compute_features(
    root: &Path,
    repo: &gix::Repository,
    commit: &gix::Commit<'_>,
    parent_tree: &gix::Tree<'_>,
    commit_tree: &gix::Tree<'_>,
    ctx: &CommitContext,
    options: &Options,
) -> Result<(JitFeatures, JitPurpose), Error> {
    let touched = collect_touched(repo, parent_tree, commit_tree, options)?;
    let size = size_features(&touched);
    let diffusion = diffusion_features(&touched);
    let history = history_features(root, &touched, ctx.prior_base, ctx.commit_time, options)?;

    let mailmap = repo.open_mailmap();
    let target = resolve_author(commit, &mailmap)?;
    let experience = match ctx.prior_base {
        // Anchored on the commit's own time (not the wall clock) so the
        // experience windows match the file-prior windows above.
        Some(pid) => experience_features(repo, pid, &target, &mailmap, options, ctx.commit_time)?,
        None => JitExperience::default(),
    };

    let class = classify::classify(commit.message_raw().map_err(walk_err)?);
    let purpose = JitPurpose {
        is_fix: class.bug_fix,
        is_security_fix: class.security_fix,
        is_revert: class.revert,
    };

    Ok((
        JitFeatures {
            size,
            diffusion,
            history,
            experience,
        },
        purpose,
    ))
}

/// Diff `parent_tree → commit_tree` and collect every touched text file's
/// path, parent-side path, line churn, and hunk count. Binary blobs and
/// symlinks are skipped (no line churn), mirroring the file-level walk.
fn collect_touched(
    repo: &gix::Repository,
    parent_tree: &gix::Tree<'_>,
    commit_tree: &gix::Tree<'_>,
    options: &Options,
) -> Result<Vec<Touched>, Error> {
    use gix::object::tree::diff::Change;

    let rewrites = options.follow_renames.then(Rewrites::default);
    let mut cache = repo.diff_resource_cache_for_tree_diff().map_err(diff_err)?;
    let mut touched = Vec::new();

    parent_tree
        .changes()
        .map_err(diff_err)?
        .options(|opts| {
            opts.track_path();
            opts.track_rewrites(rewrites);
        })
        .for_each_to_obtain_tree(commit_tree, |change| -> Result<ControlFlow<()>, Error> {
            let mode = change.entry_mode();
            if !mode.is_blob() || mode.is_link() {
                return Ok(ControlFlow::Continue(()));
            }
            // The scored-commit-side path (for diffusion / display) and
            // the parent-side path (for the prior lookup) differ only on a
            // rename; an addition has no parent-side path.
            let (path, parent_path) = match &change {
                Change::Addition { location, .. } => (bstr_to_path(location)?, None),
                Change::Deletion { location, .. } | Change::Modification { location, .. } => {
                    let p = bstr_to_path(location)?;
                    (p.clone(), Some(p))
                }
                Change::Rewrite {
                    source_location,
                    location,
                    ..
                } => (
                    bstr_to_path(location)?,
                    Some(bstr_to_path(source_location)?),
                ),
            };
            let Some((added, deleted, hunks)) = blob_line_stats(&change, &mut cache)? else {
                return Ok(ControlFlow::Continue(())); // binary blob
            };
            touched.push(Touched {
                path,
                parent_path,
                added,
                deleted,
                hunks,
            });
            Ok(ControlFlow::Continue(()))
        })
        .map_err(diff_err)?;

    Ok(touched)
}

/// Per-file `(added, deleted, hunks)` for one change, or `None` when the
/// blob is binary (no line counts), so the caller skips it.
fn blob_line_stats(
    change: &gix::object::tree::diff::Change<'_, '_, '_>,
    cache: &mut gix::diff::blob::Platform,
) -> Result<Option<(u64, u64, u32)>, Error> {
    let mut platform = change.diff(cache).map_err(diff_err)?;
    let Some(counts) = platform.line_counts().map_err(diff_err)? else {
        return Ok(None);
    };
    // One callback per hunk; the line data is already summed in
    // `line_counts`, so the closure only tallies. Writing to a counter is
    // infallible, hence the `Infallible` error type.
    let mut hunks: u32 = 0;
    platform
        .lines(|_hunk| {
            hunks = hunks.saturating_add(1);
            Ok::<(), std::convert::Infallible>(())
        })
        .map_err(diff_err)?;
    Ok(Some((
        u64::from(counts.insertions),
        u64::from(counts.removals),
        hunks,
    )))
}

/// Size features: summed churn, file count, and hunk count. Shared with the
/// [`diff_parse`](super::diff_parse) sibling so a diff score and a commit
/// score never fork their metric math.
pub(super) fn size_features(touched: &[Touched]) -> JitSize {
    let mut size = JitSize {
        files_touched: u32::try_from(touched.len()).unwrap_or(u32::MAX),
        ..JitSize::default()
    };
    for t in touched {
        size.lines_added = size.lines_added.saturating_add(t.added);
        size.lines_deleted = size.lines_deleted.saturating_add(t.deleted);
        size.hunks = size.hunks.saturating_add(t.hunks);
    }
    size
}

/// Diffusion features: distinct subsystems (top-level dir) and
/// directories touched, plus the commit's churn-distribution entropy.
///
/// The `churn as f64` casts are exact for any realistic line count and
/// the entropy is ordinal, so the precision lint is allowed here.
///
/// Shared with the [`diff_parse`](super::diff_parse) sibling (see
/// [`size_features`]).
#[allow(clippy::cast_precision_loss)]
pub(super) fn diffusion_features(touched: &[Touched]) -> JitDiffusion {
    // At most one distinct entry per touched file, so size both sets to
    // the file count to avoid rehashing on a wide commit.
    let mut subsystems: HashSet<&Path> = HashSet::with_capacity(touched.len());
    let mut directories: HashSet<&Path> = HashSet::with_capacity(touched.len());
    for t in touched {
        subsystems.insert(top_level(&t.path));
        directories.insert(t.path.parent().unwrap_or_else(|| Path::new("")));
    }
    JitDiffusion {
        subsystems: u32::try_from(subsystems.len()).unwrap_or(u32::MAX),
        directories: u32::try_from(directories.len()).unwrap_or(u32::MAX),
        entropy: shannon_entropy(touched.iter().map(|t| t.churn() as f64)),
    }
}

/// The top-level subsystem of a repo-relative path: its first directory
/// component, or `""` (the repo root) for a file that lives at the root.
fn top_level(path: &Path) -> &Path {
    let mut components = path.components();
    match components.next() {
        // A first component followed by more means `path` is inside a
        // directory, so that first component is the subsystem.
        Some(first) if components.next().is_some() => Path::new(first.as_os_str()),
        // A single-component path is a root-level file: the root
        // subsystem, shared by every other root file.
        _ => Path::new(""),
    }
}

/// History features: the touched files' priors, looked up from a
/// file-level history index rooted at the parent commit (so the priors
/// exclude the commit being scored). A file absent from that index is new
/// and contributes zero priors.
///
/// The prior index is always built with the **weighted** risk formula
/// regardless of `options.risk_formula`, so `file_risk_*` lands on the
/// known weighted scale the JIT formula expects (a percentile re-ranking
/// would be meaningless for a single file's prior).
///
/// The `touched.len() as f64` cast for the mean is exact for any
/// realistic file count, so the precision lint is allowed.
#[allow(clippy::cast_precision_loss)]
fn history_features(
    root: &Path,
    touched: &[Touched],
    prior_base: Option<ObjectId>,
    commit_time: i64,
    options: &Options,
) -> Result<JitHistory, Error> {
    let Some(base) = prior_base else {
        // Root commit or shallow boundary: no prior history exists, so
        // every touched file is new.
        return Ok(JitHistory {
            new_files: u32::try_from(touched.len()).unwrap_or(u32::MAX),
            ..JitHistory::default()
        });
    };

    let prior_options = Options {
        reference: base.to_hex().to_string(),
        as_of: Some(commit_time),
        risk_formula: RiskFormula::Weighted,
        // The JIT prior walk only needs the per-file index; the bus-factor
        // aggregate would be wasted work on every scored commit.
        compute_bus_factor: false,
        ..options.clone()
    };
    let index = crate::vcs::build_history_index(root, &prior_options)?;

    let mut history = JitHistory::default();
    let mut risk_sum = 0.0_f64;
    for t in touched {
        match t.parent_path.as_deref().and_then(|p| index.get(p)) {
            Some(stats) => {
                history.prior_changes = history.prior_changes.saturating_add(stats.commits_long);
                history.prior_distinct_authors =
                    history.prior_distinct_authors.max(stats.authors_long);
                history.prior_bug_fix_commits = history
                    .prior_bug_fix_commits
                    .saturating_add(stats.bug_fix_commits);
                history.prior_security_fix_commits = history
                    .prior_security_fix_commits
                    .saturating_add(stats.security_fix_commits);
                history.file_risk_max = history.file_risk_max.max(stats.risk_score);
                risk_sum += stats.risk_score;
            }
            None => history.new_files = history.new_files.saturating_add(1),
        }
    }
    // New files contribute a zero prior to the mean, so divide by the full
    // touched count (the commit's average prior file risk).
    if !touched.is_empty() {
        history.file_risk_mean = risk_sum / touched.len() as f64;
    }
    Ok(history)
}

/// The scored commit's canonical author identity, resolved through the
/// repository `.mailmap`. Used as the target the experience walk counts.
fn resolve_author(
    commit: &gix::Commit<'_>,
    mailmap: &gix::mailmap::Snapshot,
) -> Result<AuthorId, Error> {
    let author = commit
        .author()
        .map_err(|e| Error::Walk(format!("decoding commit author: {e}")))?;
    let resolved = mailmap.resolve(author);
    Ok(AuthorId::new(&resolved.name, &resolved.email))
}

/// Experience features: the target author's prior commit count in the
/// long and recent windows, walking history from `parent` (so the scored
/// commit itself is excluded). Author-only — no diffs — so it stays cheap
/// even on large repositories. Merges are skipped unless
/// `options.include_merges`, matching the file-level walk.
///
/// `reference_time` is the scored commit's timestamp: the windows and the
/// clock-skew clamp anchor on it (not the wall clock), so the count is
/// "the author's commits in the window ending at this commit" regardless
/// of when the scoring runs.
fn experience_features(
    repo: &gix::Repository,
    parent: ObjectId,
    target: &AuthorId,
    mailmap: &gix::mailmap::Snapshot,
    options: &Options,
    reference_time: i64,
) -> Result<JitExperience, Error> {
    let long_boundary = reference_time - options.long_window_secs;
    let recent_boundary = reference_time - options.recent_window_secs;

    let mut platform = repo.rev_walk([parent]);
    if !options.full_history {
        platform = platform.first_parent_only();
    }
    let walk = platform
        .sorting(Sorting::ByCommitTimeCutoff {
            order: CommitTimeOrder::NewestFirst,
            seconds: long_boundary,
        })
        .all()
        .map_err(walk_err)?;

    let mut experience = JitExperience::default();
    for info in walk {
        let info = info.map_err(walk_err)?;
        let commit = info.object().map_err(walk_err)?;
        let commit_time = history::commit_seconds(&info, &commit)?.min(reference_time);
        if commit_time < long_boundary {
            continue;
        }
        // `take(2)` is enough to tell a merge (≥2 parents) from a normal
        // commit without counting every parent.
        if !options.include_merges && commit.parent_ids().take(2).count() > 1 {
            continue;
        }
        if &resolve_author(&commit, mailmap)? == target {
            experience.author_prior_commits = experience.author_prior_commits.saturating_add(1);
            if commit_time >= recent_boundary {
                experience.author_recent_commits =
                    experience.author_recent_commits.saturating_add(1);
            }
        }
    }
    Ok(experience)
}
