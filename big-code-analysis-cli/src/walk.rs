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
    /// needed for the CSV aggregate, whose rows are keyed by file, and
    /// as the aggregate document's ordering key).
    Metrics(Box<FuncSpace>, PathBuf),
    /// An `ops` operator/operand tree plus its emitted path. `Ops`
    /// carries only a `name`, which for non-UTF-8 input is a *lossy*
    /// rendering of that path, so the path rides alongside it as the
    /// ordering key (see `write_aggregate`).
    Ops(Box<Ops>, PathBuf),
}

impl AggregateItem {
    /// The emitted path of the file this result came from — the
    /// aggregate document's ordering key (#1244).
    pub(crate) fn emitted_path(&self) -> &Path {
        match self {
            Self::Metrics(_, path) | Self::Ops(_, path) => path,
        }
    }
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
        // Filled by the `bca.toml` merge, which runs after this.
        manifest_excludes: None,
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
    /// Both exclude sets plus the roots they resolve against. One type
    /// rather than a `{cli, manifest, manifest_dir}` triple per call
    /// site: the three sites that spell this rule used to give three
    /// different answers about anchoring, which is what let a manifest
    /// glob stop applying under a directory seed (#1189).
    excludes: walk_seed::AnchoredExcludes<'a>,
    /// Whether `--language` forces a language, which makes every named
    /// file analyzable regardless of its extension. Consulted through
    /// [`Self::analyzable`] by the exclude-override warning and the
    /// ignore-rule measurement — see [`Self::warn_exclude_overridden`]
    /// for why a filter bundle knows about languages at all.
    language_forced: bool,
    /// Whether this walk measures what ignore rules dropped (#1055).
    /// On only for [`resolve_walk_files_with_ignored`], and forced off
    /// under `--no-ignore`, where nothing can be ignore-dropped. Rides
    /// the filter bundle so the visitor and seed loop need no extra
    /// parameter to know whether the directory channel has a consumer.
    measure_ignored: bool,
}

impl WalkFilters<'_> {
    /// Does `match_path` pass the include/exclude filters? An empty
    /// globset is a no-op (no `--include` means "all"; neither exclude
    /// set configured means "none"). `match_path` is the form anchored
    /// to the file's walk root (#489), so `./`-anchored patterns match
    /// regardless of how the seed was spelled.
    ///
    /// Each exclude set is matched at its own root — see
    /// [`walk_seed::AnchoredExcludes`]. Matching the manifest set at the
    /// walk root instead is correct only when the two coincide, which
    /// they do for the canonical `paths = ["."]` and do not for a
    /// directory seed: `bca metrics -p sub` moved the walk root to `sub`
    /// and every manifest glob stopped applying (#1189).
    ///
    /// `path` is the file as the walk found it, needed because manifest
    /// anchoring resolves against the `bca.toml` directory rather than
    /// against the walk root the `match_path` form carries.
    fn passes(&self, path: &Path, match_path: &Path) -> bool {
        // Strip a leading `./` so bare-relative patterns (`dir/**`) match
        // the `./`-anchored walk-root form just like `./dir/**` does (#726).
        let match_path = walk_seed::strip_cur_dir(match_path);
        (self.include.is_empty() || self.include.is_match(match_path))
            && !self.excludes.excludes(path, walk_seed::CwdForm(match_path))
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

    /// Would any parser own `path`? The extension half of the language
    /// guess only (the modeline and shebang fallbacks need the file's
    /// bytes, which the walk has not read), routed through
    /// [`get_language_for_file`] so the #1111 ASCII case-fold applies —
    /// the one shared spelling of a rule that two call sites used to
    /// risk deriving apart.
    fn analyzable(&self, path: &Path) -> bool {
        self.language_forced || get_language_for_file(path).is_some()
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
    ///
    /// [`get_language_for_file`] rather than a local `extension()` +
    /// `get_from_ext` pair, which is what this shipped as: that spelling
    /// dropped the ASCII case-fold #1111 put *inside*
    /// `get_language_for_file`, so `bca check SKIPME/A.RS` analyzed the
    /// file and stayed silent about the override — the exact asymmetry
    /// #1146 exists to remove, surviving for every mixed-case extension.
    /// It is deliberately the extension half of `guess_language` only:
    /// the dispatch's modeline and shebang fallbacks need the file's
    /// bytes, and the walk has not read them here.
    fn warn_exclude_overridden(&self, seed: &Path, match_path: &Path) {
        if !self.analyzable(seed) {
            return;
        }
        let cwd_form = walk_seed::CwdForm(walk_seed::strip_cur_dir(match_path));
        // Each set at its own root, via the same rule `passes` applies —
        // a manifest glob is written against the manifest's directory,
        // so matching it against the CWD form left this warning silent
        // for every caller standing anywhere but the project root, which
        // is exactly when the override it announces happens (#1164).
        if let Some(glob) = self.excludes.first_match(seed, cwd_form) {
            warn(format_args!(
                "{} matches an exclude pattern ({glob}) \
                 but was named explicitly; analyzing anyway",
                seed.display()
            ));
        }
    }
}

/// What VCS ignore rules kept out of a walk, measured at the walk's
/// prune points (#1055): for every directory the walker visited, the
/// immediate children it did not keep, classified by what dropped them.
/// Only ignore rules remain once hidden entries, symlinks, exclude
/// globs, and unrecognized extensions are accounted for, so `files` is
/// exactly the analyzable files an ignore rule removed from the run.
///
/// An ignored *directory* is reported as one pruned entry and never
/// entered: enumerating its contents would walk the no-ignore universe
/// (a `target/` tree measured in the millions of entries here), and the
/// interesting fact — the walker did not go in — is already known.
#[derive(Debug, Default)]
pub(crate) struct IgnoredEntries {
    /// Analyzable files an ignore rule dropped, sorted.
    pub(crate) files: Vec<PathBuf>,
    /// Directories the walker did not enter because an ignore rule
    /// matched them, sorted. Contents unknown by design.
    pub(crate) dirs: Vec<PathBuf>,
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
    /// What ignore rules dropped, populated only by
    /// [`resolve_walk_files_with_ignored`] (empty otherwise): the gate
    /// consults it for the #1055 not-checked summary, and no other walk
    /// pays for the measurement.
    pub(crate) ignored: IgnoredEntries,
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
/// `tests/discovery/read_failures.rs`.
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
    materialize_paths_from(&mut paths, paths_from.as_deref());
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
    let mut walked_dirs: Vec<(PathBuf, Vec<PathBuf>)> = Vec::new();
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
            admit_file_seed(&seed, filters, &mut found);
            continue;
        }
        // The dedupe spans every seed, so it belongs to `found` rather
        // than to one seed's walk — a single walk cannot see the others.
        let walk = walk_directory_seed(&seed, no_ignore, threads, filters, &mut walk_errors);
        for path in walk.files {
            found.push_walked(path);
        }
        // `walk.dirs` is empty unless this walk measures (the visitor
        // gates the sends), so unconditional retention costs nothing.
        walked_dirs.push((seed, walk.dirs));
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
    let ignored = if filters.measure_ignored {
        measure_ignored_entries(&walked_dirs, &found, filters)
    } else {
        IgnoredEntries::default()
    };
    ResolvedFiles {
        files: found.files,
        explicit_files: found.explicit_files,
        walk_errors,
        ignored,
    }
}

/// Admit an explicitly-named file seed into `found`.
///
/// A single explicit file seed keeps the form the caller spelled (its
/// emitted `name` must match the single-file `bca.analyze()` API). It
/// bypasses the exclude deny-set: a file named directly on the command
/// line is a direct request that a project's directory-walk ignore
/// rules (`.bcaignore`, `--exclude`, manifest `exclude`) must not
/// silently drop (#726), matching the ripgrep/fd convention that an
/// explicit path overrides ignore rules. An `--include` allow-list
/// still narrows which named files are analyzed, matched against the
/// seed's CWD-relative form so `--include 'src/**'` treats
/// `--paths "$PWD/src/f.rs"` and `--paths src/f.rs` alike.
fn admit_file_seed(seed: &Path, filters: &WalkFilters<'_>, found: &mut SeedSet) {
    let include_form = walk_seed::file_seed_match_path(seed);
    // Gated on the admission so an overlapping seed warns once rather
    // than per mention (#1146).
    if filters.includes(&include_form) && found.push_explicit(seed) {
        filters.warn_exclude_overridden(seed, &include_form);
    }
}

/// Diff each walked directory's immediate children against what the
/// walk kept, leaving exactly the entries VCS ignore rules dropped
/// (#1055). See [`IgnoredEntries`] for the classification and for why
/// pruned directories are never entered.
///
/// One `read_dir` per walked directory — the same directories the walk
/// just listed, so the cost tracks the *kept* tree, not the no-ignore
/// universe. Read errors are skipped silently: the walk already warned
/// about anything it could not list, and this pass is advisory.
fn measure_ignored_entries(
    walked_dirs: &[(PathBuf, Vec<PathBuf>)],
    found: &SeedSet,
    filters: &WalkFilters<'_>,
) -> IgnoredEntries {
    let walked = WalkedSets {
        dirs: walked_dirs
            .iter()
            .flat_map(|(_, dirs)| dirs)
            .map(|dir| walk_seed::strip_cur_dir(dir))
            .collect(),
        files: found
            .files
            .iter()
            .map(|file| walk_seed::strip_cur_dir(file))
            .collect(),
        explicit_canon: found
            .explicit_files
            .iter()
            .filter_map(|path| path.canonicalize().ok())
            .collect(),
    };
    let mut ignored = IgnoredEntries::default();
    for (seed, dirs) in walked_dirs {
        for dir in dirs {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                classify_dropped_child(&entry, seed, filters, &walked, &mut ignored);
            }
        }
    }
    // Overlapping seeds can flag the same entry once per seed; one
    // mention per path, in a stable order for the `--report-skipped`
    // listing.
    dedupe_measured(&mut ignored.files);
    dedupe_measured(&mut ignored.dirs);
    ignored
}

/// Sort and dedupe one measured list in the `./`-stripped space, so the
/// two spellings overlapping seeds give one path (`-p .` yields
/// `./src/f.rs` where `-p src` yields `src/f.rs`) collapse to a single
/// mention instead of being counted twice. A *stable* sort keeps the
/// surviving spelling the first-seeded one rather than whichever walker
/// thread happened to report it.
fn dedupe_measured(paths: &mut Vec<PathBuf>) {
    paths.sort_by(|a, b| walk_seed::strip_cur_dir(a).cmp(walk_seed::strip_cur_dir(b)));
    paths.dedup_by(|a, b| walk_seed::strip_cur_dir(a) == walk_seed::strip_cur_dir(b));
}

/// What the walk produced, as membership sets: the directories it
/// entered and the files it kept. [`classify_dropped_child`] consults
/// both to tell "the walk saw this" from "an ignore rule dropped it".
///
/// Keyed on the `./`-stripped form, because a path's spelling is the
/// seed's and not the file's: `-p . -p build` records `build` as walked
/// while `.`'s prune point offers `./build`, and comparing those
/// verbatim reported an explicitly-walked directory as ignore-pruned.
struct WalkedSets<'a> {
    dirs: std::collections::HashSet<&'a Path>,
    files: std::collections::HashSet<&'a Path>,
    /// Explicitly-named file seeds, canonicalized: an explicit seed
    /// bypasses ignore rules (#726) and *was* analyzed, but its
    /// caller spelling — often absolute — need not match the walker
    /// form either verbatim or `./`-stripped, so membership is by
    /// canonical path. Seeds that fail to canonicalize (vanished
    /// mid-run) are simply absent, which errs toward reporting.
    explicit_canon: std::collections::HashSet<PathBuf>,
}

/// Classify one directory child the walk did not keep, recording it in
/// `ignored` when only an ignore rule can explain the drop. Hidden
/// entries and symlinks are what the walker skips unconditionally;
/// exclude globs and unrecognized extensions are the walk's own
/// filters; anything in `walked` the walk saw. What remains was pruned
/// by ignore rules.
fn classify_dropped_child(
    entry: &std::fs::DirEntry,
    seed: &Path,
    filters: &WalkFilters<'_>,
    walked: &WalkedSets<'_>,
    ignored: &mut IgnoredEntries,
) {
    // `DirEntry::file_type` does not follow symlinks, matching the
    // walk's `follow_links(false)`.
    let Ok(file_type) = entry.file_type() else {
        return;
    };
    // Hidden entries are dropped by `.hidden(true)` and symlinks by
    // `follow_links(false)` whether or not any ignore rule exists, so
    // neither carries an ignore signal.
    if walker_hides(entry) || file_type.is_symlink() {
        return;
    }
    let path = entry.path();
    // Both membership tests compare in the `./`-stripped space — see
    // [`WalkedSets`] for why the spelling cannot be trusted verbatim.
    if file_type.is_dir() {
        if !walked.dirs.contains(walk_seed::strip_cur_dir(&path)) {
            ignored.dirs.push(path);
        }
        return;
    }
    if !file_type.is_file() || walked.files.contains(walk_seed::strip_cur_dir(&path)) {
        return;
    }
    // A file no parser owns would have been read and skipped, not
    // checked, so it is not worth reporting as a gate bypass. The
    // walk's own include/exclude globs drop the file with ignore
    // handling off too, so they own their explanation; and a file the
    // caller named explicitly was analyzed despite the ignore rule
    // (#726), so accusing the run of skipping it would contradict the
    // violations it just printed.
    let checked_explicitly = !walked.explicit_canon.is_empty()
        && path
            .canonicalize()
            .is_ok_and(|canonical| walked.explicit_canon.contains(&canonical));
    if !checked_explicitly
        && filters.analyzable(&path)
        && filters.passes(&path, &walk_seed::match_path_for(seed, &path))
    {
        ignored.files.push(path);
    }
}

/// Mirror of the walker's `.hidden(true)` filter: a dot-prefixed name
/// on every platform, plus the hidden file attribute on Windows —
/// `ignore`'s hidden test checks both, and modelling only the dot rule
/// reported attribute-hidden files as ignore-dropped on Windows.
fn walker_hides(entry: &std::fs::DirEntry) -> bool {
    if entry.file_name().as_encoded_bytes().starts_with(b".") {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // FILE_ATTRIBUTE_HIDDEN from the Windows file-attribute set.
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        if entry
            .metadata()
            .is_ok_and(|md| md.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0)
        {
            return true;
        }
    }
    false
}

/// The two channels a walk visitor reports through: files that passed
/// the filters, and every directory the walker entered — the "what the
/// walker saw" base set [`measure_ignored_entries`] diffs against.
struct WalkSinks {
    files: crossbeam::channel::Sender<PathBuf>,
    dirs: crossbeam::channel::Sender<PathBuf>,
}

/// Handle one entry produced by [`walk_directory_seed`]'s parallel walker:
/// forward a matching file down `sinks`, or warn about (and tally) an error.
///
/// A free function rather than the closure body it was extracted from, because
/// `ignore`'s `build_parallel().run(|| Box::new(move |entry| …))` API nests the
/// visitor two closures deep. Cognitive complexity charges a nesting increment
/// per enclosing lambda, so four decisions scored 16 there against 5 here — and
/// the visitor reads better with a name than buried inside the builder setup.
fn visit_walk_entry(
    entry: Result<ignore::DirEntry, ignore::Error>,
    seed: &Path,
    filters: &WalkFilters<'_>,
    sinks: &WalkSinks,
    io_errors: &AtomicUsize,
) {
    // A per-entry error skips that entry rather than aborting the run (#704):
    // a single EACCES directory deep in a large tree must not take down every
    // file the walk has yet to reach.
    let entry = match entry {
        Ok(entry) => entry,
        Err(e) => {
            warn(format_args!(
                "skipping walk entry in {}: {e}",
                seed.display()
            ));
            // Every variant warns; only an I/O-backed one is tallied, because
            // only that one means the walk lost files it should have seen
            // (#1131). See [`WalkErrors`] for why the rest stay non-fatal.
            if e.io_error().is_some() {
                io_errors.fetch_add(1, Ordering::Relaxed);
            }
            return;
        }
    };
    let Some(file_type) = entry.file_type() else {
        return;
    };
    if file_type.is_dir() {
        // Record every directory the walker entered — but only when
        // this walk measures ignore drops, so `bca metrics` over a
        // million-directory tree does not allocate and retain a
        // directory list nothing will read. An ignore-pruned directory
        // never reaches this visitor, so the recorded set is exactly
        // what the measurement needs to tell "walked" from "pruned"
        // without re-deriving any ignore decision.
        if filters.measure_ignored {
            let _ = sinks.dirs.send(entry.into_path());
        }
        return;
    }
    if !file_type.is_file() {
        return;
    }
    let path = entry.into_path();
    // Anchor the glob match to the walk root rather than the emitted (possibly
    // absolute) path, so `./`-anchored excludes match regardless of how the
    // seed resolved — including a manifest root above the CWD (#489).
    if filters.passes(&path, &walk_seed::match_path_for(seed, &path)) {
        // The receiver outlives the walk (it is drained by the caller), so
        // this cannot fail.
        let _ = sinks.files.send(path);
    }
}

/// Walk the directory `seed` with `threads` walker threads, returning every
/// supported file that passes the include/exclude `filters` (sorted)
/// plus the directories the walker entered.
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
) -> SeedWalk {
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
    // against each other. Directories ride their own channel so the
    // file drain below stays a plain `Vec<PathBuf>`.
    let (tx, rx) = crossbeam::channel::unbounded();
    let (dirs_tx, dirs_rx) = crossbeam::channel::unbounded();
    // The error tally is a shared counter rather than a second channel
    // item: a walk error yields no path, so widening `tx`'s item to an
    // enum would make every hot-path send pay for the rare case.
    let io_errors = AtomicUsize::new(0);
    wb.build_parallel().run(|| {
        let sinks = WalkSinks {
            files: tx.clone(),
            dirs: dirs_tx.clone(),
        };
        let io_errors = &io_errors;
        Box::new(move |entry| {
            visit_walk_entry(entry, seed, filters, &sinks, io_errors);
            WalkState::Continue
        })
    });
    // Drop the builder's own senders so the drains below terminate;
    // every visitor clone is already gone, `run` having joined its
    // threads.
    drop(tx);
    drop(dirs_tx);
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
    SeedWalk {
        files: found,
        dirs: dirs_rx.into_iter().collect(),
    }
}

/// One directory seed's walk: the files that passed the filters,
/// sorted, plus every directory the walker entered (unsorted — the
/// ignore measurement only needs membership).
struct SeedWalk {
    files: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
}

/// Resolve the seeds into the terminal, walk-root-anchored file list
/// plus the worker count. The include/exclude filtering is applied here,
/// anchored to each file's walk root (#489), so the resolved list is the
/// final file set: the library runner processes it as-is, with no second
/// walk and no second, emitted-path-form-sensitive glob match (the dead
/// library globsets and re-walk were removed in #495). This anchored
/// walk is the single filtering seam.
pub(crate) fn resolve_walk_files(globals: GlobalOpts) -> (ResolvedFiles, usize) {
    resolve_walk_files_inner(globals, false)
}

/// Like [`resolve_walk_files`], with the ignore-rule measurement on:
/// `ResolvedFiles::ignored` reports what ignore rules dropped. Only the
/// `bca check` gate calls this — the measurement re-lists every walked
/// directory, which no other command has a summary to spend it on.
pub(crate) fn resolve_walk_files_with_ignored(globals: GlobalOpts) -> (ResolvedFiles, usize) {
    resolve_walk_files_inner(globals, true)
}

fn resolve_walk_files_inner(globals: GlobalOpts, measure_ignored: bool) -> (ResolvedFiles, usize) {
    let include = mk_globset(globals.include).unwrap_or_else(|e| die(e));
    let exclude = build_exclude_globset(
        globals.exclude,
        globals.exclude_from.as_deref(),
        "--exclude-from",
    );
    // The manifest's own globs compile into their own set so they keep
    // the manifest directory as their anchor (#1164). `unwrap_or_default`
    // yields the no-manifest case an empty, never-matching set.
    let manifest_excludes = globals.manifest_excludes.unwrap_or_default();
    let manifest_exclude = build_exclude_globset(
        manifest_excludes.globs,
        manifest_excludes.globs_from.as_deref(),
        "bca.toml exclude_from",
    );
    let num_jobs = globals.num_jobs.resolve();
    // Read once, not per walked file: `passes` runs for every entry and
    // `current_dir()` is a syscall. It cannot be cached process-wide
    // either — `bca diff`'s directory guard moves the cwd — so once per
    // walk is the right granularity (#1189).
    let cwd = std::env::current_dir().unwrap_or_default();
    // An empty set never matches, so there is no anchor worth supplying —
    // and the no-manifest default above has no directory to offer in the
    // first place.
    let manifest_dir = (!manifest_exclude.is_empty()).then_some(manifest_excludes.dir.as_path());
    // Once per walk, not once per file: the exclude match runs for every
    // entry, and a project reached through a symlink would otherwise pay
    // two `canonicalize` syscalls per file inside `relative_tail`.
    let canonical_manifest_dir = manifest_dir.and_then(|dir| dir.canonicalize().ok());
    let filters = WalkFilters {
        include: &include,
        excludes: walk_seed::AnchoredExcludes::new(
            &exclude,
            &manifest_exclude,
            manifest_dir,
            &cwd,
            canonical_manifest_dir.as_deref(),
        ),
        language_forced: globals.language.is_some(),
        // With ignore handling off nothing can be ignore-dropped, so
        // the measurement short-circuits to empty rather than reporting
        // every hidden-or-excluded child as a mystery.
        measure_ignored: measure_ignored && !globals.no_ignore,
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
