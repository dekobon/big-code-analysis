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
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::ControlFlow;
use std::path::PathBuf;

use gix::diff::Rewrites;
use gix::diff::blob::Algorithm;
use gix::revision::walk::Sorting;
use gix::traverse::commit::simple::CommitTimeOrder;

use super::identity::ParticipantResolver;
use super::repo::bstr_to_path;
use super::{diff_err, diff_visitor, walk_err};
use crate::vcs::cache::{CommitEvent, WalkInputDigests};
use crate::vcs::classify::{self, Classification};
use crate::vcs::error::Error;
use crate::vcs::identity::{AuthorId, BotFilter};
use crate::vcs::options::{Options, window_boundary};

/// Rename edges (`source → destination`) a commit introduced.
type RenameEdges = Vec<(PathBuf, PathBuf)>;
/// Touched text files as `(location path at the commit, churn)` pairs.
type TouchedFiles = Vec<(PathBuf, u64)>;

/// The per-build inputs a history walk reads, opened once and shared by
/// everything that consumes them.
///
/// The mailmap is read by [`WalkContext::open`] and reachable only through
/// this context because the persistent cache stamps every entry with a
/// digest of the mailmap its events were resolved under
/// ([`mailmap_digest`](Self::mailmap_digest)): the digest and the walk have
/// to read the *same* snapshot, or an entry carries one view over events
/// resolved under another. Until #1409 each read the mailmap for itself
/// and nothing made the two reads agree. An edit landing between them
/// stamped an entry with the pre-edit digest over post-edit events. Worse,
/// `open_mailmap` turns a failed or mid-rewrite read into an empty or
/// partial snapshot rather than an error, so a truncate-then-write save of
/// *unchanged* content could leave an entry stamped with the full mailmap's
/// digest over events resolved under none of it — and since the bytes
/// never changed, no later run fingerprinted differently: the wrong hit was
/// served until `--clear-cache`. With one snapshot behind both consumers, a
/// transient read yields a *consistent* pair — the partial map's digest
/// over events resolved under that same partial map — which the next clean
/// read fails to match and overwrites.
///
/// The fix alone would not retire an entry a release before #1409
/// mis-stamped: its stamp equals the digest the fixed code computes for
/// the same mailmap. The diff-configuration term #1425 added to the
/// fingerprint in the same release does retire it, as it does every entry
/// stamped before it; see `cache::fingerprint`.
///
/// The diff platform is held here for the same reason (issue #1425): the
/// configuration it was built from decides the churn the walk records, so
/// [`diff_config_digest`](Self::diff_config_digest) stamps an entry with
/// it, and the walk takes its line counts through a clone of this one
/// platform rather than a second one it configures for itself.
///
/// The shallow boundary is the third input read here, for
/// [`shallow_boundary_digest`](Self::shallow_boundary_digest), so that
/// deepening a clone that stays shallow — which moves neither `HEAD` nor
/// `is_shallow()` — still retires the entry walked at the old depth.
/// Unlike the mailmap and the diff platform, it is not the only read: gix's
/// `rev_walk` calls `shallow_commits` itself when the traversal starts, to
/// skip the grafted parents, and accepts no shallow set from the caller. Both
/// reads share gix's snapshot, which re-reads `.git/shallow` only when the
/// file's modification time has moved, so they disagree only when a fetch
/// rewrites the file between this `open` and the walk. The entry is then
/// stamped with the earlier boundary over events walked under the later
/// one.
///
/// That window is left open because, unlike the mailmap pair #1409 closed,
/// it heals itself. git replaces `.git/shallow` through a lock file and a
/// rename, and gix fails rather than degrading on a file it cannot parse,
/// so neither read sees a partial boundary. The next run reads the later
/// boundary for both, matches no entry stamped with the earlier one, and
/// walks again. A wrong hit needs the boundary to return to its earlier
/// value, which takes a second fetch making the clone shallower.
pub(super) struct WalkContext<'a> {
    /// The opened repository the walk traverses.
    repo: &'a gix::Repository,
    /// The walk-affecting options (windows, traversal mode, filters).
    pub(super) options: &'a Options,
    /// The repository's effective mailmap, as
    /// [`gix::Repository::open_mailmap`] merged it when this context was
    /// opened.
    mailmap: gix::mailmap::Snapshot,
    /// The blob-diff platform gix configured from the repository's diff
    /// settings — `diff.algorithm`, the `diff.<driver>.*` sections,
    /// `core.bigFileThreshold` — and the attribute stack it resolves
    /// `diff=` bindings through. Its resource cache is empty: the walk
    /// clones it and fills the clone.
    diff: gix::diff::blob::Platform,
    /// The shallow boundary (`.git/shallow`) as gix read it when this
    /// context was opened; `None` for a clone with full history.
    shallow: Option<gix::shallow::Commits>,
    /// Reference time in Unix seconds: `--as-of`, else the wall clock.
    pub(super) now: i64,
}

impl<'a> WalkContext<'a> {
    /// Open the inputs for one build, reading the mailmap and the diff
    /// configuration exactly once.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Diff`] when gix cannot build the diff platform:
    /// the index cannot be loaded (or, in a bare repository, built from
    /// `HEAD`'s tree) or an attributes file cannot be read. The diff keys
    /// are not a source of failure — the repository is opened leniently,
    /// so an unparseable `diff.algorithm`, `diff.<driver>.*` or
    /// `core.bigFileThreshold` falls back to gix's default. Returns
    /// [`Error::OpenRepository`] when `.git/shallow` exists but cannot be
    /// read or parsed.
    ///
    /// [`repo::open`](super::repo::open) only stats that file, so before
    /// this read a corrupt one failed the walk but not a pure cache hit,
    /// which replayed an entry against a boundary nobody could read. Every
    /// path now fails alike, which is the intent: the uncached and
    /// incremental paths walk, and gix's walk reads the same file and
    /// fails on it. Treating the failure as "no boundary" would not keep
    /// the hit either, since that digest matches no entry stamped while
    /// the file was readable, so the build would miss and walk into the
    /// same error; only skipping the read restores the old hit.
    pub(super) fn open(
        repo: &'a gix::Repository,
        options: &'a Options,
        now: i64,
    ) -> Result<Self, Error> {
        Ok(Self {
            repo,
            options,
            mailmap: repo.open_mailmap(),
            diff: repo.diff_resource_cache_for_tree_diff().map_err(diff_err)?,
            shallow: repo
                .shallow_commits()
                .map_err(|e| Error::OpenRepository(e.to_string()))?,
            now,
        })
    }

    /// The digests of the walk inputs the repository supplies outside
    /// [`Options`], for [`cache::fingerprint`](crate::vcs::cache::fingerprint).
    /// All three come from the snapshot, platform and shallow boundary
    /// this context was opened with, never from a second read.
    #[must_use]
    pub(super) fn input_digests(&self) -> WalkInputDigests {
        WalkInputDigests {
            mailmap: self.mailmap_digest(),
            diff_config: self.diff_config_digest(),
            shallow_boundary: self.shallow_boundary_digest(),
        }
    }

    /// A stable 64-bit digest of the shallow boundary — the commits
    /// listed in `.git/shallow`, whose parents the clone does not hold —
    /// or of no boundary for a clone with full history.
    ///
    /// The boundary decides where the walk stops recording, so it is a
    /// walk input like the mailmap and the diff configuration, and like
    /// them it moves without moving `HEAD` or any field of [`Options`].
    /// `is_compatible`'s shallow flag catches only the boundary vanishing
    /// (`git fetch --unshallow`, issue #810): a clone deepened in place by
    /// `--deepen`, a larger `--depth` or an earlier `--shallow-since` is
    /// still shallow, so without this term the entry walked at the old
    /// depth was a pure hit, and the incremental splice re-persisted its
    /// truncated tail under each new head.
    ///
    /// Hashed as a sorted set of raw object-id bytes: gix sorts the list
    /// on read, but a rewrite of the file in another order should not
    /// cost a cold walk whether or not that stays true, and the raw bytes
    /// keep `ObjectId`'s `Hash` impl out of a persisted digest. The walk
    /// reads the file again, and [`WalkContext`] says why a fetch landing
    /// between the two reads costs a cold walk rather than a stale hit.
    #[must_use]
    pub(super) fn shallow_boundary_digest(&self) -> u64 {
        let mut boundary: Vec<&gix::ObjectId> = self
            .shallow
            .as_ref()
            .map(|commits| commits.iter().collect())
            .unwrap_or_default();
        boundary.sort_unstable();
        let mut hasher = DefaultHasher::new();
        boundary.len().hash(&mut hasher);
        for id in boundary {
            hasher.write(id.as_slice());
        }
        hasher.finish()
    }

    /// The oldest commit time the walk admits: the long window's start,
    /// anchored at `now`. Derived here so the walk's cutoff and a splice's
    /// prune cannot be computed from two different reference times.
    #[must_use]
    pub(super) fn long_boundary(&self) -> i64 {
        window_boundary(self.now, self.options.long_window_secs)
    }

    /// A stable 64-bit digest of the mailmap this context resolves
    /// identities through — the merged snapshot
    /// [`gix::Repository::open_mailmap`] produces.
    ///
    /// Author identities are canonicalised at *walk* time and stored in the
    /// cache as digests, so a mailmap edit changes the recorded events
    /// without moving `HEAD` or any field of [`Options`]. Feeding this value
    /// into [`cache::fingerprint`](crate::vcs::cache::fingerprint) is what
    /// stops a stale event log being replayed — or, worse, spliced into and
    /// re-persisted — under a mailmap it was not walked with (issue #1262).
    ///
    /// # Why the parsed snapshot rather than the source bytes
    ///
    /// `open_mailmap` merges up to four sources whose selection is
    /// conditional: the working-tree `.mailmap` (uncommitted edits
    /// included), `HEAD:.mailmap` for a bare repo *when `mailmap.blob` is
    /// unset*, the `mailmap.blob` object, and the `mailmap.file` path.
    /// Re-deriving that list here would be a coverage claim nothing checks
    /// — a source missed (or one added by a future gix release, the
    /// dependency being caret-ranged) silently reproduces this bug for that
    /// source. Digesting the merged snapshot instead delegates the source
    /// list to gix, so it cannot drift.
    ///
    /// `Snapshot::iter` is documented as ordered by `(old_email, old_name)`,
    /// so the digest is deterministic across processes, and `DefaultHasher`
    /// is created with fixed keys for the same cross-process stability
    /// [`cache::fingerprint`](crate::vcs::cache::fingerprint) relies on.
    /// That documented order is why this hashes `entries()` rather than the
    /// `Snapshot` itself, which also derives `Hash`: the derive would rest
    /// on gix's private field layout, where `iter`'s ordering is part of its
    /// contract. The `Vec` is a mailmap's worth of borrowed slices.
    ///
    /// Digesting the parsed entries also means a comment- or
    /// whitespace-only `.mailmap` edit does not cost a needless cold walk:
    /// the resolution it feeds is unchanged, so replaying is correct.
    #[must_use]
    pub(super) fn mailmap_digest(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.mailmap.entries().hash(&mut hasher);
        hasher.finish()
    }

    /// A stable 64-bit digest of the diff configuration this context's
    /// platform diffs under: the resolved `diff.algorithm`, each
    /// `diff.<driver>` section's `algorithm`, `binary` and `textconv`
    /// presence, and `core.bigFileThreshold`.
    ///
    /// The platform decides the line counts the walk records — which
    /// algorithm produces the edit script, and whether a blob is diffed
    /// at all — so a change to any of these rewrites the churn of every
    /// cached commit while moving neither `HEAD` nor any field of
    /// [`Options`] (issue #1425, the class #1262 fixed for the mailmap).
    /// The values are read off the platform gix built from the
    /// repository, so git's precedence rules — repository over global
    /// over system config, environment overrides — stay gix's, not a
    /// list re-derived here.
    ///
    /// # What is projected, and what is left out
    ///
    /// gix's `Algorithm` and `Driver` derive no `Hash`, so the fields are
    /// projected by hand against what `Platform::prepare_diff` and the
    /// `Mode::ToGit` conversion consult:
    ///
    /// - `diff.<driver>.command` is skipped: the platform is built with
    ///   `skip_internal_diff_if_external_is_configured = false`, so the
    ///   internal diff runs whether or not an external one is configured.
    /// - `diff.<driver>.binary` and `diff.<driver>.textconv` are hashed as
    ///   the raw pair: `binary` as written ([`binary_tag`]) and the
    ///   textconv by presence. Under `Mode::ToGit` a textconv never runs,
    ///   but its presence makes the driver's paths text whatever `binary`
    ///   says and, with `binary` unset, turns off the NUL-byte sniffing,
    ///   so today `binary = true` and `binary = false` under a textconv
    ///   are one behaviour and the raw pair cold-walks on an edit between
    ///   them that the platform cannot observe. That fold is gix-diff's
    ///   private reading of the two fields and the dependency is
    ///   caret-ranged: re-deriving it here would turn a change to it into
    ///   a stale hit, the class this digest exists to prevent, while the
    ///   raw pair can only over-invalidate, which costs one cold walk —
    ///   the same reason [`mailmap_digest`](Self::mailmap_digest) hashes
    ///   gix's merged snapshot rather than a source list of its own.
    /// - A section that sets none of `algorithm`, `binary` and `textconv`
    ///   is skipped: it behaves exactly as no driver would, so a `command`-
    ///   or `xfuncname`-only section in a user's global config must not
    ///   cold-walk every repository.
    /// - The `fs` capabilities are skipped: they govern worktree reads,
    ///   and the tree-diff platform has no worktree roots.
    ///
    /// Each gix struct is destructured exhaustively, so a field a gix bump
    /// adds to a driver or to either options struct fails to compile here
    /// rather than silently staying outside the digest.
    ///
    /// `drivers()` is sorted by name, so section order in the config
    /// cannot move the digest.
    ///
    /// Not covered are the `diff` *attribute* bindings that route a path
    /// to a driver or mark it `-diff`. gix resolves them per path from the
    /// attribute files it reads — the index's `.gitattributes` blobs,
    /// `$GIT_DIR/info/attributes`, the global and system files — inside an
    /// attribute stack that keeps those sources private; covering them
    /// would mean re-deriving that source list — the coverage claim #1262
    /// avoided — or resolving every path before the cache lookup. Neither
    /// is done: a binding change, committed or not, is served stale until
    /// `--clear-cache`, as the `git::cached` module docs record.
    #[must_use]
    pub(super) fn diff_config_digest(&self) -> u64 {
        // Exhaustive destructures, for the reason `algorithm_tag` matches
        // exhaustively: a field a gix bump adds fails here rather than
        // staying out of a persisted digest. The ignored fields are the
        // ones the list above leaves out.
        let gix::diff::blob::platform::Options {
            algorithm: platform_algorithm,
            skip_internal_diff_if_external_is_configured: _,
        } = self.diff.options;
        let gix::diff::blob::pipeline::Options {
            large_file_threshold_bytes,
            fs: _,
        } = self.diff.filter.options;
        let mut hasher = DefaultHasher::new();
        algorithm_tag(platform_algorithm).hash(&mut hasher);
        large_file_threshold_bytes.hash(&mut hasher);
        for driver in self.diff.filter.drivers() {
            let gix::diff::blob::Driver {
                name,
                command: _,
                algorithm,
                binary_to_text_command,
                is_binary,
            } = driver;
            let has_textconv = binary_to_text_command.is_some();
            if algorithm.is_none() && is_binary.is_none() && !has_textconv {
                continue;
            }
            name.hash(&mut hasher);
            algorithm_tag(*algorithm).hash(&mut hasher);
            binary_tag(*is_binary).hash(&mut hasher);
            has_textconv.hash(&mut hasher);
        }
        hasher.finish()
    }
}

/// Project an optional diff algorithm onto a small integer for the
/// digest, since gix's `Algorithm` derives no `Hash`. `None` is a driver
/// inheriting the platform's algorithm, which is hashed on its own, so it
/// stays distinct from every explicit choice. Exhaustive, so a variant a
/// gix bump adds fails here rather than hashing as an existing one. A
/// fixed tag rather than `mem::discriminant`, whose hash is not promised
/// stable across toolchains: the fingerprint this feeds is persisted.
fn algorithm_tag(algorithm: Option<Algorithm>) -> u8 {
    match algorithm {
        None => 0,
        Some(Algorithm::Histogram) => 1,
        Some(Algorithm::Myers) => 2,
        Some(Algorithm::MyersMinimal) => 3,
    }
}

/// Project a driver's `binary` setting, as written, onto a small integer
/// for the digest. `None` is a driver that leaves blob detection to the
/// big-file threshold and the NUL sniff, `true` one whose paths are never
/// diffed, `false` one whose paths are always text. A fixed tag rather
/// than the derived `Hash` of an `Option<bool>`, for the reason
/// [`algorithm_tag`] gives. Deliberately not folded with the textconv the
/// way gix reads the pair; [`diff_config_digest`] says why.
///
/// [`diff_config_digest`]: WalkContext::diff_config_digest
fn binary_tag(is_binary: Option<bool>) -> u8 {
    match is_binary {
        None => 0,
        Some(true) => 1,
        Some(false) => 2,
    }
}

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
    walk: &WalkContext<'_>,
    tip: gix::ObjectId,
    stop_oids: &HashSet<gix::ObjectId>,
) -> Result<(Vec<CommitEvent>, Option<gix::ObjectId>), Error> {
    let WalkContext {
        repo, options, now, ..
    } = *walk;
    let long_boundary = walk.long_boundary();

    let bots = if options.exclude_bots {
        Some(BotFilter::new(&options.bot_pattern)?)
    } else {
        None
    };
    let resolver = ParticipantResolver::new(&walk.mailmap, bots.as_ref());

    let rewrites = options.follow_renames.then(Rewrites::default);
    // Take line counts through the context's platform so the churn
    // recorded here was produced under the configuration
    // `diff_config_digest` stamps the entry with. The walk fills and
    // clears a resource cache per commit, so it takes a clone — the
    // drivers, the attribute stack and an empty cache, once per build —
    // rather than a mutable borrow of the context. (`for_each_to_obtain_tree`
    // builds a second platform per commit, whether or not rename tracking
    // uses it, from the same configuration, so the digest covers it too;
    // retiring that rebuild is #1761.)
    let mut cache = walk.diff.clone();

    let mut events = Vec::new();

    let mut platform = repo.rev_walk([tip]);
    if !options.full_history {
        platform = platform.first_parent_only();
    }
    // The cutoff prunes commits older than the long window during
    // traversal; the in-loop guard re-checks inclusively for the
    // boundary commit and any out-of-order timestamps.
    let commits = platform
        .sorting(Sorting::ByCommitTimeCutoff {
            order: CommitTimeOrder::NewestFirst,
            seconds: long_boundary,
        })
        .all()
        .map_err(walk_err)?;

    for info in commits {
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
            walk, &commit, info.id, raw_time, &resolver, rewrites, &mut cache,
        )? {
            events.push(event);
        }

        // The resource cache only grows; clear it each commit to keep
        // memory bounded across a long history (gix docs guidance). Unlike
        // `enumerate_target_files`, the walk frees the buffers too: kept,
        // each grows to the largest blob it has held, and over this
        // repository's history that raised peak RSS by 7-13 MB for a
        // 1-2% shorter walk.
        cache.clear_resource_cache();
    }

    Ok((events, None))
}

/// Turn one walked commit into a [`CommitEvent`], or `None` to drop it:
/// merges (unless opted in) and bot-only commits contribute nothing.
/// Diffs against the first parent and records raw (pre-alias) touched
/// locations plus the rename edges replay needs.
fn process_commit(
    walk: &WalkContext<'_>,
    commit: &gix::Commit<'_>,
    oid: gix::ObjectId,
    raw_time: i64,
    resolver: &ParticipantResolver<'_>,
    rewrites: Option<Rewrites>,
    cache: &mut gix::diff::blob::Platform,
) -> Result<Option<CommitEvent>, Error> {
    let WalkContext { repo, options, .. } = *walk;
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
        // Authors are stored only as their SHA-256 digests — never
        // plaintext (see the cache module's privacy note; the digest is a
        // pseudonym, not anonymization).
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
        .for_each_to_obtain_tree(
            commit_tree,
            diff_visitor(|change| {
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
            }),
        )
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

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
