//! Loading the two sides of a `bca diff`: a per-file JSON document, an
//! aggregate file of them, or a directory tree of either off disk, or the
//! `FuncSpace` trees a `--since` walk streamed back, into the
//! [`MetricSet`] the comparison in the parent module reads. The one thing
//! both routes must agree on is how a document becomes its pairing key,
//! and [`pairing_key`] is the single place that decides it (#1116).
//!
//! Every document the loader reads either pairs or is an error naming
//! it, never a gap: a document missing from one side is
//! indistinguishable from a file the other side added or removed, so
//! skipping one yields a wrong diff that exits `0` (the failure mode
//! #1098 closed for the `--since` walk). A directory side's documents are
//! its `*.json` files; any other file in it is not an input and is
//! passed over, and a directory holding no `*.json` file at all is an
//! error.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use super::{DiffError, MetricSet};
use crate::format_util::slash_path;

/// Where a loaded document came from, for an error that must name it: a
/// file (or, for the `--since` walk, the source path it streamed), or one
/// element of an aggregate file's top-level array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DocumentOrigin {
    pub(crate) path: PathBuf,
    pub(crate) element: Option<usize>,
}

impl DocumentOrigin {
    pub(crate) fn file(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            element: None,
        }
    }

    pub(crate) fn element(path: &Path, index: usize) -> Self {
        Self {
            path: path.to_path_buf(),
            element: Some(index),
        }
    }
}

impl std::fmt::Display for DocumentOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.path.display())?;
        match self.element {
            Some(index) => write!(f, "[{index}]"),
            None => Ok(()),
        }
    }
}

/// JSON key under which each per-file document nests its metric values.
const METRICS_KEY: &str = "metrics";

/// JSON key carrying a per-file document's display name: the source path
/// bca emitted, and the pairing identity of every document that has one,
/// whichever input layout it was read from.
const NAME_KEY: &str = "name";

/// Load a metric-output set from a file or directory: every `*.json`
/// under a directory, or what one file holds. A file holds either one
/// per-file document or, as `bca metrics -O json --output <file>` writes
/// it, an aggregate array of them. A document keys on its own `name`
/// field, so the old and new sides pair on the paths bca emitted whatever
/// their output layout; only a document directly in a directory file may
/// lack one (see [`load_dir_set`]).
pub(super) fn load_set(path: &Path) -> Result<MetricSet, DiffError> {
    if path.is_dir() {
        return load_dir_set(path);
    }
    let mut set = SetBuilder::default();
    load_file(&mut set, path, None)?;
    Ok(set.finish())
}

/// Load every JSON file under `root` into a [`MetricSet`]. Files of any
/// other extension are skipped: an output directory is the user's, and
/// may hold a README or a second capture in another format beside the
/// JSON one. A skipped file can never be a document `bca metrics -O json`
/// wrote, so skipping it cannot shorten a JSON capture; what it can do is
/// leave nothing at all (a YAML capture, an empty directory), which reads
/// as a tree whose every file was removed, or as two identical empty
/// sides, so a directory holding no `*.json` file is
/// [`DiffError::NoDocuments`]. A `*.json` file that parses to no document
/// is not: the aggregate `[]` that `bca metrics -O json --output` writes
/// for a walk matching nothing is a capture of an empty tree, and loads
/// as one here exactly as it does when passed as a file.
///
/// Each document is keyed by its own `name` field (the root-relative
/// source path bca emits), so a directory set pairs with a single-file
/// set and with the `--since` sides regardless of the `.json` output-file
/// suffix or output-dir layout. A file holding one document without a `name` falls
/// back to its path relative to `root`, which still pairs across two
/// directories of the same layout (`walk_json_files` only yields paths
/// under `root`, so `strip_prefix` cannot fail in practice). Reached
/// through [`load_set`]'s directory branch; the `--since` walk builds its
/// side with [`set_from_spaces`] instead.
pub(super) fn load_dir_set(root: &Path) -> Result<MetricSet, DiffError> {
    let files = walk_json_files(root)?;
    if files.is_empty() {
        return Err(DiffError::NoDocuments {
            path: root.to_path_buf(),
        });
    }
    let mut set = SetBuilder::default();
    for entry in files {
        let fallback = entry.strip_prefix(root).unwrap_or(&entry);
        load_file(&mut set, &entry, Some(fallback))?;
    }
    Ok(set.finish())
}

/// Read one JSON file into `set`: its one document, or each element of an
/// aggregate array. `fallback` is the key a single document without a
/// `name` takes; an array element has no path of its own to fall back
/// on, so it must carry a `name`.
fn load_file(set: &mut SetBuilder, path: &Path, fallback: Option<&Path>) -> Result<(), DiffError> {
    match read_json(path)? {
        Value::Array(documents) => {
            for (index, document) in documents.into_iter().enumerate() {
                let origin = DocumentOrigin::element(path, index);
                insert_document(set, document, origin, None)?;
            }
            Ok(())
        }
        document => insert_document(set, document, DocumentOrigin::file(path), fallback),
    }
}

/// Key one per-file JSON document into `set` and store its `metrics`
/// object. A value that is not an object carrying a `metrics` object is
/// not bca's per-file document (an `ops` result, a hand-written file),
/// and reading it as one with no metrics would report every field of the
/// other side's file as added or removed.
fn insert_document(
    set: &mut SetBuilder,
    document: Value,
    origin: DocumentOrigin,
    fallback: Option<&Path>,
) -> Result<(), DiffError> {
    let Value::Object(mut document) = document else {
        return Err(DiffError::NotADocument { origin });
    };
    let Some(metrics @ Value::Object(_)) = document.remove(METRICS_KEY) else {
        return Err(DiffError::NotADocument { origin });
    };
    let name = document.get(NAME_KEY).and_then(Value::as_str);
    let key = pairing_key(name, fallback, &origin)?;
    set.insert(key, metrics, origin)
}

/// The key a document pairs on: its `name`, else `fallback`, both
/// through [`path_to_key`] so the two spellings of one source path that
/// differ only in `.` components pair.
///
/// Shared by the on-disk loaders and [`set_from_spaces`] so the two
/// cannot drift on how a document becomes a pairing key (#1116). That
/// agreement is load-bearing: `bca diff --since` builds one side in
/// memory and `bca diff <old> <new>` reads the other off disk, and a key
/// derived differently would pair nothing.
///
/// `fallback` is a path that stays comparable across sides (the relative
/// path within a directory set, or the path a `--since` walk emitted).
/// Without one a nameless document is an error: keyed on the input file's
/// own path, `bca diff old.json new.json` would report one file removed
/// and another added instead of comparing them.
fn pairing_key(
    name: Option<&str>,
    fallback: Option<&Path>,
    origin: &DocumentOrigin,
) -> Result<String, DiffError> {
    match (name, fallback) {
        (Some(name), _) => path_to_key(Path::new(name)),
        (None, Some(path)) => path_to_key(path),
        (None, None) => Err(DiffError::Nameless {
            origin: origin.clone(),
        }),
    }
}

/// A [`MetricSet`] under construction, remembering where each key came
/// from so a second document claiming the same key is reported with both
/// origins rather than silently replacing the first.
#[derive(Default)]
struct SetBuilder {
    set: MetricSet,
    origins: BTreeMap<String, DocumentOrigin>,
}

impl SetBuilder {
    fn insert(
        &mut self,
        key: String,
        metrics: Value,
        origin: DocumentOrigin,
    ) -> Result<(), DiffError> {
        match self.origins.entry(key) {
            Entry::Occupied(first) => Err(DiffError::DuplicateName {
                name: first.key().clone(),
                first: first.get().clone(),
                second: origin,
            }),
            Entry::Vacant(slot) => {
                self.set.insert(slot.key().clone(), metrics);
                slot.insert(origin);
                Ok(())
            }
        }
    }

    fn finish(self) -> MetricSet {
        self.set
    }
}

/// Build a [`MetricSet`] from the `FuncSpace` trees a `--since` walk
/// streamed back, without a filesystem round-trip (#1116).
///
/// The in-memory twin of [`load_dir_set`], and deliberately keyed the
/// same way: the key is the space's `name` (the field the per-file JSON
/// writer emits as `name`) through [`pairing_key`], falling back to the
/// emitted path, which is what `name` is set from anyway, so the fallback
/// only matters for a space that somehow carries none. The value is
/// `serde_json::to_value` of the space's `metrics` alone: the same
/// `Serialize` impl the writer runs for that field, without projecting
/// and serializing the whole nested space tree only to keep one object
/// of it.
///
/// The old route wrote one JSON document per source file (each behind a
/// `create_dir_all`), then re-walked the temp tree and re-parsed every
/// document into a `Value`. Both sides of every `bca diff --since` paid
/// that twice over while the trees were already in memory.
pub(crate) fn set_from_spaces(
    items: impl IntoIterator<Item = crate::AggregateItem>,
) -> Result<MetricSet, DiffError> {
    let mut set = SetBuilder::default();
    for item in items {
        // `ops` never shares a walk with `metrics` — separate
        // subcommands — and `--since` only ever runs the metrics action,
        // so the `Ops` arm cannot occur here. Rejected rather than
        // skipped anyway: silently dropping an item would leave the file
        // absent from this side's set, and an absent file reads as one
        // the commit added or removed. That is the wrong-answer failure
        // mode the surrounding `UnreadableInputs` guard exists to
        // prevent (#1098), so it must not be reintroduced here by a
        // `continue`.
        let crate::AggregateItem::Metrics(space, path) = item else {
            return Err(DiffError::UnexpectedAggregate);
        };
        let metrics =
            serde_json::to_value(&space.metrics).map_err(|source| DiffError::Serialize {
                path: path.clone(),
                source,
            })?;
        let origin = DocumentOrigin::file(&path);
        let key = pairing_key(space.name.as_deref(), Some(&path), &origin)?;
        set.insert(key, metrics, origin)?;
    }
    Ok(set.finish())
}

fn read_json(path: &Path) -> Result<Value, DiffError> {
    let bytes = std::fs::read(path).map_err(|source| DiffError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| DiffError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// Convert a path into a stable string key, erroring on non-UTF-8 rather
/// than lossily transcoding (identifier paths must round-trip).
///
/// The key is lexical: every `.` component is dropped, so `-p ./src` and
/// `-p src` captures (`./src/a.rs`, `src/a.rs`) pair, and so do a
/// doubled or trailing separator. Nothing else is resolved: a `..` stays,
/// since collapsing it is only right when its parent is not a symlink,
/// and answering that would mean reading the filesystem the captures
/// were taken on, which may be another machine. The `/` normalisation is
/// then [`slash_path`]'s, so a key reads the same on every platform.
pub(super) fn path_to_key(path: &Path) -> Result<String, DiffError> {
    let lexical: PathBuf = path
        .components()
        .filter(|component| *component != Component::CurDir)
        .collect();
    slash_path(&lexical).ok_or_else(|| DiffError::NonUtf8Path {
        path: path.to_path_buf(),
    })
}

/// Recursively collect `*.json` files under a directory. Uses the same
/// `ignore` crate the walker uses elsewhere, but with ignore-file
/// awareness disabled: a metric-output dir is a build artifact, not a
/// source tree, so `.gitignore` rules must not hide its contents.
///
/// Symlinks follow the source walk's policy where it can apply and part
/// from it where it cannot. A symlinked *directory* is not entered, as
/// the source walk's `follow_links(false)` enters none, which also rules
/// out a link cycle. A `*.json` entry is resolved through any symlink and
/// read if it is a file; one that cannot be resolved (a dangling link, an
/// entry `stat` refuses) is a read error naming it. The source walk drops
/// such an entry silently, which only shrinks what it analyzes; here a
/// dropped document reads as a removed file.
fn walk_json_files(root: &Path) -> Result<Vec<PathBuf>, DiffError> {
    let mut files = Vec::new();
    let walker = ignore::WalkBuilder::new(root)
        .standard_filters(false)
        .follow_links(false)
        .build();
    for result in walker {
        let entry = result.map_err(|e| DiffError::Read {
            path: root.to_path_buf(),
            source: std::io::Error::other(e),
        })?;
        let p = entry.path();
        // A real directory named `*.json` is walked, not read.
        let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
        if is_dir
            || !p
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        {
            continue;
        }
        let target = std::fs::metadata(p).map_err(|source| DiffError::Read {
            path: p.to_path_buf(),
            source,
        })?;
        if target.is_file() {
            files.push(p.to_path_buf());
        }
    }
    files.sort();
    Ok(files)
}
