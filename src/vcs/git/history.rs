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
use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::ops::ControlFlow;
use std::path::PathBuf;

use gix::bstr::BString;
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
/// platform rather than a second one it configures for itself. Since
/// #1761 it is the *only* platform a build constructs: the walk's rename
/// tracking, the seed enumeration and the walk's line counts each diff
/// through a clone of it, where gix's `for_each_to_obtain_tree` used to
/// build one more per call — loading or `stat`ing the index and probing
/// the global attribute files for every walked commit.
///
/// The `diff` attribute bindings are the third input (issue #1760), and
/// they are read the way the mailmap is: once. [`open`](Self::open)
/// reads the bytes of every attribute file outside the index into an
/// [`AttributeSources`] capture, builds the platform's attribute stack
/// from that capture ([`tree_diff_platform`]), and takes
/// [`diff_attributes_digest`](Self::diff_attributes_digest) over the
/// same capture. The first shape of this term let gix's
/// `attributes_only` read the global files into the platform, let each
/// clone's stack read `info/attributes` on its own first lookup, and
/// then read every file a second time for the digest — so a file
/// replaced between the two reads left the walk under the old rules and
/// the entry stamped with the new digest, which every later run matched:
/// a stale hit served until `--clear-cache`, the #1409 defect with the
/// attribute files in place of the mailmap. Unlike the shallow boundary
/// below, that window did not heal, because the edited file's digest is
/// exactly what the mis-stamped entry carries. With one capture behind
/// both consumers, a transient read yields a consistent pair that the
/// next clean read fails to match.
///
/// The shallow boundary is the fourth input read here, for
/// [`shallow_boundary_digest`](Self::shallow_boundary_digest), so that
/// deepening a clone that stays shallow — which moves neither `HEAD` nor
/// `is_shallow()` — still retires the entry walked at the old depth.
/// Unlike the mailmap, the diff platform and the attribute files, it is
/// not the only read: gix's
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
    /// The blob-diff platform configured from the repository's diff
    /// settings — `diff.algorithm`, the `diff.<driver>.*` sections,
    /// `core.bigFileThreshold` — and the attribute stack it resolves
    /// `diff=` bindings through. Its resource cache is empty: every
    /// consumer clones it and fills the clone.
    diff: gix::diff::blob::Platform,
    /// The digest of the `diff` attribute bindings
    /// ([`diff_attributes_digest`](Self::diff_attributes_digest)),
    /// computed by [`open`](Self::open) over the index the platform's
    /// attribute stack reads its `.gitattributes` blobs through —
    /// `.git/index` when there is one, else (a bare repository) one built
    /// from `HEAD`'s tree — and the captured bytes of the attribute files
    /// outside it, the same capture the stack was built from.
    diff_attributes: u64,
    /// The shallow boundary (`.git/shallow`) as gix read it when this
    /// context was opened; `None` for a clone with full history.
    shallow: Option<gix::shallow::Commits>,
    /// Reference time in Unix seconds: `--as-of`, else the wall clock.
    pub(super) now: i64,
}

impl<'a> WalkContext<'a> {
    /// Open the inputs for one build, reading the mailmap, the diff
    /// configuration and the attribute files exactly once.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Diff`] when the diff platform cannot be built:
    /// the index cannot be loaded (or, in a bare repository, built from
    /// `HEAD`'s tree), `core.attributesFile` cannot be interpolated, or
    /// an attribute file exists but cannot be read — the failures gix's
    /// own platform build raises for the same inputs. The diff keys are
    /// not a source of failure — the repository
    /// is opened leniently, so an unparseable `diff.algorithm`,
    /// `diff.<driver>.*` or `core.bigFileThreshold` falls back to gix's
    /// default — and neither is `core.ignoreCase`, which
    /// [`ignore_case`] reads as unset when it does not parse, whatever
    /// the open's leniency. Returns [`Error::OpenRepository`] when `.git/shallow`
    /// exists but cannot be read or parsed.
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
        let index = repo.index_or_load_from_head_or_empty().map_err(diff_err)?;
        let sources = AttributeSources::read(
            repo,
            &mut gix::path::env::var,
            repo.open_options().permissions.attributes,
        )
        .map_err(diff_err)?;
        let ignore_case = ignore_case(repo);
        let mappings = id_mappings(&index, ignore_case);
        let diff_attributes = diff_attributes_digest(&mappings, &sources, ignore_case);
        let diff = tree_diff_platform(repo, sources, ignore_case, mappings).map_err(diff_err)?;
        Ok(Self {
            repo,
            options,
            mailmap: repo.open_mailmap(),
            diff,
            diff_attributes,
            shallow: repo
                .shallow_commits()
                .map_err(|e| Error::OpenRepository(e.to_string()))?,
            now,
        })
    }

    /// The context's diff platform, for a consumer that diffs outside the
    /// walk — the seed enumeration — to clone and fill.
    #[must_use]
    pub(super) fn diff_platform(&self) -> &gix::diff::blob::Platform {
        &self.diff
    }

    /// The digests of the walk inputs the repository supplies outside
    /// [`Options`], for [`cache::fingerprint`](crate::vcs::cache::fingerprint).
    /// All four come from the snapshot, platform, index and shallow
    /// boundary this context was opened with, never from a second read.
    #[must_use]
    pub(super) fn input_digests(&self) -> WalkInputDigests {
        WalkInputDigests {
            mailmap: self.mailmap_digest(),
            diff_config: self.diff_config_digest(),
            diff_attributes: self.diff_attributes_digest(),
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
    /// to a driver or mark it `-diff`: they are the separate
    /// [`diff_attributes_digest`](Self::diff_attributes_digest) term.
    #[must_use]
    pub(super) fn diff_config_digest(&self) -> u64 {
        diff_config_digest(&self.diff)
    }

    /// A stable 64-bit digest of the `diff` attribute bindings the
    /// platform resolves paths through: which `.gitattributes` blobs the
    /// index supplies, the bytes of every attribute file gix reads from
    /// outside the index, and whether patterns match case-insensitively.
    ///
    /// The bindings route a path to a `diff.<driver>` section or mark it
    /// `-diff`, so they decide the line counts as much as the
    /// configuration does, and gix resolves them for *every* walked
    /// commit from the current index and attribute files — never from the
    /// commit's own tree. A binding change therefore rewrites the churn a
    /// fresh walk would record while moving neither `HEAD` nor any field
    /// of [`Options`] (issue #1760, the class #1262 and #1425 fixed for
    /// the mailmap and the configuration). A *committed* change does move
    /// `HEAD`, but the next run then takes the incremental splice under
    /// the unchanged fingerprint and adopts the tail walked under the old
    /// bindings, so it was served stale either way until `--clear-cache`.
    ///
    /// # Three terms, one hasher
    ///
    /// A lookup's answer is a function of exactly three things: the
    /// ordered patterns of each source the stack consults, the
    /// precedence between those sources, and the case rule the globs
    /// match under. The precedence is gix's and fixed; the other two are
    /// hashed as the inputs they are, in a fixed order:
    ///
    /// 1. The `(path, blob id)` pairs
    ///    [`id_mappings_from_index`](gix::worktree::stack::State::id_mappings_from_index)
    ///    yields — the very list the stack binary-searches when it pushes
    ///    a directory, computed from the context's index and handed to
    ///    the stack as its own ([`id_mappings`]). It covers every
    ///    committed or staged `.gitattributes`, for every path the
    ///    patterns can reach — including paths deleted before `HEAD`,
    ///    which a per-path term over the index cannot see.
    /// 2. The captured bytes of each attribute file gix loads from
    ///    outside the index, in the order [`AttributeSources`] holds
    ///    them: the system file, the user file (`core.attributesFile`,
    ///    else the XDG default) and `$GIT_COMMON_DIR/info/attributes`. Each is hashed as a fixed
    ///    source tag followed by its content, or by an absent marker, so
    ///    a file appearing, vanishing, or changing in any way — a rule
    ///    edit, a rule *reorder*, which changes what wins without
    ///    changing the rule set — moves the digest. The stack is built
    ///    from the same capture, so the bytes hashed are the bytes the
    ///    walk resolves under. Nothing about a file's *path* is hashed:
    ///    `info/attributes` is reached through
    ///    [`common_dir`](gix::Repository::common_dir), whose spelling
    ///    follows the caller's (`-p .` against `-p "$PWD"`, a symlinked
    ///    checkout), and a digest over it would thrash between two
    ///    spellings of one repository.
    /// 3. `core.ignoreCase`, which the stack applies to every glob: a
    ///    flip lets a rule already in a file newly bind a path whose case
    ///    differs from the pattern's, with no file changed.
    ///
    /// The earlier shape of term 2 hashed the set of rules that *won*
    /// the attribute for the index's blobs, resolved through the stack.
    /// A per-winner term is unsound on its own evidence: a reorder of
    /// two overlapping rules leaves the set unchanged while flipping
    /// every path both match; a case flip binds a new path through a rule
    /// already in the set; and the winner's source path carried the
    /// invocation spelling. Only the ordered content of each source is
    /// complete. The content is also cheaper: three `read`s against one
    /// stack lookup per index entry, which on this repository was a
    /// quarter of a millisecond per 1,000 blobs on every cache hit.
    ///
    /// Term 1 hashes the construction order of a sorted list and term 2
    /// a fixed source order, so the digest is deterministic across
    /// processes. The source tags are fixed `u8`s, never
    /// `mem::discriminant`, since the fingerprint is persisted.
    ///
    /// # What is left out
    ///
    /// The working tree's `.gitattributes` is deliberately not read: the
    /// platform is built with `Source::IdMapping`, so an unstaged edit
    /// changes nothing the walk reads, and a digest over the worktree
    /// bytes would cold-walk on an edit the walk cannot observe.
    ///
    /// The capture follows gix's own permission policy:
    /// [`AttributeSources::read`] takes the
    /// [`Attributes`](gix::open::permissions::Attributes) permissions
    /// and withholds the system file and the XDG default exactly as
    /// `attribute_global_paths` does under the same value, so the
    /// stack and the digest see the files gix's build would have, and
    /// no others. [`open`](Self::open) reads that value off the open
    /// itself (`gix::Repository::open_options`), so a context over an
    /// `Options::isolated()` repository — which only the tests build —
    /// withholds what gix's own build withholds, the same as one over
    /// the `Permissions::all()` / `Permissions::secure()` open
    /// `repo::open` produces through `gix::discover`, both of which set
    /// the attribute permissions to `Attributes::all()`. The
    /// installation's file is never loaded by either.
    ///
    /// The residual risk is the list itself. [`AttributeSources`] names
    /// the sources gix 0.90's `attribute_global_paths` and
    /// `Attributes::push_directory` read; a gix release that adds a
    /// source kind would leave it outside both the capture and this
    /// digest, and that fails toward *staleness*: an edit to the new
    /// source would hit. A gix bump must re-check that function against
    /// [`AttributeSources::read`] — which since gix 0.90 asks it
    /// directly, through [`gix::Repository::attribute_global_paths`],
    /// for the user file, and whose parity tests ask gix which paths
    /// it selected.
    #[must_use]
    pub(super) fn diff_attributes_digest(&self) -> u64 {
        self.diff_attributes
    }
}

/// The `(path, blob id)` pairs of every `.gitattributes` the index holds,
/// computed the way gix's `attributes_only` computes the stack's own.
/// Computed once per open and handed both to the stack and to the digest
/// (term 1), so the two cannot read different index loads.
fn id_mappings(index: &gix::index::State, ignore_case: bool) -> Vec<(BString, gix::ObjectId)> {
    gix::worktree::stack::State::AttributesStack(gix::worktree::stack::state::Attributes::default())
        .id_mappings_from_index(index, index.path_backing(), glob_case(ignore_case))
}

/// `core.ignoreCase` as gix's own stack applies it: the value the
/// repository was opened with, where a value that does not parse as a
/// boolean (`ignoreCase = yesplease`) reads as unset under a lenient
/// open — the open every `bca` run is, since `repo::open` keeps
/// `Options::default()`'s `lenient_config`. The snapshot's `boolean`
/// degrades a parse failure to `None` whatever the open's leniency, so
/// it can never answer where gix's open-time read did not. Read once,
/// so the digest's third term and the stack's case rule come from one
/// value. `filesystem_options()` is the same read, but through a
/// method whose error a caller is tempted to propagate; an earlier
/// shape of `open` did, and a strict parse would refuse a repository
/// `git` and gix both open, so the fixture for it stays even though
/// today's gix also applies the open's leniency there.
fn ignore_case(repo: &gix::Repository) -> bool {
    repo.config_snapshot()
        .boolean(gix::config::tree::Core::IGNORE_CASE)
        .unwrap_or(false)
}

/// The glob case rule gix derives from `core.ignoreCase`.
fn glob_case(ignore_case: bool) -> gix::glob::pattern::Case {
    if ignore_case {
        gix::glob::pattern::Case::Fold
    } else {
        gix::glob::pattern::Case::Sensitive
    }
}

/// The digest [`WalkContext::diff_attributes_digest`] documents, over the
/// three inputs [`WalkContext::open`] captured: the id mappings the stack
/// was handed, the attribute-file bytes it was built from, and the case
/// rule it matches under.
fn diff_attributes_digest(
    mappings: &[(BString, gix::ObjectId)],
    sources: &AttributeSources,
    ignore_case: bool,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    mappings.len().hash(&mut hasher);
    for (path, id) in mappings {
        path.hash(&mut hasher);
        hasher.write(id.as_slice());
    }
    for (tag, source) in (0u8..).zip(&sources.files) {
        tag.hash(&mut hasher);
        match &source.bytes {
            Some(bytes) => {
                true.hash(&mut hasher);
                bytes.hash(&mut hasher);
            }
            None => false.hash(&mut hasher),
        }
    }
    u8::from(ignore_case).hash(&mut hasher);
    hasher.finish()
}

/// Whether gix's `add_patterns_file` reads an attribute file that failed
/// to open this way as *absent* rather than failing: a missing path, or
/// a path that names a directory. Mirrors gix-glob 0.29's
/// `read_in_full_ignore_missing` and `io_err_is_dir`
/// (`src/search/pattern.rs`), including the Windows clause: there,
/// opening a directory as a file reports `PermissionDenied`, so gix reads
/// that kind as a directory too — and with it a genuine permission
/// failure, which it cannot tell apart. The capture is now the *only*
/// reader of these files, so what it treats as absent is what the walk
/// resolves without, and this rule has to be gix's exactly or an
/// attributes file a user could not read would fail here where `git`
/// and gix silently skip it.
fn gix_treats_as_missing(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::NotFound
            | std::io::ErrorKind::IsADirectory
            | std::io::ErrorKind::NotADirectory
    ) || (cfg!(windows) && err.kind() == std::io::ErrorKind::PermissionDenied)
}

/// The user attribute file gix selected for `repo`, or the XDG default
/// resolved through `env` when `permissions.git` allows one and the
/// open's own permissions did not.
///
/// gix's selection is [`gix::Repository::attribute_global_paths`]: the
/// installation file (never — every preset turns it off), the system
/// file, then `core.attributesFile` or the XDG default, each present
/// only when selected, every one anchored to
/// [`gix::Repository::current_dir`]. The user file is the entry after
/// the system one, when gix selected a system file, else the first —
/// a positional read, so a user file configured *at* the system
/// location is still the user file. Whether gix selected a system
/// file is decided the way gix decides it: the open's
/// `attributes.system` permission and `GIT_ATTR_NOSYSTEM` read under
/// the open's *environment* permissions, which an isolated open
/// denies — `env`, the test seam, is deliberately not consulted here.
/// Asking gix rather than `trusted_path` is what gets an
/// `:(optional)` spelling right: gix anchors the path before the
/// existence check `optional` adds, where `trusted_path` checks it
/// against the process's directory at read time.
///
/// # Errors
///
/// Fails when `core.attributesFile` is set but cannot be interpolated.
fn user_attributes_path(
    repo: &gix::Repository,
    env: &mut dyn FnMut(&str) -> Option<OsString>,
    permissions: gix::open::permissions::Attributes,
) -> gix::Result<Option<PathBuf>> {
    use gix::attrs::Source;
    let base = repo.current_dir();
    let open_permissions = &repo.open_options().permissions;
    let mut gix_env = |name: &str| {
        open_permissions
            .env
            .git_prefix
            .check_opt(name)
            .and_then(gix::path::env::var)
    };
    let gix_system = open_permissions
        .attributes
        .system
        .then(|| Source::System.storage_location(&mut gix_env))
        .flatten()
        .map(|path| base.join(path));
    let selected = repo.attribute_global_paths()?;
    let user = match (selected.first(), gix_system) {
        (Some(first), Some(system)) if *first == system => selected.get(1),
        (first, _) => first,
    };
    Ok(user.cloned().or_else(|| {
        permissions
            .git
            .then(|| Source::Git.storage_location(env))
            .flatten()
            .map(|path| base.join(path))
    }))
}

/// One attribute file gix loads from outside the index: where it was
/// looked for, and its bytes if it was there.
struct AttributeSource {
    /// The path gix would open, or `None` where gix could not resolve a
    /// location for this source (no `HOME`, no git on `PATH`). Kept for
    /// the stack, which records the source path beside each pattern;
    /// the digest never hashes it.
    path: Option<PathBuf>,
    /// The file's content, or `None` for a file gix reads as absent.
    bytes: Option<Vec<u8>>,
}

/// The one read of every attribute file gix loads from outside the index
/// (issue #1760), in the order gix adds them to its global pattern group,
/// which is the order [`diff_attributes_digest`] hashes them in.
///
/// Both consumers — the platform's attribute stack and the digest — are
/// built from one capture, so the walk can only resolve under the bytes
/// the entry is stamped with. Written out against gix 0.90's
/// `attribute_global_paths` (the system and user files, in that
/// order — never the installation's, which every permission preset
/// turns off) and `Attributes::push_directory`
/// (`info/attributes`), so a source gix adds must be added here;
/// [`WalkContext::diff_attributes_digest`] says why that failure leans
/// toward staleness.
struct AttributeSources {
    /// System, user, `info/attributes`.
    files: [AttributeSource; 3],
}

/// The index entry gix's `push_directory` looks up for the root
/// directory: the exact, case-sensitive spelling its binary search
/// compares against, whatever case rule the mappings were collected
/// under.
const ROOT_ATTRIBUTES: &str = ".gitattributes";

impl AttributeSources {
    /// Read the three sources once, under the permissions gix's own
    /// build applies: the system file only when `permissions.system`,
    /// the XDG default only when `permissions.git`, as
    /// `attribute_global_paths` gates them. `core.attributesFile`
    /// is consulted regardless, as it is there. The installation's
    /// file is never read: every gix preset sets `git_binary: false`,
    /// so gix never loads it either, and resolving its location would
    /// run the `git` binary.
    ///
    /// The user file is the one gix selected ([`user_attributes_path`]):
    /// `core.attributesFile` when set, else the XDG default — resolved
    /// through `env` when `permissions.git` allows it and the open's
    /// own permissions did not. The XDG and system locations come from
    /// [`gix::attrs::Source::storage_location`] with `env`, which
    /// production passes as [`gix::path::env::var`]: the resolver gix's
    /// own `make_source_env` wraps, whose `HOME` falls back to the
    /// platform home directory (`std::env::home_dir`, the passwd entry
    /// on Unix) when the variable is unset. A plain `std::env::var_os`
    /// here read `~/.config/git/attributes` as absent exactly when gix
    /// loaded it. `GIT_ATTR_NOSYSTEM` drops the system file here as it
    /// does there. A location that cannot be resolved, or that the
    /// permissions withhold, is captured as absent.
    ///
    /// Every path is anchored to [`gix::Repository::current_dir`] — the
    /// working directory captured when the repository was opened — as
    /// gix 0.90 anchors its own. A relative `core.attributesFile` or a
    /// relative repository path otherwise resolved against whatever the
    /// process's working directory was at *read* time, so a `chdir`
    /// between open and walk made this capture read a different file
    /// from the one gix's platform resolves.
    ///
    /// # Errors
    ///
    /// Fails when `core.attributesFile` is set but cannot be
    /// interpolated, or when a file exists but cannot be read — the
    /// errors gix's own platform build raises for the same inputs.
    fn read(
        repo: &gix::Repository,
        env: &mut dyn FnMut(&str) -> Option<OsString>,
        permissions: gix::open::permissions::Attributes,
    ) -> gix::Result<Self> {
        let base = repo.current_dir();
        let system = permissions
            .system
            .then(|| gix::attrs::Source::System.storage_location(env))
            .flatten()
            .map(|path| base.join(path));
        let user = user_attributes_path(repo, env, permissions)?;
        let mut files = [
            system,
            user,
            Some(base.join(repo.common_dir()).join("info").join("attributes")),
        ]
        .map(|path| AttributeSource { path, bytes: None });
        for file in &mut files {
            file.bytes = match file.path.as_deref().map(std::fs::read) {
                Some(Ok(bytes)) => Some(bytes),
                Some(Err(err)) if !gix_treats_as_missing(&err) => {
                    return Err(gix::Error::from_error(err));
                }
                Some(Err(_)) | None => None,
            };
        }
        Ok(Self { files })
    }

    /// Build the attribute state gix's `attributes_only` would, from the
    /// captured bytes instead of from a read of its own, with
    /// `root_attributes` — the root `.gitattributes` blob, when the index
    /// holds one — folded in where gix's stack would push it.
    ///
    /// # Reproducing gix's precedence from captured bytes
    ///
    /// gix seeds its *global* group with the built-in `binary` macro and
    /// then each file it read, and leaves `info/attributes` to the
    /// stack's first directory push, which appends it to the
    /// *per-directory* group right after the root `.gitattributes`.
    /// Matching walks the per-directory group, then the globals, each
    /// last-added-first, so the order a lookup consults is: a nested
    /// `.gitattributes`, `info/attributes`, the root `.gitattributes`,
    /// the user file, the system file, the built-in.
    /// (That is not git's: `git check-attr` has `info/attributes`
    /// override every in-tree file, nested ones included. The walk has
    /// always resolved through gix's stack, so gix's order is the one
    /// reproduced here, and the parity test says where the two differ.)
    ///
    /// The per-directory group cannot be seeded from outside, so the
    /// captured `info/attributes` has to be a global — and a global
    /// below the root `.gitattributes` unless the root moves too. It
    /// does: the root blob is appended to the globals just before
    /// `info/attributes`, with the base and macro permission gix's push
    /// gives it (none, and allowed, since the root is the directory
    /// macros may be read from), and [`tree_diff_platform`] withholds
    /// its mapping from the stack so the first push finds nothing and
    /// pushes gix's empty placeholder instead. The group that holds a
    /// list is then different, the order lookups consult is the same,
    /// and so is the order the lists enter the metadata collection.
    /// `the_hand_built_stack_resolves_diff_as_gix_s_does` pins this
    /// against gix's own construction on a fixture where every pair of
    /// sources overlaps.
    fn into_attributes(
        self,
        root_attributes: Option<&[u8]>,
        source: gix::worktree::stack::state::attributes::Source,
    ) -> std::io::Result<gix::worktree::stack::state::Attributes> {
        let mut collection = gix::attrs::search::MetadataCollection::default();
        let mut globals = gix::attrs::Search::default();
        globals.add_patterns_buffer(
            b"[attr]binary -diff -merge -text",
            "[builtin]".into(),
            None,
            &mut collection,
            true,
        )?;
        let [system, user, info] = self.files;
        for file in [system, user] {
            let (Some(path), Some(bytes)) = (file.path, file.bytes) else {
                continue;
            };
            globals.add_patterns_buffer(&bytes, path, None, &mut collection, true)?;
        }
        if let Some(bytes) = root_attributes {
            globals.add_patterns_buffer(
                bytes,
                ROOT_ATTRIBUTES.into(),
                None,
                &mut collection,
                true,
            )?;
        }
        if let (Some(path), Some(bytes)) = (info.path, info.bytes) {
            globals.add_patterns_buffer(&bytes, path, None, &mut collection, true)?;
        }
        Ok(gix::worktree::stack::state::Attributes::new(
            globals, None, source, collection,
        ))
    }
}

/// The digest [`WalkContext::diff_config_digest`] documents, over any
/// platform: the walk's, or one built another way for comparison.
fn diff_config_digest(platform: &gix::diff::blob::Platform) -> u64 {
    // Exhaustive destructures, for the reason `algorithm_tag` matches
    // exhaustively: a field a gix bump adds fails here rather than
    // staying out of a persisted digest. The ignored fields are the
    // ones the list above leaves out.
    let gix::diff::blob::platform::Options {
        algorithm: platform_algorithm,
        skip_internal_diff_if_external_is_configured: _,
    } = platform.options;
    let gix::diff::blob::pipeline::Options {
        large_file_threshold_bytes,
        fs: _,
    } = platform.filter.options;
    let mut hasher = DefaultHasher::new();
    algorithm_tag(platform_algorithm).hash(&mut hasher);
    large_file_threshold_bytes.hash(&mut hasher);
    for driver in platform.filter.drivers() {
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

/// Build the tree-to-tree blob-diff platform from inputs already read:
/// what [`gix::Repository::diff_resource_cache_for_tree_diff`] builds,
/// minus the index load and the attribute-file reads it performs for
/// itself. Spelled out so [`WalkContext::open`] can read each of those
/// once and digest the attribute bindings from the same reads (#1760),
/// rather than from a second index load or a second read of a file that
/// could have changed in between.
/// `the_hand_built_platform_matches_gix_s` pins that the two
/// constructions diff under the same configuration, and
/// `the_hand_built_stack_resolves_diff_as_gix_s_does` that their
/// attribute stacks resolve `diff` alike.
///
/// The attribute source is `IdMapping`: without worktree roots gix reads
/// `.gitattributes` from the index's blobs, never from the working tree,
/// which is what `diff_resource_cache` selects for an unset
/// [`WorktreeRoots`](gix::diff::blob::pipeline::WorktreeRoots). The
/// stack's root is the one gix's `attributes_only` uses — a virtual
/// root that is never read, since every lookup resolves through
/// `mappings`.
fn tree_diff_platform(
    repo: &gix::Repository,
    sources: AttributeSources,
    ignore_case: bool,
    mut mappings: Vec<(BString, gix::ObjectId)>,
) -> gix::Result<gix::diff::blob::Platform> {
    let root_attributes = take_root_attributes(repo, &mut mappings)?;
    let attributes = sources
        .into_attributes(
            root_attributes.as_deref(),
            gix::worktree::stack::state::attributes::Source::IdMapping,
        )
        .map_err(gix::Error::from_error)?;
    let stack = gix::worktree::Stack::new(
        repo.workdir().unwrap_or(repo.git_dir()),
        gix::worktree::stack::State::AttributesStack(attributes),
        glob_case(ignore_case),
        Vec::with_capacity(512),
        mappings,
    );
    gix::diff::resource_cache(
        repo,
        gix::diff::blob::pipeline::Mode::ToGit,
        stack,
        gix::diff::blob::pipeline::WorktreeRoots::default(),
    )
}

/// Remove the root `.gitattributes` from `mappings` and return its blob,
/// for [`AttributeSources::into_attributes`] to place among the globals.
/// The lookup is the exact one gix's `push_directory` makes for the root
/// — a binary search on the sorted mappings for the literal
/// [`ROOT_ATTRIBUTES`] — so the entry withheld here is precisely the one
/// the stack would have found. Under `core.ignoreCase` the mappings may
/// also hold a `.GitAttributes`; gix's search misses it just the same,
/// so it stays.
///
/// # Errors
///
/// Fails when the index names a blob the object database does not hold,
/// the failure gix's first push would raise.
fn take_root_attributes(
    repo: &gix::Repository,
    mappings: &mut Vec<(BString, gix::ObjectId)>,
) -> gix::Result<Option<Vec<u8>>> {
    let Ok(at) =
        mappings.binary_search_by(|(path, _)| path.as_slice().cmp(ROOT_ATTRIBUTES.as_bytes()))
    else {
        return Ok(None);
    };
    let (_, id) = mappings.remove(at);
    Ok(Some(repo.find_blob(id)?.take_data()))
}

/// The two platforms one commit's tree diff fills: gix's rewrite tracker
/// borrows one for the whole `for_each_to_obtain_tree_with_cache` call,
/// and the visitor diffs each change through the other for its line
/// counts. They cannot be one object — the visitor runs inside the call
/// that holds the first — so the walk clones the context's template
/// twice (drivers, attribute stack, an empty cache) once per build.
struct DiffPlatforms {
    /// Filled by the visitor's `change.diff` for line counts.
    lines: gix::diff::blob::Platform,
    /// Filled by gix's rename tracking, when `track_rewrites` is set.
    rewrites: gix::diff::blob::Platform,
}

impl DiffPlatforms {
    /// Two empty-cache clones of `template`.
    fn from_template(template: &gix::diff::blob::Platform) -> Self {
        Self {
            lines: template.clone(),
            rewrites: template.clone(),
        }
    }

    /// Drop every cached blob from both platforms, buffers included.
    fn clear(&mut self) {
        self.lines.clear_resource_cache();
        self.rewrites.clear_resource_cache();
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
    // Take every diff through the context's platform so the churn
    // recorded here was produced under the configuration
    // `diff_config_digest` stamps the entry with. The walk fills and
    // clears the resource caches per commit, so it takes clones — the
    // drivers, the attribute stack and an empty cache, twice per build —
    // rather than a mutable borrow of the context. Handing gix the
    // rewrite platform is what stops `for_each_to_obtain_tree` building
    // one per commit, with the index load and attribute-file probes that
    // costs (#1761).
    let mut platforms = DiffPlatforms::from_template(&walk.diff);

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
            walk,
            &commit,
            info.id,
            raw_time,
            &resolver,
            rewrites,
            &mut platforms,
        )? {
            events.push(event);
        }

        // The resource caches only grow; clear both each commit to keep
        // memory bounded across a long history (gix docs guidance). Unlike
        // `enumerate_target_files`, the walk frees the buffers too: kept,
        // each grows to the largest blob it has held, and over this
        // repository's history that raised peak RSS by 7-13 MB for a
        // 1-2% shorter walk.
        platforms.clear();
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
    platforms: &mut DiffPlatforms,
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

    let (renames, touched) = diff_collect(&parent_tree, &commit_tree, platforms, rewrites)?;

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
    platforms: &mut DiffPlatforms,
    rewrites: Option<Rewrites>,
) -> Result<(RenameEdges, TouchedFiles), Error> {
    use gix::object::tree::diff::Change;

    let DiffPlatforms {
        lines: cache,
        rewrites: rewrite_cache,
    } = platforms;
    let mut renames: RenameEdges = Vec::new();
    let mut touched: TouchedFiles = Vec::new();
    parent_tree
        .changes()
        .map_err(diff_err)?
        .options(|opts| {
            opts.track_path();
            opts.track_rewrites(rewrites);
        })
        .for_each_to_obtain_tree_with_cache(
            commit_tree,
            rewrite_cache,
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

#[cfg(test)]
#[path = "history_coverage_tests.rs"]
mod coverage_tests;
