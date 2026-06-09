// bca: suppress-file(halstead, nargs, exit)
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
    JIT_SCHEMA_VERSION, JIT_SCORE_VERSION, JitCommit, JitDiffReport, JitDiffusion, JitExperience,
    JitFeatures, JitHistory, JitPurpose, JitReport, JitSize, JitSource, score, score_diff_features,
};
use crate::vcs::options::{Options, RiskFormula};

/// One text file the scored commit touched.
#[cfg_attr(test, derive(Debug))]
struct Touched {
    /// The file's path in the scored commit's tree (for diffusion and the
    /// new-name side of a rename).
    path: PathBuf,
    /// The file's path in the parent tree, for looking up its priors;
    /// `None` for a file added by this commit (no prior history).
    parent_path: Option<PathBuf>,
    /// Lines added in this commit.
    added: u64,
    /// Lines deleted in this commit.
    deleted: u64,
    /// Diff hunks in this commit.
    hunks: u32,
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

/// Score an arbitrary unified diff (issue #580). See
/// [`crate::vcs::score_diff`] for the public contract: only the size and
/// diffusion groups are computable, so the result is a partial
/// [`JitDiffReport`] that is **not comparable** to a commit score.
///
/// The diff text is parsed into the same per-file `(added, deleted, hunks,
/// path)` shape [`collect_touched`] produces, then fed through the *same*
/// [`size_features`] / [`diffusion_features`] / scoring path as a commit —
/// no forked metric math.
pub(crate) fn score_diff(diff: &str) -> Result<JitDiffReport, Error> {
    let touched = parse_unified_diff(diff)?;
    let size = size_features(&touched);
    let diffusion = diffusion_features(&touched);
    let (partial_score, contributions) = score_diff_features(size, diffusion);
    Ok(JitDiffReport {
        jit_schema_version: JIT_SCHEMA_VERSION,
        jit_score_version: JIT_SCORE_VERSION,
        source: JitSource::Diff,
        partial_score,
        size,
        diffusion,
        contributions,
    })
}

/// Parse a unified diff into the per-file [`Touched`] shape, counting added
/// (`+`) and removed (`-`) body lines and the hunk (`@@`) count for each
/// file. Binary-file stanzas contribute a touched file with zero line churn
/// (mirroring [`collect_touched`], which skips binary blobs' line counts);
/// renames without a body change still count as a touched file.
///
/// The scored-commit-side path is the new (`+++ b/…`) name so diffusion
/// keys on the post-change tree, matching the commit path; `parent_path` is
/// always `None` (a bare diff has no prior history to look up).
///
/// # Errors
///
/// [`Error::InvalidDiff`] when a `@@` hunk header is malformed, a `+`/`-`
/// body line appears before any hunk header (a structurally broken diff),
/// or the input carries diff content but no `diff --git` file header at all
/// (plain `diff -u` or a combined/merge diff) — so a garbage or unsupported
/// input is a clean client error rather than a silent mis-count or a
/// misleading zero-churn score.
fn parse_unified_diff(diff: &str) -> Result<Vec<Touched>, Error> {
    let mut files: Vec<Touched> = Vec::new();
    // The file currently being accumulated: its parsed new-side path (once a
    // `+++` line is seen) and running counters. `None` until the first file
    // header opens a stanza.
    let mut current: Option<DiffFile> = None;
    // Set when a hunk header or combined-diff marker appears while no stanza
    // is open. If the input never opens a single `diff --git` stanza, this
    // turns an otherwise-silent empty result (plain `diff -u`, a
    // `git diff --cc` combined diff, other non-git diff text) into a clean
    // `InvalidDiff` instead of a misleading zero-churn score.
    let mut saw_orphan_marker = false;

    for raw in diff.lines() {
        // Tolerate CRLF: `lines()` strips `\n`, this strips a trailing `\r`.
        let line = raw.strip_suffix('\r').unwrap_or(raw);

        if let Some(rest) = line.strip_prefix("diff --git ") {
            // A new file stanza begins. Flush the previous file (if any) and
            // seed the path from the `a/… b/…` header as a fallback for a
            // binary or rename-only stanza that carries no `+++` line.
            flush_diff_file(&mut files, current.take());
            current = Some(DiffFile::new(diff_git_new_path(rest)));
            continue;
        }
        let Some(file) = current.as_mut() else {
            // No file stanza is open. `git diff` opens one with a
            // `diff --git` header above any hunk, so a hunk header or a
            // `diff --cc`/`diff --combined` marker here means the input is
            // not a supported git diff (plain `diff -u` carries no
            // `diff --git`; combined/merge diffs use `diff --cc`). Flag it
            // and reject after the walk *only if* no stanza ever opens, so a
            // `git show`/`git log -p` preamble that merely mentions `@@`
            // before its real `diff --git` stanzas is still accepted.
            if line.starts_with("@@")
                || line.starts_with("diff --cc ")
                || line.starts_with("diff --combined ")
            {
                saw_orphan_marker = true;
            }
            // Other preamble (commit message, `index` lines, blank): ignore.
            continue;
        };

        if line.starts_with("@@") {
            // Check the hunk header before the `+`/`-` content branches so a
            // `@@@`/`@@` line is never mistaken for a body line.
            parse_hunk_header(line)?;
            file.saw_hunk = true;
            file.hunks = file.hunks.saturating_add(1);
        } else if !file.saw_hunk && line.starts_with("+++ ") {
            // The `+++ b/<path>` new-side header only ever appears *before*
            // the first `@@` of a file. Once a hunk is open, a `+++ …` line
            // is a real added body line whose content starts with `++ `
            // (e.g. a `++` operator), so it falls through to the `+` branch.
            if let Some(path) = line.strip_prefix("+++ ") {
                file.set_new_path(path);
            }
        } else if !file.saw_hunk && line.starts_with("--- ") {
            // Pre-hunk old-side path: not needed (diffusion keys on the new
            // side), and must not be counted as a deleted body line. After a
            // hunk opens, a `--- …` line is a real deleted body line (e.g. a
            // SQL/Lua/Haskell `--` comment) and falls through to the `-`
            // branch — gating on `!saw_hunk` is what stops that deletion from
            // being silently dropped.
        } else if line.starts_with('+') {
            file.require_open_hunk()?;
            file.added = file.added.saturating_add(1);
        } else if line.starts_with('-') {
            file.require_open_hunk()?;
            file.deleted = file.deleted.saturating_add(1);
        }
        // Everything else is ignored: context lines (' '), `index`,
        // `old/new mode`, `rename from/to`, a `Binary files … differ`
        // marker (the file still flushes as a zero-churn touched entry, like
        // the commit path skips binary blobs), and `\ No newline at end of
        // file`.
    }
    flush_diff_file(&mut files, current.take());
    if files.is_empty() && saw_orphan_marker {
        return Err(Error::InvalidDiff(
            "no `diff --git` file headers found; expected a git-style unified \
             diff (plain `diff -u` and combined/merge diffs are not supported)"
                .to_owned(),
        ));
    }
    Ok(files)
}

/// Accumulator for one file stanza while parsing a unified diff.
struct DiffFile {
    /// New-side path, seeded from the `diff --git` header and refined by the
    /// `+++ b/…` line. `None` only for a `/dev/null` new side (a deletion).
    new_path: Option<PathBuf>,
    /// Whether a `@@` hunk header has been seen yet (body lines before one
    /// are a malformed diff).
    saw_hunk: bool,
    added: u64,
    deleted: u64,
    hunks: u32,
}

impl DiffFile {
    fn new(new_path: Option<PathBuf>) -> Self {
        Self {
            new_path,
            saw_hunk: false,
            added: 0,
            deleted: 0,
            hunks: 0,
        }
    }

    /// Refine the new-side path from a `+++ ` header, dropping the `b/`
    /// prefix. A `/dev/null` new side marks a *deletion*: keep the
    /// `diff --git` header fallback (the `b/<old>` path) so the deleted file
    /// still has a name and counts toward the features.
    fn set_new_path(&mut self, raw: &str) {
        if let Some(path) = unified_path(raw) {
            self.new_path = Some(path);
        }
    }

    /// Confirm a hunk header has opened, so a subsequent body line is
    /// well-formed (a `+`/`-` line before any `@@` is a malformed diff).
    fn require_open_hunk(&self) -> Result<(), Error> {
        if self.saw_hunk {
            Ok(())
        } else {
            Err(Error::InvalidDiff(
                "a +/- line appears before any @@ hunk header".to_owned(),
            ))
        }
    }
}

/// Push a finished [`DiffFile`] onto the touched list as a [`Touched`],
/// preferring the new-side path and falling back to the repo root for a
/// pure deletion (`/dev/null` new side) so the file still counts toward the
/// size and diffusion features.
fn flush_diff_file(files: &mut Vec<Touched>, file: Option<DiffFile>) {
    let Some(file) = file else { return };
    let path = file.new_path.unwrap_or_else(|| PathBuf::from(""));
    files.push(Touched {
        path,
        parent_path: None,
        added: file.added,
        deleted: file.deleted,
        hunks: file.hunks,
    });
}

/// Validate a `@@ -a,b +c,d @@` hunk header: it must carry both a `-` and a
/// `+` range marker. A line starting `@@` without them is a malformed
/// header. A `@@@` header (a combined / merge diff, `git diff --cc`) is
/// rejected outright: its 2-column `+`/`-` body prefixes would be
/// miscounted, and combined diffs are outside this parser's documented
/// `git diff` / `diff -u` scope.
fn parse_hunk_header(line: &str) -> Result<(), Error> {
    if line.starts_with("@@@") {
        return Err(Error::InvalidDiff(
            "combined/merge diffs (@@@ headers) are not supported".to_owned(),
        ));
    }
    // The minimal well-formed header is `@@ -l +l @@`; require both range
    // markers rather than fully parsing the (optional) counts, which the
    // size features do not use.
    if line.contains(" -") && line.contains(" +") {
        Ok(())
    } else {
        Err(Error::InvalidDiff(format!(
            "malformed hunk header: {line:?}"
        )))
    }
}

/// The new-side path from a `diff --git a/<old> b/<new>` header line (the
/// text after `diff --git `), used as a fallback before the `+++` line is
/// seen. Returns the `b/<new>` portion with its `b/` prefix dropped, or
/// `None` if the header does not carry a recognizable `b/` side.
fn diff_git_new_path(rest: &str) -> Option<PathBuf> {
    // `git diff` writes `a/<old> b/<new>`; the new side is the last
    // whitespace-separated token starting with `b/`. Paths with spaces are
    // uncommon in this header form and fall back to the `+++` line.
    rest.rsplit(' ')
        .find_map(|tok| tok.strip_prefix("b/"))
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
}

/// Normalize a `+++`/`---` unified-diff path: strip the optional `a/` or
/// `b/` prefix and a trailing tab-delimited timestamp, and treat
/// `/dev/null` as no path (the absent side of an add / delete).
fn unified_path(raw: &str) -> Option<PathBuf> {
    // `git` appends nothing, but POSIX `diff -u` appends a tab + timestamp;
    // cut at the first tab to be tolerant of both.
    let path = raw.split('\t').next().unwrap_or(raw).trim();
    if path == "/dev/null" || path.is_empty() {
        return None;
    }
    let stripped = path
        .strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path);
    if stripped.is_empty() {
        None
    } else {
        Some(PathBuf::from(stripped))
    }
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

/// Size features: summed churn, file count, and hunk count.
fn size_features(touched: &[Touched]) -> JitSize {
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
#[allow(clippy::cast_precision_loss)]
fn diffusion_features(touched: &[Touched]) -> JitDiffusion {
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

#[cfg(test)]
mod diff_parse_tests {
    //! Unit tests for the unified-diff parser backing `score_diff` (issue
    //! #580). These exercise the `(added, deleted, hunks, path)` extraction
    //! and the partial-report contract directly on diff text — no
    //! repository needed.
    #![allow(clippy::float_cmp)]

    use super::{parse_unified_diff, score_diff};
    use crate::vcs::JitSource;
    use crate::vcs::error::Error;
    use std::path::Path;

    /// Find the parsed touched-file entry for a path, by its new-side name.
    fn touched<'a>(files: &'a [super::Touched], path: &str) -> &'a super::Touched {
        files
            .iter()
            .find(|t| t.path == Path::new(path))
            .unwrap_or_else(|| panic!("no touched entry for {path}; got {:?}", paths(files)))
    }

    fn paths(files: &[super::Touched]) -> Vec<String> {
        files.iter().map(|t| t.path.display().to_string()).collect()
    }

    #[test]
    fn single_file_added_and_deleted_lines() {
        let diff = "\
diff --git a/src/a.rs b/src/a.rs
index 111..222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,3 +1,4 @@
 ctx
-old
+new1
+new2
 tail
";
        let files = parse_unified_diff(diff).expect("parse");
        assert_eq!(files.len(), 1);
        let t = touched(&files, "src/a.rs");
        // expected: two `+` body lines, one `-` body line, one hunk; the
        // ` ctx` / ` tail` context lines and the `---`/`+++`/`index`
        // headers are NOT counted.
        assert_eq!(t.added, 2, "two added lines");
        assert_eq!(t.deleted, 1, "one deleted line");
        assert_eq!(t.hunks, 1, "one hunk");
    }

    #[test]
    fn multi_file_diff_separates_files_and_counts_hunks() {
        let diff = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,1 +1,2 @@
 keep
+added
diff --git a/docs/b.md b/docs/b.md
--- a/docs/b.md
+++ b/docs/b.md
@@ -1,2 +1,1 @@
-removed1
-removed2
+merged
@@ -10,1 +9,1 @@
-x
+y
";
        let files = parse_unified_diff(diff).expect("parse");
        assert_eq!(files.len(), 2, "two distinct files");
        let a = touched(&files, "src/a.rs");
        assert_eq!((a.added, a.deleted, a.hunks), (1, 0, 1));
        let b = touched(&files, "docs/b.md");
        // expected: across two hunks, 2 `+` and 3 `-` body lines.
        assert_eq!((b.added, b.deleted, b.hunks), (2, 3, 2));
    }

    #[test]
    fn rename_uses_new_side_path() {
        let diff = "\
diff --git a/old/name.rs b/new/name.rs
similarity index 95%
rename from old/name.rs
rename to new/name.rs
--- a/old/name.rs
+++ b/new/name.rs
@@ -1,1 +1,1 @@
-a
+b
";
        let files = parse_unified_diff(diff).expect("parse");
        assert_eq!(files.len(), 1);
        // The new-side path is what diffusion keys on (matches the commit
        // path); the `rename from`/`rename to` lines are not body lines.
        let t = touched(&files, "new/name.rs");
        assert_eq!((t.added, t.deleted), (1, 1));
    }

    #[test]
    fn new_file_has_no_dev_null_path() {
        let diff = "\
diff --git a/created.rs b/created.rs
new file mode 100644
index 000..abc
--- /dev/null
+++ b/created.rs
@@ -0,0 +1,2 @@
+line1
+line2
";
        let files = parse_unified_diff(diff).expect("parse");
        assert_eq!(files.len(), 1);
        // `/dev/null` on the old side must not become the path; the new
        // side wins.
        let t = touched(&files, "created.rs");
        assert_eq!((t.added, t.deleted), (2, 0));
    }

    #[test]
    fn deleted_file_dev_null_new_side_falls_back_to_header() {
        let diff = "\
diff --git a/gone.rs b/gone.rs
deleted file mode 100644
--- a/gone.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-line1
-line2
";
        let files = parse_unified_diff(diff).expect("parse");
        assert_eq!(files.len(), 1);
        // The new side is `/dev/null`; the path falls back to the `b/gone.rs`
        // from the `diff --git` header so the deletion still counts.
        let t = touched(&files, "gone.rs");
        assert_eq!((t.added, t.deleted), (0, 2));
    }

    #[test]
    fn binary_file_counts_as_touched_with_zero_lines() {
        let diff = "\
diff --git a/logo.png b/logo.png
index 111..222 100644
Binary files a/logo.png and b/logo.png differ
";
        let files = parse_unified_diff(diff).expect("parse");
        assert_eq!(files.len(), 1, "binary file still counts as touched");
        let t = touched(&files, "logo.png");
        // No line churn or hunks for a binary blob (mirrors the commit
        // path, which skips binary blobs' line counts).
        assert_eq!((t.added, t.deleted, t.hunks), (0, 0, 0));
    }

    #[test]
    fn crlf_line_endings_are_tolerated() {
        let diff = "diff --git a/a.rs b/a.rs\r\n--- a/a.rs\r\n+++ b/a.rs\r\n@@ -1,1 +1,1 @@\r\n-x\r\n+y\r\n";
        let files = parse_unified_diff(diff).expect("parse");
        let t = touched(&files, "a.rs");
        assert_eq!((t.added, t.deleted, t.hunks), (1, 1, 1));
    }

    #[test]
    fn empty_diff_yields_no_files() {
        let files = parse_unified_diff("").expect("parse empty");
        assert!(files.is_empty(), "an empty diff touches no files");
    }

    #[test]
    fn body_line_before_hunk_is_malformed() {
        // A `+`/`-` body line before any `@@` header is structurally broken.
        let diff = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n+orphan\n";
        let err = parse_unified_diff(diff).expect_err("must reject");
        assert!(matches!(err, Error::InvalidDiff(_)), "got {err:?}");
    }

    #[test]
    fn malformed_hunk_header_is_rejected() {
        let diff = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ garbage @@\n";
        let err = parse_unified_diff(diff).expect_err("must reject");
        assert!(matches!(err, Error::InvalidDiff(_)), "got {err:?}");
    }

    #[test]
    fn deleted_line_starting_with_dash_dash_is_counted() {
        // Regression (#580): a deleted line whose CONTENT begins with `-- `
        // (a SQL/Lua/Haskell/Ada comment) renders, under git's single-char
        // `-` prefix, as a body line literally beginning `--- `. The old
        // ungated `starts_with("--- ")` header branch SILENTLY DROPPED it.
        // Gating the header on `!saw_hunk` makes the post-hunk `--- …` line
        // count as a deletion.
        let diff = "\
diff --git a/q.sql b/q.sql
--- a/q.sql
+++ b/q.sql
@@ -1,2 +1,1 @@
 SELECT 1;
--- this is a sql comment
+SELECT 2;
";
        let files = parse_unified_diff(diff).expect("parse");
        assert_eq!(files.len(), 1);
        let t = touched(&files, "q.sql");
        // expected: one real `+` line and one `-` line (the `-- comment`);
        // the pre-fix parser dropped the deletion entirely (deleted == 0).
        assert_eq!(t.added, 1, "one added line");
        assert_eq!(
            t.deleted, 1,
            "the `-- sql comment` deletion must be counted, not dropped"
        );
        assert_eq!(t.hunks, 1);
    }

    #[test]
    fn added_line_starting_with_plus_plus_keeps_path_and_counts() {
        // Regression (#580): an added line whose CONTENT begins with `++ `
        // renders as a body line literally beginning `+++ `. The old
        // ungated `strip_prefix("+++ ")` header branch REWROTE the file path
        // to the line's content AND dropped the addition. Gating the header
        // on `!saw_hunk` keeps the path and counts the line.
        let diff = "\
diff --git a/m.cpp b/m.cpp
--- a/m.cpp
+++ b/m.cpp
@@ -1,1 +1,2 @@
 int x = 0;
+++ foo bar baz
";
        let files = parse_unified_diff(diff).expect("parse");
        assert_eq!(files.len(), 1);
        // The path must stay the real `m.cpp` from the header, NOT be
        // corrupted to `foo bar baz` by the misread body line.
        let t = touched(&files, "m.cpp");
        assert_eq!(t.added, 1, "the `++ …` line must be counted as added");
        assert_eq!(t.deleted, 0);
        assert_eq!(
            paths(&files),
            vec!["m.cpp".to_owned()],
            "the `+++ …` body line must not rewrite the file path"
        );
    }

    #[test]
    fn combined_merge_diff_is_rejected() {
        // A combined / merge diff (`git diff --cc`) uses `@@@` headers and
        // 2-column +/- prefixes that this parser would miscount. Reject it
        // cleanly as a malformed diff instead of silently mis-scoring.
        // Use a `diff --git` header so the stanza opens and the `@@@`
        // header reaches `parse_hunk_header`; a real `git diff --cc`
        // preamble varies but the `@@@` hunk header is the invariant marker.
        let diff = "\
diff --git a/m.rs b/m.rs
--- a/m.rs
+++ b/m.rs
@@@ -1,1 -1,1 +1,1 @@@
- a
 -b
++c
";
        let err = parse_unified_diff(diff).expect_err("combined diff must be rejected");
        assert!(matches!(err, Error::InvalidDiff(_)), "got {err:?}");
    }

    #[test]
    fn plain_diff_u_without_git_header_is_rejected() {
        // POSIX `diff -u a.c b.c` output carries `---`/`+++`/`@@` but no
        // `diff --git` header, so no stanza ever opens. Without the
        // orphan-marker guard this parses to zero files and scores a
        // misleading 0.0; it must be rejected as an unsupported diff so a
        // `--fail-over` gate cannot silently pass a risky change.
        let diff = "\
--- a.c\t2026-01-01 00:00:00
+++ b.c\t2026-01-02 00:00:00
@@ -1,2 +1,2 @@
 keep
-old
+new
";
        let err = parse_unified_diff(diff).expect_err("plain diff -u must be rejected");
        assert!(matches!(err, Error::InvalidDiff(_)), "got {err:?}");
    }

    #[test]
    fn combined_cc_diff_with_real_header_is_rejected() {
        // A real `git diff --cc` combined diff opens with a `diff --cc`
        // file header (not `diff --git`), so the parser never opens a
        // stanza and the `@@@` rejection in `parse_hunk_header` is never
        // reached. The `diff --cc` orphan marker is what rejects it here,
        // matching the documented "combined/merge diffs not supported".
        let diff = "\
diff --cc describe.c
index abc,def..ghi
--- a/describe.c
+++ b/describe.c
@@@ -1,1 -1,1 +1,1 @@@
- a
 -b
++c
";
        let err = parse_unified_diff(diff).expect_err("git diff --cc must be rejected");
        assert!(matches!(err, Error::InvalidDiff(_)), "got {err:?}");
    }

    #[test]
    fn show_style_preamble_mentioning_at_at_still_parses() {
        // Guard against a false-positive: a `git show` / commit-message
        // preamble that merely contains a line starting with `@@` *before*
        // a real `diff --git` stanza must still parse (the stanza opens, so
        // the orphan marker is moot once files are non-empty).
        let diff = "\
commit deadbeef
Author: A B <a@b.c>

    Refactor the @@ dispatch table

diff --git a/x.rs b/x.rs
--- a/x.rs
+++ b/x.rs
@@ -1,1 +1,2 @@
 keep
+added
";
        let files = parse_unified_diff(diff).expect("show-style preamble parses");
        assert_eq!(files.len(), 1, "one real file stanza");
        assert_eq!(files[0].added, 1);
    }

    #[test]
    fn score_diff_marks_unavailable_groups_and_is_partial() {
        // A two-subsystem diff: size + diffusion are present; the report
        // type itself has NO history / experience / purpose fields, so an
        // absent group can never be read as a real zero (the #580 trap).
        let diff = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,1 +1,3 @@
 keep
+one
+two
diff --git a/docs/b.md b/docs/b.md
--- a/docs/b.md
+++ b/docs/b.md
@@ -1,1 +1,2 @@
 title
+body
";
        let report = score_diff(diff).expect("score diff");
        assert_eq!(report.source, JitSource::Diff);
        assert_eq!(report.size.files_touched, 2);
        assert_eq!(report.size.lines_added, 3);
        assert_eq!(report.diffusion.subsystems, 2, "src + docs");
        // The partial score is exactly size + diffusion contributions (the
        // unavailable groups contribute nothing because they are absent).
        let expected = report.contributions.size + report.contributions.diffusion;
        assert!(
            (report.partial_score - expected).abs() < 1e-12,
            "partial score {} != size+diffusion {expected}",
            report.partial_score
        );
        assert!(report.partial_score > 0.0);

        // The serialized JSON must NOT carry history / experience / purpose
        // keys at all — proving "unavailable" is distinct from "zero".
        let json = serde_json::to_value(&report).expect("serialize");
        let obj = json.as_object().expect("object");
        assert_eq!(obj["source"], "diff");
        for absent in ["history", "experience", "purpose", "commit", "score"] {
            assert!(
                !obj.contains_key(absent),
                "diff report must not carry `{absent}` (would be misread as a real value)"
            );
        }
        assert!(obj.contains_key("partial_score"));
    }
}
