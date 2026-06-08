// bca: suppress-file(halstead, nargs, exit, nom)
// File-level halstead/nargs/exit/nom are many-fn aggregation artifacts
// (the gix `?`-heavy open/resolve plumbing, the blame→bucket→finalize
// pipeline, and the many short iterator closures it counts as functions),
// not per-function logic complexity (cognitive/cyclomatic stay enforced)
// — mirrors the sibling `git/` backend files and `vcs_command.rs`.

//! Per-function change-history attribution via `git blame` (issue #329).
//!
//! Where the file-level walk in [`super::history`] diffs each commit
//! against its first parent and folds churn into one accumulator per
//! file, this path runs a single whole-file blame per file and buckets
//! the resulting line→commit attributions into the function line spans
//! the AST walk produced.
//!
//! # Semantics (and how they differ from file-level)
//!
//! Blame reports, for every line *surviving at the target ref*, the
//! commit that last touched it. So a function's per-blame metrics are a
//! **current-snapshot** view, not the full historical churn the
//! file-level walk computes:
//!
//! - `commits_long` / `commits_recent` — distinct commits whose surviving
//!   lines fall inside the function span, intersected with the window.
//! - `churn_long` / `churn_recent` — **surviving-line counts**, *not* the
//!   `added + deleted` line churn the file-level walk reports. A line
//!   rewritten ten times in the window counts once (its last touch); a
//!   line added then deleted within the window does not survive and is
//!   not counted at all. The two churn numbers are therefore not
//!   comparable across the file and function levels by construction.
//! - `authors_*` / `ownership_top_share` — credited per touching commit
//!   (one edit per commit), matching the file-level convention, so
//!   ownership is by commit count rather than surviving-line share.
//! - `age_days` / `last_modified_days` — derived from the in-window
//!   touching commits' timestamps, capped at the long window exactly as
//!   the file-level walk caps them.
//! - `risk_score` — the same weighted formula
//!   ([`crate::vcs::score::weighted`] via [`Accumulator::finalize`]) over
//!   the per-function signals, so file and function scores are directly
//!   rank-comparable.
//!
//! Lines whose last touch predates the long window contribute to the
//! span's `sloc` (the size tie-breaker) but to none of the windowed
//! counts — a function nobody has changed recently reports zero commits.
//!
//! # Documented limitations
//!
//! - **Renames / moves.** With `follow_renames` on (the default), blame
//!   follows a line across a file rename, so edits made under a former
//!   path still attribute to the surviving line. A line *moved between
//!   functions* within a file is attributed to whichever function spans
//!   its current position, not its origin.
//! - **Function splits.** When one function is split into two, both
//!   inherit the blame of the lines they kept; there is no notion of the
//!   pre-split function. Best-effort by line position only.
//! - **Deletion + recreation.** A function deleted and later recreated
//!   over the same line range is one identity to blame: its lines
//!   attribute to the recreating commit(s), with no memory of the prior
//!   incarnation.
//! - **Pathologically repetitive content (issue #579).** `blame_file`
//!   can intermittently surface a spurious "object could not be found" /
//!   "iterator over a tree" error on a file built almost entirely from
//!   identical lines over a deep history (a stress-test artifact, not
//!   real source). The failure is *non-deterministic on identical input*
//!   — `Ok` one run, `Err` the next — because the root cause is not in
//!   `gix-blame` but one layer below it: a lock-free `gix-odb`
//!   pack-refresh race (gitoxide discussion #1412) in which a thread
//!   momentarily fails to observe a freshly-loaded pack index and reports
//!   an object missing that actually exists. Whole-repo analysis blames
//!   from a worker pool sharing one [`gix::ThreadSafeRepository`] ODB, so
//!   it is exactly the contended case that provokes the race. Two defences
//!   apply: [`per_function`](PerFunctionBlame::per_function) retries each
//!   object lookup that can hit this miss — the whole-file blame and the
//!   post-blame commit resolution — up to [`MAX_BLAME_ATTEMPTS`] times
//!   (each retry re-reads the ODB index snapshot, so a momentary miss
//!   becomes a hit), and if every attempt still fails the front end skips
//!   that one file's per-function blocks while keeping its file-level
//!   block, so an unblameable file never aborts the walk. The defect is
//!   still present in the latest upstream release as of 2026-06-08 (`gix`
//!   0.84 / `gix-blame` 0.14; this crate pins 0.83 / 0.13), so bumping
//!   does not fix it; real code does not trigger it (verified on this
//!   repository's own 200-plus-commit files).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bstr::BString;
use gix::ObjectId;

use super::identity::ParticipantResolver;
use super::repo;
use crate::vcs::classify::{self, Classification};
use crate::vcs::error::Error;
use crate::vcs::identity::{AuthorId, BotFilter};
use crate::vcs::options::Options;
use crate::vcs::stats::{Accumulator, ChangeRecord, Stats};

/// A function's 1-based, inclusive line span at the target ref, as taken
/// from the AST `FuncSpace`. A newtype rather than a bare `(u32, u32)`
/// so the inclusive-range contract is explicit at every call site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineSpan {
    start: u32,
    end: u32,
}

impl LineSpan {
    /// Build a span from 1-based inclusive `start`/`end` lines. An
    /// inverted range (`end < start`) is normalised to an empty span at
    /// `start`, so a degenerate AST node cannot blame a negative width.
    #[must_use]
    pub fn new(start: u32, end: u32) -> Self {
        Self {
            start,
            end: end.max(start),
        }
    }

    /// Number of source lines the span covers (≥ 1).
    fn line_count(self) -> u64 {
        u64::from(self.end - self.start) + 1
    }

    /// Lines this span shares with the 1-based inclusive `[lo, hi]` run.
    fn overlap(self, lo: u32, hi: u32) -> u64 {
        let start = self.start.max(lo);
        let end = self.end.min(hi);
        if start > end {
            0
        } else {
            u64::from(end - start) + 1
        }
    }
}

/// Maximum number of times [`PerFunctionBlame::per_function`] issues a
/// retryable ODB object lookup (the whole-file blame, and each post-blame
/// commit resolution) before giving up to the front end's graceful
/// degradation. One initial attempt plus two retries: the transient
/// `gix-odb` pack-refresh race it guards against (issue #579) clears as
/// soon as the racing index load is observed, so a small bound suffices
/// and a deterministic failure (e.g. the path is absent at the ref) is
/// never retried at all — see [`is_transient_blame_miss`].
const MAX_BLAME_ATTEMPTS: u32 = 3;

/// Run `attempt` up to `attempts` times, retrying only while `retry_if`
/// classifies the error as transient; return the first `Ok` or, once the
/// budget is spent (or a non-retryable error is seen), the last `Err`.
///
/// Generic over the value and error types so the retry policy is exercised
/// by pure unit tests without a live repository — the only branching logic
/// lives here, while the production call supplies a real blame closure and
/// the [`is_transient_blame_miss`] predicate. A success on the first
/// attempt costs exactly one call, so the happy path carries no overhead.
fn retry_transient<T, E>(
    attempts: u32,
    mut attempt: impl FnMut() -> Result<T, E>,
    retry_if: impl Fn(&E) -> bool,
) -> Result<T, E> {
    let mut result = attempt();
    // The first call is `attempts == 1`, so retry over the remaining
    // budget. The range is empty for `attempts` 0 or 1 — a single call,
    // with no counter to underflow.
    for _ in 1..attempts {
        if !matches!(&result, Err(e) if retry_if(e)) {
            break;
        }
        result = attempt();
    }
    result
}

/// Whether a [`blame_file`](gix::Repository::blame_file) error is the
/// transient `gix-odb` object-lookup miss tracked in issue #579 — the
/// only blame error [`PerFunctionBlame::per_function`] retries (its
/// sibling [`is_transient_object_miss`] covers the commit lookups).
///
/// Scoped to exactly the two "expected object was reported missing"
/// variants the bug surfaces (`FindExistingObject` / `FindExistingIter`).
/// A genuinely-absent object (`FindObject`), a missing file at the ref,
/// or any decode/diff failure is deterministic and must fail fast rather
/// than waste retries. Matching named variants is compile-time-checked, so
/// a future `gix-blame` that renames or removes them breaks the build here
/// instead of silently degrading.
fn is_transient_blame_miss(err: &gix::repository::blame_file::Error) -> bool {
    matches!(
        err,
        gix::repository::blame_file::Error::Blame(
            gix::blame::Error::FindExistingObject(_) | gix::blame::Error::FindExistingIter(_)
        )
    )
}

/// Whether a [`find_commit`](gix::Repository::find_commit) error is the
/// same transient `gix-odb` object-lookup miss as [`is_transient_blame_miss`].
///
/// The blamed commit ids come straight out of a successful blame, so a
/// `NotFound` here is the race re-surfacing on the post-blame commit
/// resolution (issue #579) rather than a genuinely-absent commit. The
/// lower-level `Find` lookup error and the `Convert` (object-was-not-a-
/// commit) error are deterministic and fail fast.
fn is_transient_object_miss(err: &gix::object::find::existing::with_conversion::Error) -> bool {
    matches!(
        err,
        gix::object::find::existing::with_conversion::Error::Find(
            gix::object::find::existing::Error::NotFound { .. }
        )
    )
}

/// A blame engine bound to one repository, reusable across the per-file
/// metrics walk.
///
/// Holds a [`gix::ThreadSafeRepository`] (so the field can be shared
/// read-only across the CLI's worker threads — a plain
/// [`gix::Repository`] is not `Sync`); each [`per_function`] call clones
/// a thread-local handle. Construct once per invocation with [`open`].
///
/// [`per_function`]: PerFunctionBlame::per_function
/// [`open`]: PerFunctionBlame::open
pub struct PerFunctionBlame {
    repo: gix::ThreadSafeRepository,
    /// The resolved target-ref commit lines are blamed against.
    head: ObjectId,
    /// Canonicalised working-tree root; absolute paths are stripped of
    /// this prefix to recover the repo-relative (git) path.
    workdir: PathBuf,
    options: Options,
    now: i64,
    long_boundary: i64,
    recent_boundary: i64,
    /// Pre-compiled bot filter, `Some` only when bot exclusion is on.
    bots: Option<BotFilter>,
}

// The engine is stored in the CLI's `Config` and shared read-only across
// the per-file worker pool, so it must stay `Send + Sync`. Assert it at
// compile time: if a future `gix` makes `ThreadSafeRepository` non-`Sync`,
// this fails here with a clear message rather than at a distant `Arc` use.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<PerFunctionBlame>();
};

// Hand-written so the (large, non-informative) repository handle is
// elided; front ends embed the engine in a `Debug` config struct.
impl std::fmt::Debug for PerFunctionBlame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PerFunctionBlame")
            .field("head", &self.head)
            .field("workdir", &self.workdir)
            .field("options", &self.options)
            .field("now", &self.now)
            .finish_non_exhaustive()
    }
}

impl PerFunctionBlame {
    /// Open the repository enclosing `root`, resolve the target ref, and
    /// build a reusable blame engine.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotARepository`] when `root` is not inside a git
    /// working tree, [`Error::ResolveRef`] when the configured reference
    /// cannot be peeled to a commit, [`Error::InvalidBotPattern`] for a
    /// bad bot regex, or [`Error::OpenRepository`] for a bare repository
    /// (per-function blame needs a working tree to map files).
    pub fn open(root: &Path, options: Options) -> Result<Self, Error> {
        let repo::OpenRepo { repo, workdir, .. } = repo::open(root)?;
        let workdir = workdir.ok_or_else(|| {
            Error::OpenRepository(
                "per-function VCS metrics require a working tree (bare repository)".to_owned(),
            )
        })?;

        let resolve_err = |e: &dyn std::fmt::Display| Error::ResolveRef {
            reference: options.reference.clone(),
            reason: e.to_string(),
        };
        let tip = repo
            .rev_parse_single(options.reference.as_bytes())
            .map_err(|e| resolve_err(&e))?;
        let head = tip
            .object()
            .map_err(|e| resolve_err(&e))?
            .peel_to_commit()
            .map_err(|e| resolve_err(&e))?
            .id;

        let bots = options
            .exclude_bots
            .then(|| BotFilter::new(&options.bot_pattern))
            .transpose()?;

        let now = options.as_of.unwrap_or_else(super::current_unix_seconds);
        let long_boundary = now - options.long_window_secs;
        let recent_boundary = now - options.recent_window_secs;

        Ok(Self {
            repo: repo.into_sync(),
            head,
            workdir,
            options,
            now,
            long_boundary,
            recent_boundary,
            bots,
        })
    }

    /// The working-tree root the engine resolved (canonicalised).
    #[must_use]
    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// Blame the file at `absolute` once and return one [`Stats`] per
    /// entry in `spans`, in the same order.
    ///
    /// A span with no surviving in-window lines yields a zero-valued
    /// `Stats` (the per-function analogue of a tracked file with no
    /// recent activity), so the caller can attach a block to every
    /// function space uniformly.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Blame`] when the path lies outside the working
    /// tree, is not valid UTF-8, or the blame itself fails (e.g. the
    /// file does not exist at the target ref).
    pub fn per_function(&self, absolute: &Path, spans: &[LineSpan]) -> Result<Vec<Stats>, Error> {
        let repo = self.repo.to_thread_local();
        let relative = self.repo_relative(absolute)?;

        // `blame_file` takes its options by value, so clone the (cheap)
        // built options per attempt. Retry only the transient
        // object-lookup miss (issue #579); any other error fails fast.
        //
        // `gix`'s `blame_file::Error` is large (~264 bytes), so surfacing
        // it through the retry closure trips `result_large_err`. Boxing it
        // would only shave a cold, immediately-stringified error path (one
        // blame per file) at the cost of an allocation and `Box` noise in
        // the predicate, so the allow is the smaller change.
        let options = self.blame_options();
        #[allow(clippy::result_large_err)]
        let outcome = retry_transient(
            MAX_BLAME_ATTEMPTS,
            || repo.blame_file(relative.as_ref(), self.head, options.clone()),
            is_transient_blame_miss,
        )
        .map_err(|e| Error::Blame(e.to_string()))?;

        // Resolve each distinct blamed commit once: timestamp, window
        // membership, participants, and message classification. A commit
        // outside the long window or authored solely by filtered bots
        // resolves to `None` and contributes to no span.
        let mailmap = repo.open_mailmap();
        let resolver = ParticipantResolver::new(&mailmap, self.bots.as_ref());
        let mut meta: HashMap<ObjectId, Option<CommitMeta>> = HashMap::new();
        for entry in &outcome.entries {
            if let std::collections::hash_map::Entry::Vacant(slot) = meta.entry(entry.commit_id) {
                slot.insert(self.resolve_commit(&repo, entry.commit_id, &resolver)?);
            }
        }

        // Blame entries partition the file but are not guaranteed sorted;
        // sort their 1-based inclusive line runs by start once so each span
        // can binary-search to its overlapping runs rather than rescanning
        // every entry (otherwise O(spans × entries) over the whole file).
        let mut runs: Vec<LineRun> = outcome
            .entries
            .iter()
            .map(|entry| {
                // `start_in_blamed_file` is 0-based; convert to the 1-based
                // inclusive range the spans use.
                let lo = entry.start_in_blamed_file + 1;
                LineRun {
                    lo,
                    hi: lo + entry.len.get() - 1,
                    commit_id: entry.commit_id,
                }
            })
            .collect();
        runs.sort_unstable_by_key(|run| run.lo);

        Ok(spans
            .iter()
            .map(|span| self.aggregate_span(*span, &runs, &meta))
            .collect())
    }

    /// Fold the blame runs overlapping one span into a [`Stats`].
    fn aggregate_span(
        &self,
        span: LineSpan,
        runs: &[LineRun],
        meta: &HashMap<ObjectId, Option<CommitMeta>>,
    ) -> Stats {
        // Surviving in-window lines this span owes to each commit, paired
        // with that commit's resolved metadata. Built first so each commit
        // is recorded once with its total contribution, keeping the author
        // edit-credit at one per commit. Storing the metadata borrow here
        // (rather than re-looking it up at record time) means the in-window
        // filter happens exactly once per run. Most functions touch only a
        // handful of commits, so a small pre-allocation avoids the first
        // rehash without over-reserving.
        let mut surviving: HashMap<ObjectId, (&CommitMeta, u64)> = HashMap::with_capacity(8);
        // Runs are disjoint and sorted by start, so their ends are also
        // ascending: skip every run ending before the span, then stop at
        // the first run starting past it.
        let first = runs.partition_point(|run| run.hi < span.start);
        for run in &runs[first..] {
            if run.lo > span.end {
                break;
            }
            // Only commits that resolved to in-window, non-bot metadata
            // contribute; everything else (pre-window, bot-only) is skipped.
            if let Some(Some(commit)) = meta.get(&run.commit_id) {
                let overlap = span.overlap(run.lo, run.hi);
                surviving
                    .entry(run.commit_id)
                    .and_modify(|entry| entry.1 += overlap)
                    .or_insert((commit, overlap));
            }
        }

        let mut acc = Accumulator::new(span.line_count());
        for (commit, churn) in surviving.into_values() {
            acc.record(&ChangeRecord {
                churn,
                commit_time: commit.commit_time,
                in_recent: commit.commit_time >= self.recent_boundary,
                class: commit.class,
                authors: &commit.authors,
                // Change/co-change entropy are file-level history-walk
                // signals; the per-function block is a current-blame
                // snapshot (see the vcs.md caveat) with no per-commit
                // cross-file distribution to draw on, so its entropy
                // fields stay zero rather than reporting a misleading value.
                change_entropy: 0.0,
            });
        }
        acc.finalize(self.now, &self.options, 0.0, 0.0)
    }

    /// Resolve one commit's metadata, returning `None` when it is older
    /// than the long window or authored solely by filtered bots.
    fn resolve_commit(
        &self,
        repo: &gix::Repository,
        commit_id: ObjectId,
        resolver: &ParticipantResolver<'_>,
    ) -> Result<Option<CommitMeta>, Error> {
        // The same transient `gix-odb` object-lookup race that can trip
        // the blame itself (issue #579) can re-surface on this post-blame
        // commit lookup against the shared ODB, so retry it identically;
        // only a `NotFound` miss is retried (see `is_transient_object_miss`).
        let commit = retry_transient(
            MAX_BLAME_ATTEMPTS,
            || repo.find_commit(commit_id),
            is_transient_object_miss,
        )
        .map_err(|e| Error::Blame(format!("looking up blamed commit {commit_id}: {e}")))?;
        // Clamp future-dated commits (clock skew) to `now`, matching the
        // file-level walk.
        let commit_time = commit
            .time()
            .map_err(|e| Error::Blame(format!("decoding commit time: {e}")))?
            .seconds
            .min(self.now);
        if commit_time < self.long_boundary {
            return Ok(None);
        }
        let authors = resolver.participants(&commit)?;
        if authors.is_empty() {
            return Ok(None);
        }
        let message = commit
            .message_raw()
            .map_err(|e| Error::Blame(format!("decoding commit message: {e}")))?;
        Ok(Some(CommitMeta {
            commit_time,
            class: classify::classify(message),
            authors,
        }))
    }

    /// Strip the working-tree prefix and render the repo-relative path as
    /// forward-slash git bytes, erroring rather than lossily mangling a
    /// non-UTF-8 identifier path (per the path rules in AGENTS.md).
    fn repo_relative(&self, absolute: &Path) -> Result<BString, Error> {
        let canonical = absolute
            .canonicalize()
            .unwrap_or_else(|_| absolute.to_path_buf());
        let relative = canonical.strip_prefix(&self.workdir).map_err(|_| {
            Error::Blame(format!(
                "{} is outside the repository working tree",
                absolute.display()
            ))
        })?;
        let text = relative.to_str().ok_or_else(|| {
            Error::Blame(format!("path {} is not valid UTF-8", relative.display()))
        })?;
        Ok(BString::from(text.replace(std::path::MAIN_SEPARATOR, "/")))
    }

    /// Blame the whole file, following renames per the walk options. No
    /// `since` cutoff: the per-commit timestamp filter in
    /// [`resolve_commit`](Self::resolve_commit) applies the window, so a
    /// pre-window line is dropped rather than mis-attributed to a blame
    /// boundary commit.
    fn blame_options(&self) -> gix::repository::blame_file::Options {
        gix::repository::blame_file::Options {
            diff_algorithm: None,
            ranges: gix::blame::BlameRanges::WholeFile,
            since: None,
            rewrites: self
                .options
                .follow_renames
                .then(gix::diff::Rewrites::default),
        }
    }
}

/// One blamed commit's metadata, resolved once and shared across every
/// span that the commit touches.
struct CommitMeta {
    commit_time: i64,
    class: Classification,
    authors: Vec<AuthorId>,
}

/// One blame hunk as a 1-based inclusive line range plus its commit.
/// Pre-sorted by `lo` so spans bucket their lines with a binary search.
struct LineRun {
    lo: u32,
    hi: u32,
    commit_id: ObjectId,
}

#[cfg(test)]
#[path = "blame_tests.rs"]
mod tests;
