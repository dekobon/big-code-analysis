//! Path-scoped threshold overrides (`[[thresholds.path]]`).
//!
//! A `[thresholds.lang.<slug>]` table tunes a limit for a *language*;
//! this layer tunes it for a *population of files*. The case it exists
//! for is a directory of hand-rolled dispatch tables whose cyclomatic
//! count is the construct list and whose Halstead effort is the table's
//! vocabulary — a limit chosen for that population, not thirty copies
//! of one suppression marker.
//!
//! The module owns the two halves of the layer that the resolver does
//! not: the parse of each entry (`glob`, its origin directory, and its
//! `metric = limit` pairs), and the per-file selection that
//! [`PathLayer`] performs once the base tables are resolved. The
//! layering itself — soft tier derived from the overridden limit,
//! `--threshold` applied last — is deliberately *not* here: the layer
//! folds the matching entries into a copy of the chosen language's hard
//! map and hands that to the same resolver every other table goes
//! through, so there is one layering routine and it cannot drift.
//!
//! Two matching rules are inherited from `[check] exclude` rather than
//! restated. A glob is matched against the path relative to the
//! directory of the config that declared it (the manifest's, or
//! `--config`'s), with a file outside that tree falling back to its
//! seed-relative form — the walk-root-anchored spelling a
//! `[check] exclude` glob matches, so `-p src` presents `src/gen/a.c`
//! as `gen/a.c` to an entry declared outside the tree; and among
//! overlapping entries the *last* wins, per metric, in declaration
//! order — manifest entries first, `--config` entries after them.
//!
//! `tokens` is rejected here and accepted by a language table because
//! the two loosen different things. `tokens` is a resource cap on what
//! the walk will parse, the one limit a suppression marker cannot
//! silence; a language table restates that cap for every file of a
//! grammar, which is still a cap, while a path entry carves a
//! directory out from under it — the marker's exemption by another
//! route.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use globset::{Glob, GlobMatcher};

use crate::thresholds::{ThresholdSet, insert_canonical_limit, threshold_scalar};
use crate::walk_seed::{CwdForm, ManifestAnchor, manifest_match_path_via, strip_dot_slash};

/// Reserved key inside `[thresholds]` that introduces the path-scoped
/// override array (`[[thresholds.path]]`). Reserved alongside `soft`
/// and `lang` so a future metric can never be named into a collision.
pub(crate) const PATH_SUBTABLE_KEY: &str = "path";

/// The key inside one entry that carries its glob. Every other key is
/// a `metric = limit` pair.
pub(crate) const GLOB_KEY: &str = "glob";

/// One parsed `[[thresholds.path]]` entry.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PathThreshold {
    /// The glob exactly as written, so `--print-effective-config` can
    /// echo it and a diagnostic can name it.
    pub(crate) glob: String,
    /// `glob` compiled, with its optional leading `./` normalised away
    /// so `dir/**` and `./dir/**` are one pattern (#726).
    pattern: Glob,
    /// Directory of the config file that declared the entry. The glob
    /// is matched against paths relative to it, exactly as a
    /// `[check] exclude` glob is matched relative to its `bca.toml`.
    pub(crate) origin: PathBuf,
    /// The 1-based position of the entry within the config that
    /// declared it. Stamped at parse so every diagnostic names the
    /// entry the way its own file numbers it, whichever file it came
    /// from and wherever it lands in the merged list.
    pub(crate) ordinal: usize,
    /// The config file that declared the entry, named the way a
    /// diagnostic should name it — `bca.toml`, or the `--config` path
    /// as given. Where only one file declares entries a diagnostic
    /// stays as it always read; with two, this is what tells them apart.
    pub(crate) source: String,
    /// The overridden limits, keyed by canonical metric id.
    pub(crate) limits: BTreeMap<String, f64>,
}

impl PathThreshold {
    /// How diagnostics name an entry: its 1-based ordinal within the
    /// config that declared it plus the glob, because an array entry
    /// has no key of its own. The declaring file is *not* part of this
    /// form: the manifest's own errors are already prefixed `bca.toml:`
    /// by the caller, and a `--config` file is named only where its
    /// entries are listed beside the manifest's ([`Self::context`]).
    fn context_in_file(ordinal: usize, glob: &str) -> String {
        format!("[[thresholds.path]] #{ordinal} (glob {glob:?})")
    }

    /// How the resolver and `--explain-threshold` name an entry when
    /// the merged list may hold entries from two files: the in-file
    /// form, qualified with the declaring file.
    pub(crate) fn context(&self) -> String {
        format!(
            "{} in {}",
            Self::context_in_file(self.ordinal, &self.glob),
            self.source
        )
    }
}

/// Parse the `[[thresholds.path]]` array declared by the config at
/// `origin`, which diagnostics name as `source`.
///
/// Each entry must carry a non-empty `glob` string and at least one
/// `metric = <number>` pair. The glob is compiled here so a malformed
/// pattern is a config error naming the entry rather than a crash at
/// resolution; and a pattern that normalises to nothing (`""`, `./`) is
/// rejected rather than silently skipped, which is what the shared glob
/// builder would do with it.
///
/// `tokens` is not overridable. It is the one metric a suppression
/// marker cannot silence — a hard resource cap on what the walk will
/// parse — and a path entry is the same loosening by another route.
pub(crate) fn parse_path_entries(
    value: &toml::Value,
    origin: &Path,
    source: &str,
) -> Result<Vec<PathThreshold>, String> {
    let entries = value.as_array().ok_or_else(|| {
        "[thresholds.path] must be an array of tables (`[[thresholds.path]]`), each with a \
         `glob` key and `metric = <number>` entries"
            .to_owned()
    })?;
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| parse_one_entry(index + 1, entry, origin, source))
        .collect()
}

fn parse_one_entry(
    ordinal: usize,
    entry: &toml::Value,
    origin: &Path,
    source: &str,
) -> Result<PathThreshold, String> {
    let table = entry.as_table().ok_or_else(|| {
        format!("[[thresholds.path]] #{ordinal}: expected a table with a `glob` key")
    })?;
    let glob = entry_glob(ordinal, table)?;
    let context = PathThreshold::context_in_file(ordinal, glob);
    let pattern =
        Glob::new(strip_dot_slash(glob)).map_err(|e| format!("{context}: invalid glob: {e}"))?;
    let limits = entry_limits(&context, table)?;
    Ok(PathThreshold {
        glob: glob.to_owned(),
        pattern,
        origin: origin.to_path_buf(),
        ordinal,
        source: source.to_owned(),
        limits,
    })
}

/// The entry's `glob`, rejecting an absent, non-string, or empty one.
fn entry_glob(ordinal: usize, table: &toml::Table) -> Result<&str, String> {
    let glob = match table.get(GLOB_KEY) {
        Some(toml::Value::String(glob)) => glob.as_str(),
        Some(other) => {
            return Err(format!(
                "[[thresholds.path]] #{ordinal}: `glob` must be a string, got {}",
                other.type_str()
            ));
        }
        None => {
            return Err(format!(
                "[[thresholds.path]] #{ordinal}: missing the `glob` key naming the files it \
                 applies to"
            ));
        }
    };
    if strip_dot_slash(glob).is_empty() {
        return Err(format!(
            "{}: the glob is empty once its leading `./` is removed, so it would match nothing",
            PathThreshold::context_in_file(ordinal, glob)
        ));
    }
    Ok(glob)
}

/// The entry's `metric = limit` pairs, canonically keyed.
fn entry_limits(context: &str, table: &toml::Table) -> Result<BTreeMap<String, f64>, String> {
    let mut limits = BTreeMap::new();
    for (name, value) in table.iter().filter(|(name, _)| *name != GLOB_KEY) {
        if name == "tokens" {
            return Err(format!(
                "{context}: `tokens` cannot be overridden per path; it is a resource cap on \
                 what the walk parses, not a maintainability limit"
            ));
        }
        let limit = threshold_scalar(context, name, value)?;
        insert_canonical_limit(&mut limits, context, name, limit)?;
    }
    if limits.is_empty() {
        return Err(format!(
            "{context}: no `metric = <limit>` entries; an entry that overrides nothing would \
             silently do nothing"
        ));
    }
    Ok(limits)
}

/// The resolver the layer hands a folded hard map to. Implemented by the
/// check command's `SharedLayers`, which owns the tier and `--threshold`
/// layering; the trait keeps that type private to its module while
/// letting this one resolve lazily.
pub(crate) trait HardResolver: Send + Sync {
    /// Resolve one hard table through the tier and CLI layers.
    /// `context` names the table a build error should blame.
    fn resolve(&self, hard: BTreeMap<String, f64>, context: &str) -> ThresholdSet;
}

/// One entry compiled for matching: the glob, which origin anchors it,
/// and the limits it contributes.
#[derive(Clone)]
struct CompiledEntry {
    /// The glob as written, for `--print-effective-config`.
    glob: String,
    matcher: GlobMatcher,
    /// Index into [`PathLayer::origins`]: the directory the glob is
    /// matched relative to. An index rather than the path so `for_file`
    /// derives each file's match form once per *origin* and not once
    /// per entry — every entry of one config shares an origin.
    origin: usize,
    limits: BTreeMap<String, f64>,
    context: String,
}

/// One directory entries are anchored at — a config file's parent —
/// with its symlink-resolved form hoisted out of the per-file path.
#[derive(Clone)]
struct Origin {
    dir: PathBuf,
    /// `dir` resolved through symlinks once, so a project reached
    /// through one does not pay two syscalls per walked file.
    canonical: Option<PathBuf>,
}

/// The canonical form of each walked file's parent directory, memoised.
///
/// A file lexically outside an origin — every file, when the `--config`
/// that declared the entries lives outside the walked tree — can only
/// be matched after a `canonicalize`, which was one `readlinkat` per
/// path component per file per entry (#1746). The directory is what
/// the syscall resolves; the file name is appended lexically. A walk
/// visits each directory's files together, so this is one syscall per
/// directory rather than per file. A symlink *inside* the directory
/// chain still resolves, which is what the syscall exists for. A file
/// that is itself a symlink is canonicalised whole instead, so a seeded
/// `alias.c -> project/src/target.c` is matched as the target's path
/// and reaches the entry's `src/**` as it did before the memo. Only an
/// explicitly seeded file can be one — the walk follows no link and
/// yields none — so the `symlink_metadata` probe runs for
/// [`Admission::Explicit`] alone: one syscall per seeded file, never
/// per walked file.
#[derive(Default)]
struct CanonicalDirs(Mutex<BTreeMap<PathBuf, Option<PathBuf>>>);

impl CanonicalDirs {
    fn canonical(&self, path: &Path, admission: Admission) -> Option<PathBuf> {
        if matches!(admission, Admission::Explicit)
            && path
                .symlink_metadata()
                .is_ok_and(|meta| meta.file_type().is_symlink())
        {
            return path.canonicalize().ok();
        }
        let dir = self.canonical_dir(path.parent()?)?;
        Some(dir.join(path.file_name()?))
    }

    /// `dir` resolved through symlinks, from the memo or by one syscall
    /// chain that the memo then records.
    fn canonical_dir(&self, dir: &Path) -> Option<PathBuf> {
        let Ok(mut dirs) = self.0.lock() else {
            return dir.canonicalize().ok();
        };
        dirs.entry(dir.to_path_buf())
            .or_insert_with(|| dir.canonicalize().ok())
            .clone()
    }
}

/// How a file reached the walk: named on the command line (or in a
/// manifest's `paths`), or found by expanding a directory seed.
///
/// The path layer's one use is the leaf-symlink probe in
/// [`CanonicalDirs`]: the walk runs with `follow_links(false)` and
/// drops every non-file entry, so only an explicit seed can be a link
/// to a file, and only it pays the `symlink_metadata` syscall.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Admission {
    /// The caller named the file; it may be a symlink the walk resolved
    /// once at seed classification.
    Explicit,
    /// A directory walk produced the file; never a symlink.
    Walked,
}

/// A base table the matched entries fold onto: the global hard map, or
/// one language's already-extended map.
#[derive(Clone)]
pub(crate) struct BaseTable {
    pub(crate) hard: BTreeMap<String, f64>,
    /// The table to blame in a build error; `None` for the global one.
    pub(crate) context: Option<String>,
}

/// The memo key: which base table, and which entries matched, in order.
/// The path itself is never a key — the product of (language, matched
/// entry set) is tiny, and keying on it keeps a non-UTF-8 path out of
/// any string conversion.
type MemoKey = (Option<&'static str>, Vec<usize>);

/// The path-scoped layer of a resolved gate: every entry compiled, every
/// base table it can fold onto, and the resolved sets memoised per
/// `(base, matched entries)`.
pub(crate) struct PathLayer {
    entries: Vec<CompiledEntry>,
    /// The distinct directories entries are anchored at, in first-use
    /// order: the manifest's, then the `--config` file's.
    origins: Vec<Origin>,
    bases: BTreeMap<Option<&'static str>, BaseTable>,
    resolver: Arc<dyn HardResolver>,
    /// Ordered, not hashed: `resolved_sets` feeds `all_sets`, whose
    /// order decides which of two equal-metric advisories
    /// `warn_limit_advisories` prints first, and a run's stderr must not
    /// vary with the hash seed (#1091).
    memo: Mutex<BTreeMap<MemoKey, Arc<ThresholdSet>>>,
    /// The working directory every relative path is completed against,
    /// read once rather than per file.
    cwd: PathBuf,
    canonical_dirs: CanonicalDirs,
    /// `--explain-threshold` narrows the gate to the explained metrics
    /// after resolution; a set resolved lazily afterwards must be
    /// narrowed the same way.
    keep: Option<BTreeSet<String>>,
}

impl std::fmt::Debug for PathLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PathLayer")
            .field("entries", &self.entries.len())
            .field("bases", &self.bases.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl PathLayer {
    /// Compile the entries and validate every reachable combination.
    ///
    /// `bases` is the global hard map under `None` plus each language's
    /// extended map under its slug — the same maps the base sets were
    /// resolved from. Validation is eager and exhaustive on purpose: the
    /// resolver dies on a config error (an absolute soft limit looser
    /// than the hard one, a scale-relative soft entry with no base), and
    /// a lazy first resolution would die inside a worker thread halfway
    /// through the walk. Every such error is a property of one metric's
    /// hard value against one base, and in any match set a metric's
    /// value comes from exactly one entry, so resolving each single entry
    /// over each base here surfaces every error the walk could hit. The
    /// results seed the memo, and the sets they produce are what
    /// `selected_metrics` unions so a path-only metric is computed.
    pub(crate) fn new(
        entries: &[PathThreshold],
        bases: BTreeMap<Option<&'static str>, BaseTable>,
        resolver: Arc<dyn HardResolver>,
    ) -> Self {
        let mut origins: Vec<Origin> = Vec::new();
        let entries = entries
            .iter()
            .map(|entry| {
                let origin = origins
                    .iter()
                    .position(|o| o.dir == entry.origin)
                    .unwrap_or_else(|| {
                        origins.push(Origin {
                            canonical: entry.origin.canonicalize().ok(),
                            dir: entry.origin.clone(),
                        });
                        origins.len() - 1
                    });
                CompiledEntry {
                    glob: entry.glob.clone(),
                    matcher: entry.pattern.compile_matcher(),
                    origin,
                    limits: entry.limits.clone(),
                    context: entry.context(),
                }
            })
            .collect();
        let layer = Self {
            entries,
            origins,
            bases,
            resolver,
            memo: Mutex::new(BTreeMap::new()),
            cwd: std::env::current_dir().unwrap_or_default(),
            canonical_dirs: CanonicalDirs::default(),
            keep: None,
        };
        let slugs: Vec<Option<&'static str>> = layer.bases.keys().copied().collect();
        for slug in slugs {
            for index in 0..layer.entries.len() {
                layer.resolved_for(slug, vec![index]);
            }
        }
        layer
    }

    /// Whether the layer carries any entry at all.
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Each entry's glob with its declared metrics at their *hard-tier*
    /// limit — `--threshold` applied last, the tier not applied — in
    /// declaration order. This is the form `--print-effective-config`
    /// prints: an entry printed as declared beside a global row that
    /// already carries the `--threshold` value reads back through
    /// `--config` with the entry *over* the CLI limit, reversing the
    /// precedence of the run it was printed from (#1746).
    ///
    /// The hard tier, not the resolved tier, because a path row cannot
    /// carry a soft band losslessly: the band is a property of the
    /// *match set*, not the entry. A second entry naming only another
    /// metric flips the soft table into merge mode for the first, so
    /// the band an entry would print alone is not the band a file
    /// matching both is gated at; and a printed band read back under
    /// any `--tier` is scaled or ceilinged a second time. The hard limit
    /// is what every band is derived from, so loading the printout with
    /// the same `--tier` the run used re-derives the same gate. The
    /// global and language rows print tier-resolved, as they always
    /// have.
    ///
    /// Resolved over the global base so the CLI layer is applied by the
    /// same resolver every table goes through; the entry's own metrics
    /// take the same value on every base.
    pub(crate) fn resolved_rows(&self) -> impl Iterator<Item = (&str, BTreeMap<String, f64>)> {
        self.entries.iter().enumerate().map(|(index, entry)| {
            let set = self.resolved_for(None, vec![index]);
            let limits = set
                .hard_limits()
                .filter(|(name, _)| entry.limits.contains_key(*name))
                .map(|(name, limit)| (name.to_owned(), limit))
                .collect();
            (entry.glob.as_str(), limits)
        })
    }

    /// The set gating a file at `path` under the base `slug`, or `None`
    /// when no entry matches and the caller should use the base set.
    ///
    /// `slug` must be a base the layer was built with — the caller maps
    /// a language without a table of its own to `None`, the global
    /// base, exactly as `for_language` falls back. `cwd_form` is the
    /// file's seed-relative spelling — anchored at the walk seed that
    /// produced it, exactly as `[check] exclude` matches it — and is
    /// what a file outside an entry's origin tree is matched as. So
    /// under `-p src` an entry declared outside the tree sees
    /// `src/gen/a.c` as `gen/a.c`, and its `src/**` does not match.
    ///
    /// The match form is derived once per origin, not once per entry:
    /// every entry of one config shares its directory, and deriving it
    /// per entry multiplied the outside-origin `canonicalize` by the
    /// entry count (#1746). `admission` says whether the file can be a
    /// symlink at all — see [`Admission`].
    pub(crate) fn for_file(
        &self,
        slug: Option<&'static str>,
        path: &Path,
        cwd_form: CwdForm<'_>,
        admission: Admission,
    ) -> Option<Arc<ThresholdSet>> {
        let forms: Vec<Cow<'_, Path>> = self
            .origins
            .iter()
            .map(|origin| {
                let anchor = ManifestAnchor::resolve(
                    Some(&origin.dir),
                    &self.cwd,
                    origin.canonical.as_deref(),
                );
                manifest_match_path_via(anchor, path, cwd_form, |p| {
                    self.canonical_dirs.canonical(p, admission)
                })
            })
            .collect();
        let matched: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.matcher.is_match(&forms[entry.origin]))
            .map(|(index, _)| index)
            .collect();
        if matched.is_empty() {
            return None;
        }
        Some(self.resolved_for(slug, matched))
    }

    /// Resolve (or fetch) the set for `matched` folded onto `slug`'s base.
    ///
    /// A miss resolves outside the lock, so two workers that miss the
    /// same key at once both resolve it. That is harmless — the resolver
    /// is a pure function of the key, and whichever insert lands second
    /// replaces an equal set — and it keeps the resolver, which can
    /// `die`, from ever running under the mutex.
    fn resolved_for(&self, slug: Option<&'static str>, matched: Vec<usize>) -> Arc<ThresholdSet> {
        let key = (slug, matched);
        if let Some(set) = self
            .memo
            .lock()
            .ok()
            .and_then(|memo| memo.get(&key).cloned())
        {
            return set;
        }
        let (hard, context) = self.fold(slug, &key.1);
        let mut set = self.resolver.resolve(hard, &context);
        if let Some(keep) = &self.keep {
            set = set.narrowed_to(&keep.iter().map(String::as_str).collect());
        }
        let set = Arc::new(set);
        if let Ok(mut memo) = self.memo.lock() {
            memo.insert(key, Arc::clone(&set));
        }
        set
    }

    /// The hard map `matched` folds to over `slug`'s base, and the name a
    /// build error blames: the base's table plus each entry, joined.
    ///
    /// Per-metric last-match: fold in declaration order, each entry's
    /// `extend` overwriting the metrics it names and leaving the rest.
    fn fold(
        &self,
        slug: Option<&'static str>,
        matched: &[usize],
    ) -> (BTreeMap<String, f64>, String) {
        let base = self.bases.get(&slug).or_else(|| self.bases.get(&None));
        let mut hard = base.map(|b| b.hard.clone()).unwrap_or_default();
        let mut context: Vec<&str> = base
            .and_then(|b| b.context.as_deref())
            .into_iter()
            .collect();
        for entry in matched.iter().map(|&index| &self.entries[index]) {
            hard.extend(entry.limits.iter().map(|(k, v)| (k.clone(), *v)));
            context.push(&entry.context);
        }
        (hard, context.join(" + "))
    }

    /// Every set resolved so far. After construction that is at least
    /// one per `(base, entry)` pair, which is what makes the union a
    /// complete answer for metric selection and limit advisories.
    pub(crate) fn resolved_sets(&self) -> Vec<Arc<ThresholdSet>> {
        self.memo
            .lock()
            .map(|memo| memo.values().cloned().collect())
            .unwrap_or_default()
    }

    /// A copy narrowed to `keep`: the memoised sets are narrowed now,
    /// and anything resolved later is narrowed on the way in.
    pub(crate) fn narrowed_to(&self, keep: &BTreeSet<&str>) -> Self {
        let narrowed: BTreeMap<MemoKey, Arc<ThresholdSet>> = self
            .memo
            .lock()
            .map(|memo| {
                memo.iter()
                    .map(|(key, set)| (key.clone(), Arc::new(set.narrowed_to(keep))))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            entries: self.entries.clone(),
            origins: self.origins.clone(),
            bases: self.bases.clone(),
            resolver: Arc::clone(&self.resolver),
            memo: Mutex::new(narrowed),
            cwd: self.cwd.clone(),
            canonical_dirs: CanonicalDirs::default(),
            keep: Some(keep.iter().map(|s| (*s).to_owned()).collect()),
        }
    }

    /// The entries declaring `metric`: each one's diagnostic name and
    /// the hard limit it declares — what `--explain-threshold` lists
    /// per entry.
    ///
    /// A *declaration*, deliberately not a limit its files are gated
    /// at. Entries overlap, and where two match one file the later
    /// one's value is the limit; a second entry naming only another
    /// metric can still flip the soft tier's mode for the first. The
    /// limit a given file actually resolves to is a property of its
    /// match set, which only the walk produces; the report states what
    /// each entry declares and lets the reader compose the overlap.
    pub(crate) fn overrides_of(&self, metric: &str) -> Vec<PathOverride> {
        self.entries
            .iter()
            .filter_map(|entry| {
                Some(PathOverride {
                    context: entry.context.clone(),
                    hard: *entry.limits.get(metric)?,
                })
            })
            .collect()
    }
}

/// One `[[thresholds.path]]` entry's declaration of a metric, for the
/// `--explain-threshold` report.
pub(crate) struct PathOverride {
    /// The entry as diagnostics name it (ordinal, glob and file).
    pub(crate) context: String,
    /// The hard limit the entry declares.
    pub(crate) hard: f64,
}

#[cfg(test)]
// Threshold limits are exact `f64` config values, never computed, so
// comparing them is the contract rather than a float-precision hazard.
#[allow(clippy::float_cmp)]
#[path = "threshold_path_tests.rs"]
mod tests;
