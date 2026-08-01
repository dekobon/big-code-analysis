//! File-set resolution and walk configuration for the CLI: the
//! `--paths`/`--include`/`--exclude` to [`GlobalOpts`] assembly, seed
//! classification and expansion, include/exclude filtering, the
//! `--language` resolver, and the `bca diff --since` CWD guard. The
//! Config-bound walk entry points (`run_walk`, `walk_metric_set`) stay in
//! `lib.rs` because they touch `Config`'s private fields.

use super::*;

/// One aggregated per-file result for the single-file `--output <FILE>`
/// mode on `metrics` / `ops` (#669). Workers stream these through
/// `Config::aggregate_tx`; the command runner collects them after the
/// walk and serializes the whole set as one document. Boxed so the
/// channel item stays small (a `FuncSpace` tree is large).
pub(crate) enum AggregateItem {
    /// A `metrics` file-level space plus its emitted path (the path is
    /// needed for the CSV aggregate, whose rows are keyed by file).
    Metrics(Box<FuncSpace>, PathBuf),
    /// An `ops` operator/operand tree.
    Ops(Box<Ops>),
}

/// Assemble a runtime [`GlobalOpts`] from a subcommand's flag groups.
/// `tuning`, `preproc`, `output`, and the `language` source vary by
/// subcommand (a command that does not flatten a group passes the group
/// default), so the builder takes each piece explicitly. `positional`
/// carries the trailing `[PATHS]` (#651), unioned positional-first with
/// the group's `--paths` values.
pub(crate) fn assemble_globals(
    selection: &WalkSelectionArgs,
    positional: &PositionalPaths,
    tuning: &WalkTuningArgs,
    preproc: &PreprocConsumeArgs,
    output: &OutputArgs,
    universal: &UniversalArgs,
) -> GlobalOpts {
    let mut paths = positional.positional_paths.clone();
    paths.extend(selection.paths.iter().cloned());
    GlobalOpts {
        paths,
        include: selection.include.clone(),
        exclude: selection.exclude.clone(),
        num_jobs: tuning.num_jobs,
        language: selection.language.clone(),
        warning: universal.warning,
        no_skip_generated: selection.no_skip_generated,
        report_skipped: universal.report_skipped,
        preproc_data: preproc.preproc_data.clone(),
        paths_from: selection.paths_from.clone(),
        exclude_from: selection.exclude_from.clone(),
        no_ignore: selection.no_ignore,
        exclude_tests: tuning.exclude_tests,
        count_cyclomatic_try: tuning.resolved_count_cyclomatic_try(),
        no_config: selection.no_config,
        color: output.color,
    }
}

/// Resolve the optional `--language` value to a forced [`LANG`].
///
/// Accepts either a canonical language name (`rust`, `python`, `cpp`,
/// …, via [`LANG`]'s `FromStr`) or a file extension (`rs`, `py`, …, via
/// [`get_from_ext`]). The name spelling is tried first so the obvious
/// `-l rust` works; extensions remain accepted for backward
/// compatibility. An unrecognized value is a hard error (`die`, exit 1)
/// listing the valid language names — previously such a value silently
/// disabled analysis with exit 0 (issue #595).
pub(crate) fn resolve_language(typ: Option<&str>, action: &Action) -> Option<LANG> {
    // Force `Preproc` for the producer so `act_on_file`'s "skip
    // unrecognized" guard never fires — every walked file must reach the
    // dispatch where the producer runs its own Cpp check.
    if matches!(action, Action::PreprocProduce) {
        return Some(LANG::Preproc);
    }
    let value = typ?;
    // Try the canonical name first (`rust`), then fall back to treating
    // the value as a file extension (`rs`). Both are user-reasonable
    // spellings of the same intent.
    value
        .parse::<LANG>()
        .ok()
        .or_else(|| get_from_ext(value))
        .or_else(|| {
            die(format_args!(
                "unknown --language value '{value}'; {}",
                valid_languages()
            ))
        })
}

/// Build the "valid values" suffix for the `--language` error, listing
/// every canonical language name. Mirrors the unknown-metric error
/// style used by `--threshold`.
pub(crate) fn valid_languages() -> String {
    let mut names: Vec<&'static str> = LANG::into_enum_iter().map(|lang| lang.name()).collect();
    names.sort_unstable();
    format!("valid languages are: {}", names.join(", "))
}

/// Borrowed include/exclude glob pair plus the walk-root-anchored match
/// predicate they drive. Grouping the two globsets keeps
/// `expand_seed_paths`'s signature narrow and co-locates the match
/// convention (empty globset = no-op) with the patterns it applies to.
pub(crate) struct WalkFilters<'a> {
    include: &'a GlobSet,
    exclude: &'a ExcludeGlobs,
    /// Whether `--language` forces a language, which makes every named
    /// file analyzable regardless of its extension. Only the
    /// exclude-override warning consults it — see
    /// [`Self::warn_exclude_overridden`] for why a filter bundle knows
    /// about languages at all.
    language_forced: bool,
}

impl WalkFilters<'_> {
    /// Does `match_path` pass the include/exclude filters? An empty
    /// globset is a no-op (no `--include` means "all"; no `--exclude`
    /// means "none"). `match_path` is the form anchored to the file's
    /// walk root (#489), so `./`-anchored patterns match regardless of
    /// how the seed was spelled.
    fn passes(&self, match_path: &Path) -> bool {
        // Strip a leading `./` so bare-relative patterns (`dir/**`) match
        // the `./`-anchored walk-root form just like `./dir/**` does (#726).
        let match_path = walk_seed::strip_cur_dir(match_path);
        (self.include.is_empty() || self.include.is_match(match_path))
            && (self.exclude.is_empty() || !self.exclude.is_match(match_path))
    }

    /// Does `match_path` satisfy only the include allow-list (empty =
    /// "all")? Used for explicitly-named file seeds, which bypass the
    /// exclude deny-set: a project's ignore rules shape *directory-walk*
    /// scope and must not silently drop a file the user named on the
    /// command line (#726). The include allow-list still narrows which
    /// named files are analyzed.
    fn includes(&self, match_path: &Path) -> bool {
        let match_path = walk_seed::strip_cur_dir(match_path);
        self.include.is_empty() || self.include.is_match(match_path)
    }

    /// Report on stderr that the explicitly-named `seed` overrode the
    /// exclude deny-set, naming the pattern it matched.
    ///
    /// This is the gap between [`Self::includes`] and [`Self::passes`]
    /// made visible (#1146). The override is deliberate, but it used to
    /// be silent, so a file the project had put out of scope came back
    /// as a `bca check` offender for any caller that named paths one at
    /// a time — the shape the per-edit agent hooks use. The wording sits
    /// beside `bca check`'s `bca: skipped N violations via
    /// [check.exclude]` so the two read as one family.
    ///
    /// Lives here rather than at the call site because the pattern
    /// spelling is this type's to know, and because the message is the
    /// only reason `expand_seed_paths` would need it.
    ///
    /// Silent for a seed no language claims, which is why this consults
    /// the language table at all: the advertised `git diff --name-only |
    /// bca metrics --paths-from -` pipeline feeds in whole changesets,
    /// where lockfiles, Markdown, and generated assets are the majority
    /// and produce no output either way. Warning that such a file is
    /// being "analyzed anyway" would be both noisy and untrue.
    fn warn_exclude_overridden(&self, seed: &Path, match_path: &Path) {
        if !self.language_forced && !seed_has_known_language(seed) {
            return;
        }
        if let Some(glob) = self
            .exclude
            .first_match(walk_seed::strip_cur_dir(match_path))
        {
            eprintln!(
                "bca: warning: {} matches an exclude pattern ({glob}) \
                 but was named explicitly; analyzing anyway",
                seed.display()
            );
        }
    }
}

/// Does `seed`'s extension map to a supported language? Mirrors the
/// per-file dispatch's own extension lookup, which is what decides
/// whether a walked file is analyzed or silently skipped.
fn seed_has_known_language(seed: &Path) -> bool {
    seed.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| get_from_ext(ext).is_some())
}

/// Resolved file set plus the subset of seeds that were *explicitly
/// named files* (not products of a directory expansion).
///
/// `explicit_files` backs the #663 rule: an explicitly-named file whose
/// language is unrecognized must warn on stderr and, when it is the sole
/// reason the run produced nothing, exit 1 — mirroring the #596
/// nonexistent-explicit-path error. A file discovered by walking a
/// directory seed is *not* in this set, so a tree full of READMEs and
/// configs stays silently skipped (gated behind `-w`).
///
/// `walk_errors` carries the #1131 tally: entries the directory walk
/// itself could not read. It is reported separately from the worker
/// pool's `read_failures` because the two describe different losses —
/// a read failure names a file the walk *selected*, whereas an
/// unlistable directory removes its whole subtree before anything can
/// be selected, leaving nothing for a worker to fail on.
pub(crate) struct ResolvedFiles {
    pub(crate) files: Vec<PathBuf>,
    pub(crate) explicit_files: std::collections::HashSet<PathBuf>,
    pub(crate) walk_errors: WalkErrors,
}

/// How many walk entries the traversal could not read (#1131).
///
/// A newtype rather than a bare `usize` so it cannot be confused with
/// the several other counts threaded through the same call chain (the
/// worker pool's read/write failure tallies, the job count, the
/// resolved file count) — and so a call site that forwards it has to
/// name what it is forwarding.
///
/// **Only `ignore::Error`s carrying an underlying `io::Error` are
/// counted.** The reachable non-I/O case is a malformed ignore file in
/// an *ancestor* of the walk root, which `Worker::add_parents` surfaces
/// as an `Error::Glob`; it stays a warning, because it describes how the
/// walk was configured rather than a subtree the walk dropped. Widening
/// the tally to every variant would make a stray `.gitignore` typo fail
/// a build — pinned by
/// `malformed_parent_gitignore_warns_but_still_exits_zero` in
/// `tests/read_failures.rs`.
///
/// `Error::Loop` is not a concern here: `follow_links` is off, so the
/// walker never runs its symlink-loop check.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[must_use]
pub(crate) struct WalkErrors(usize);

impl WalkErrors {
    fn add(&mut self, count: usize) {
        self.0 += count;
    }

    pub(crate) fn count(self) -> usize {
        self.0
    }
}

/// What kind of on-disk object a `--paths` seed resolves to, used to
/// route a seed into the single-file or directory-walk branch of
/// [`expand_seed_paths`]. A symlink is classified by its *target* (the
/// one deliberate follow — the user named the link as a seed), so the
/// returned kind is always `File` or `Dir`; anything else (a FIFO,
/// socket, …) is treated as a directory walk root, which discovers no
/// regular files and is harmless.
#[derive(Clone, Copy)]
pub(crate) enum SeedKind {
    File,
    Dir,
}

impl SeedKind {
    pub(crate) fn is_file(self) -> bool {
        matches!(self, Self::File)
    }
}

/// Classify `seed` without letting a *missing* path masquerade as a
/// real one. `symlink_metadata` first establishes the seed exists at all
/// (it stats the link itself, so a dangling symlink correctly errors
/// here — symmetric with the walk's `follow_links(false)`, #704); a live
/// symlink is then resolved through `metadata` exactly once to classify
/// its target. Returns `Err` only when the seed (or a symlink's target)
/// does not exist, which the caller turns into the #596 "path does not
/// exist" hard error.
pub(crate) fn seed_kind(seed: &Path) -> std::io::Result<SeedKind> {
    let link_meta = seed.symlink_metadata()?;
    let meta = if link_meta.file_type().is_symlink() {
        // Explicitly-named symlink seed: resolve its target once. A
        // dangling target propagates the `Err` (treated as nonexistent).
        seed.metadata()?
    } else {
        link_meta
    };
    Ok(if meta.is_file() {
        SeedKind::File
    } else {
        SeedKind::Dir
    })
}

/// The resolved file set under construction, plus the dedupe index that
/// keeps overlapping seeds (`--paths src --paths src/lib.rs`, or two
/// seeds whose trees intersect) from contributing a file twice (#704).
///
/// A type rather than three locals in [`expand_seed_paths`] because the
/// dedupe is an invariant of the *set*, not of either branch that feeds
/// it: nothing reaches `files` without first passing `seen`. Stated once
/// here, it cannot be half-applied by a future third feeder — the file
/// branch and the walk branch previously restated it separately.
#[derive(Default)]
struct SeedSet {
    files: Vec<PathBuf>,
    seen: std::collections::HashSet<PathBuf>,
    explicit_files: std::collections::HashSet<PathBuf>,
}

impl SeedSet {
    /// Admit an explicitly-named file seed, recording it in
    /// `explicit_files` so the per-file dispatch can tell it from a
    /// directory-expansion product: an explicitly-named file with an
    /// unrecognized language must warn and may exit 1 (#663), whereas a
    /// walked one stays silently skipped.
    ///
    /// Returns whether the seed was newly admitted, so the caller can
    /// scope a one-shot per-seed diagnostic to its first mention.
    fn push_explicit(&mut self, seed: &Path) -> bool {
        if !self.seen.insert(seed.to_path_buf()) {
            return false;
        }
        self.explicit_files.insert(seed.to_path_buf());
        self.files.push(seed.to_path_buf());
        true
    }

    /// Admit a file discovered by expanding a directory seed.
    fn push_walked(&mut self, path: PathBuf) {
        if self.seen.insert(path.clone()) {
            self.files.push(path);
        }
    }
}

pub(crate) fn expand_seed_paths(
    mut paths: Vec<PathBuf>,
    paths_from: Option<PathBuf>,
    no_ignore: bool,
    threads: usize,
    filters: &WalkFilters<'_>,
) -> ResolvedFiles {
    if let Some(src) = paths_from {
        paths.extend(read_paths_from(&src).unwrap_or_else(|e| die(e)));
    }
    // Default the walk root to the current directory when the user
    // supplied no seeds — neither via `--paths`/`--paths-from` nor via
    // a manifest `paths` key (the manifest merge already populated
    // `paths` before we reach here, so an empty `paths` here means
    // "nothing was configured anywhere"). This mirrors `bca vcs`
    // (`vcs_command::run`) so a bare `bca metrics` ranks the tree the
    // user is standing in instead of analyzing nothing (#596). The
    // injected `.` always exists, so it never trips the
    // nonexistent-seed guard below.
    if paths.is_empty() {
        paths.push(PathBuf::from("."));
    }
    let mut found = SeedSet::default();
    let mut walk_errors = WalkErrors::default();
    for seed in paths.into_iter().map(walk_seed::reanchor_seed) {
        // Classify the seed *without* following a final symlink so the
        // seed-level existence/kind check is symmetric with the walk's
        // `follow_links(false)` (#704): the previous `exists()` /
        // `is_file()` / `is_dir()` trio each followed symlinks, so a
        // dangling symlink seed passed the existence guard yet produced
        // no files (TOCTOU / asymmetry). `seed_kind` inspects the link
        // itself via `symlink_metadata`, then resolves a live symlink
        // once — the user *explicitly* named it as a seed and expects it
        // honored, so that single follow is the documented exception.
        let Ok(kind) = seed_kind(&seed) else {
            // An explicitly-supplied seed that does not exist (or whose
            // symlink dangles) is a tool error (exit 1), not a skipped
            // warning (#596): a typo in `--paths` / `--paths-from` /
            // manifest `paths` must fail loudly rather than silently
            // analyze nothing. Only the auto-injected `.` default reaches
            // the walk without being explicitly supplied, and it always
            // exists.
            die(format_args!("path does not exist: {}", seed.display()));
        };
        if kind.is_file() {
            // A single explicit file seed keeps the form the caller
            // spelled (its emitted `name` must match the single-file
            // `bca.analyze()` API). It bypasses the exclude deny-set: a
            // file named directly on the command line is a direct request
            // that a project's directory-walk ignore rules (`.bcaignore`,
            // `--exclude`, manifest `exclude`) must not silently drop
            // (#726), matching the ripgrep/fd convention that an explicit
            // path overrides ignore rules. An `--include` allow-list still
            // narrows which named files are analyzed, matched against the
            // seed's CWD-relative form so `--include 'src/**'` treats
            // `--paths "$PWD/src/f.rs"` and `--paths src/f.rs` alike.
            let include_form = walk_seed::file_seed_match_path(&seed);
            // Gated on the admission so an overlapping seed warns once
            // rather than per mention (#1146).
            if filters.includes(&include_form) && found.push_explicit(&seed) {
                filters.warn_exclude_overridden(&seed, &include_form);
            }
            continue;
        }
        // The dedupe spans every seed, so it belongs to `found` rather
        // than to one seed's walk — a single walk cannot see the others.
        for path in walk_directory_seed(&seed, no_ignore, threads, filters, &mut walk_errors) {
            found.push_walked(path);
        }
    }
    // A walk that resolved zero files is almost always a mistake — an
    // over-narrow `--include`, an `--exclude` that swept everything, or
    // a directory with no supported sources. Surface it on stderr so a
    // bare `bca metrics` in an empty tree is not silently a no-op (#596).
    // Non-gate commands still exit 0; `check` layers its own hard error
    // (`no input files matched`) on top for CI safety.
    if found.files.is_empty() {
        warn("0 files matched");
    }
    ResolvedFiles {
        files: found.files,
        explicit_files: found.explicit_files,
        walk_errors,
    }
}

/// Walk the directory `seed` with `threads` walker threads, returning every
/// supported file that passes the include/exclude `filters`, sorted.
/// Factored out of [`expand_seed_paths`] so its per-seed loop reads as
/// "handle a file seed, else expand a directory seed" rather than inlining the
/// whole `ignore::WalkBuilder` setup and per-entry handling.
///
/// Deduping against the other seeds is the caller's job: `seen` spans every
/// seed, and one seed's walk cannot see the others.
///
/// A per-entry walk error (an unreadable subdirectory, a broken symlink, a
/// racing unlink) skips that entry with a warning rather than aborting the
/// run (#704): a single EACCES directory deep in a large tree previously took
/// down every file the walk had yet to reach. This mirrors the per-file
/// tolerance the worker pool already applies to unparseable files.
///
/// Tolerating the entry is not the same as reporting success, though, so
/// each I/O-backed error is also added to `errors` for the caller's
/// exit-code guard (#1131) — an unlistable directory removes its whole
/// subtree from the resolved set, which is invisible in the output.
fn walk_directory_seed(
    seed: &Path,
    no_ignore: bool,
    threads: usize,
    filters: &WalkFilters<'_>,
    errors: &mut WalkErrors,
) -> Vec<PathBuf> {
    // Reporting the tally through a return value instead of `errors`
    // costs `expand_seed_paths` more than it saves: measured, the
    // tuple-plus-accumulate shape puts that function's halstead.effort
    // at 50_003 against a hard limit of 50_000, trading a soft nargs
    // tier for a hard breach. The five parameters are each
    // independently meaningful, and no two bundle under a name worth
    // having.
    // bca: suppress(nargs)
    use ignore::{WalkBuilder, WalkState};
    let mut wb = WalkBuilder::new(seed);
    wb.hidden(true)
        .follow_links(false)
        .require_git(false)
        .git_ignore(!no_ignore)
        .git_exclude(!no_ignore)
        .git_global(!no_ignore)
        .ignore(!no_ignore)
        .parents(!no_ignore)
        // `getdents` plus gitignore matching over a large tree used to
        // run single-threaded on the main thread while every worker sat
        // idle waiting for the list (#1114).
        .threads(threads);

    // Visitors hand matches over a channel rather than a shared `Vec`
    // behind a `Mutex`: crossbeam's unbounded sender takes no lock, so
    // the per-entry hot path does not serialize the walker threads
    // against each other.
    let (tx, rx) = crossbeam::channel::unbounded();
    // The error tally is a shared counter rather than a second channel
    // item: a walk error yields no path, so widening `tx`'s item to an
    // enum would make every hot-path send pay for the rare case.
    let io_errors = AtomicUsize::new(0);
    wb.build_parallel().run(|| {
        let tx = tx.clone();
        let io_errors = &io_errors;
        Box::new(move |entry| {
            match entry {
                Ok(entry) => {
                    if entry.file_type().is_some_and(|t| t.is_file()) {
                        let path = entry.into_path();
                        // Anchor the glob match to the walk root rather than
                        // the emitted (possibly absolute) path, so
                        // `./`-anchored excludes match regardless of how the
                        // seed resolved — including a manifest root above the
                        // CWD (#489).
                        if filters.passes(&walk_seed::match_path_for(seed, &path)) {
                            // The receiver outlives the walk (it is drained
                            // below), so this cannot fail.
                            let _ = tx.send(path);
                        }
                    }
                }
                // A per-entry error skips that entry rather than aborting
                // the run (#704): a single EACCES directory deep in a large
                // tree must not take down every file the walk has yet to
                // reach.
                Err(e) => {
                    eprintln!(
                        "bca: warning: skipping walk entry in {}: {e}",
                        seed.display()
                    );
                    // Every variant warns; only an I/O-backed one is
                    // tallied, because only that one means the walk lost
                    // files it should have seen (#1131). See
                    // [`WalkErrors`] for why the rest stay non-fatal.
                    if e.io_error().is_some() {
                        io_errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            WalkState::Continue
        })
    });
    // Drop the builder's own sender so the drain below terminates; every
    // visitor clone is already gone, `run` having joined its threads.
    drop(tx);
    errors.add(io_errors.into_inner());

    // A parallel walk yields entries in whatever order its threads
    // happen to finish, so without this sort the resolved file list —
    // and therefore the order `bca metrics` prints per-file documents at
    // `--jobs 1` — would differ run to run on the same tree. Sorting
    // also makes that order independent of readdir order, so it no
    // longer varies by filesystem or machine, which the previous
    // single-threaded walk never guaranteed.
    let mut found: Vec<PathBuf> = rx.into_iter().collect();
    found.sort_unstable();
    found
}

/// Resolve the seeds into the terminal, walk-root-anchored file list
/// plus the worker count. The include/exclude filtering is applied here,
/// anchored to each file's walk root (#489), so the resolved list is the
/// final file set: the library runner processes it as-is, with no second
/// walk and no second, emitted-path-form-sensitive glob match (the dead
/// library globsets and re-walk were removed in #495). This anchored
/// walk is the single filtering seam.
pub(crate) fn resolve_walk_files(globals: GlobalOpts) -> (ResolvedFiles, usize) {
    let include = mk_globset(globals.include).unwrap_or_else(|e| die(e));
    let exclude = build_exclude_globset(
        globals.exclude,
        globals.exclude_from.as_deref(),
        "--exclude-from",
    );
    let num_jobs = globals.num_jobs.resolve();
    let filters = WalkFilters {
        include: &include,
        exclude: &exclude,
        language_forced: globals.language.is_some(),
    };
    let resolved = expand_seed_paths(
        globals.paths,
        globals.paths_from,
        globals.no_ignore,
        // Same budget the worker pool gets: the walk and the analysis
        // never run at the same time, so there is nothing to split it
        // with (#1114).
        num_jobs,
        &filters,
    );
    (resolved, num_jobs)
}

/// RAII guard that restores the process working directory to its prior
/// value when dropped. Returned by [`with_cwd`].
pub(crate) struct CwdGuard {
    previous: PathBuf,
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        // Best-effort restore: if the prior directory is gone the
        // process is already in a degraded state, and `bca diff` is
        // about to return anyway. Swallowing the error keeps `Drop`
        // panic-free (a panic here would mask the real error on the
        // unwinding path).
        let _ = std::env::set_current_dir(&self.previous);
    }
}

/// Switch the process working directory to `dir`, returning a guard that
/// restores the previous directory on drop (including every `?`/error
/// path in the caller). Surfaces the current-dir read and the switch as
/// [`metric_diff::DiffError::Read`] so the `bca diff --since` caller can
/// render a single error kind.
pub(crate) fn with_cwd(dir: &Path) -> Result<CwdGuard, metric_diff::DiffError> {
    let previous = std::env::current_dir().map_err(|source| metric_diff::DiffError::Read {
        path: PathBuf::from("."),
        source,
    })?;
    std::env::set_current_dir(dir).map_err(|source| metric_diff::DiffError::Read {
        path: dir.to_path_buf(),
        source,
    })?;
    Ok(CwdGuard { previous })
}

#[cfg(test)]
#[path = "walk_tests.rs"]
mod tests;
