//! Loading the two sides of a `bca diff`: a per-file JSON document or a
//! directory tree of them off disk, or the `FuncSpace` trees a `--since`
//! walk streamed back, into the [`MetricSet`] the comparison in the
//! parent module reads. The one thing both routes must agree on is how a
//! document becomes its pairing key, and [`insert_document`] is the
//! single place that decides it (#1116).

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{DiffError, MetricSet};

/// JSON key under which each per-file document nests its metric values.
const METRICS_KEY: &str = "metrics";

/// JSON key carrying a per-file document's display name: the source path
/// bca emitted, and the pairing identity of every document that has one,
/// whichever input layout it was read from.
const NAME_KEY: &str = "name";

/// Load a metric-output set from a file or directory: every `*.json`
/// under a directory, or the one document a file holds. Either way a
/// document keys on its own `name` field through [`insert_document`],
/// falling back to a path — relative to the directory root, or the
/// file's own — only when `name` is absent, so the old and new sides
/// pair on the paths bca emitted whatever their output layout.
pub(super) fn load_set(path: &Path) -> Result<MetricSet, DiffError> {
    if path.is_dir() {
        return load_dir_set(path);
    }
    let mut set = MetricSet::new();
    insert_document(&mut set, read_json(path)?, path)?;
    Ok(set)
}

/// Load every per-file JSON document under `root` into a [`MetricSet`].
/// Each entry is keyed by its document's own `name` field — the
/// root-relative source path bca emits — so a directory set pairs with
/// a single-file set (which keys the same way) and with the
/// working-tree / `--since` sides regardless of the `.json` output-file
/// suffix or output-dir layout. Falls back to the path relative to
/// `root` when `name` is absent (`walk_json_files` only yields paths
/// under `root`, so `strip_prefix` cannot fail in practice). Reached
/// through [`load_set`]'s directory branch; the `--since` walk builds its
/// side with [`set_from_spaces`] instead.
pub(super) fn load_dir_set(root: &Path) -> Result<MetricSet, DiffError> {
    let mut set = MetricSet::new();
    for entry in walk_json_files(root)? {
        let value = read_json(&entry)?;
        insert_document(&mut set, value, entry.strip_prefix(root).unwrap_or(&entry))?;
    }
    Ok(set)
}

/// Key one per-file document into `set` and store its `metrics` object.
///
/// Shared by [`load_dir_set`] and [`set_from_spaces`] so the two cannot
/// drift on how a document becomes a pairing key (#1116). That agreement
/// is load-bearing: `bca diff --since` builds one side in memory and
/// `bca diff <old> <new>` reads the other off disk, and a key derived
/// differently would pair nothing.
///
/// `fallback` supplies the key only for a document with no `name` field
/// — the relative path within the set, so it stays comparable across
/// sides.
fn insert_document(set: &mut MetricSet, value: Value, fallback: &Path) -> Result<(), DiffError> {
    let key = value
        .get(NAME_KEY)
        .and_then(Value::as_str)
        .map_or_else(|| path_to_key(fallback), |name| Ok(name.to_owned()))?;
    set.insert(key, extract_metrics(value));
    Ok(())
}

/// Build a [`MetricSet`] from the `FuncSpace` trees a `--since` walk
/// streamed back, without a filesystem round-trip (#1116).
///
/// The in-memory twin of [`load_dir_set`], and deliberately keyed the
/// same way: `serde_json::to_value` runs the identical `Serialize` impl
/// the per-file JSON writer used, so the document this sees is the one
/// that used to be written out and parsed back. Keys still come from the
/// document's `name` field, falling back to the emitted path — which is
/// what `name` is set from anyway, so the fallback only matters for a
/// space that somehow carries none.
///
/// The old route wrote one JSON document per source file (each behind a
/// `create_dir_all`), then re-walked the temp tree and re-parsed every
/// document into a `Value`. Both sides of every `bca diff --since` paid
/// that twice over while the trees were already in memory.
pub(crate) fn set_from_spaces(
    items: impl IntoIterator<Item = crate::AggregateItem>,
) -> Result<MetricSet, DiffError> {
    let mut set = MetricSet::new();
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
        let value = serde_json::to_value(&*space).map_err(|source| DiffError::Serialize {
            path: path.clone(),
            source,
        })?;
        insert_document(&mut set, value, &path)?;
    }
    Ok(set)
}

/// Pull the top-level `metrics` object out of a per-file document,
/// defaulting to an empty object.
fn extract_metrics(value: Value) -> Value {
    value
        .get(METRICS_KEY)
        .cloned()
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()))
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
/// The OS separator is normalized to `/` so a directory-set's relpath
/// fallback key is platform-independent: a file at `nested/y.json` keys
/// identically whether the walk ran on Unix or Windows (where
/// `to_str()` would otherwise yield `nested\y.json`, breaking pairing
/// against a `/`-keyed set and the documented `bca diff` contract).
fn path_to_key(path: &Path) -> Result<String, DiffError> {
    path.to_str()
        .map(|s| s.replace(std::path::MAIN_SEPARATOR, "/"))
        .ok_or_else(|| DiffError::NonUtf8Path {
            path: path.to_path_buf(),
        })
}

/// Recursively collect `*.json` files under a directory. Uses the same
/// `ignore` crate the walker uses elsewhere, but with ignore-file
/// awareness disabled: a metric-output dir is a build artifact, not a
/// source tree, so `.gitignore` rules must not hide its contents.
fn walk_json_files(root: &Path) -> Result<Vec<PathBuf>, DiffError> {
    let mut files = Vec::new();
    let walker = ignore::WalkBuilder::new(root)
        .standard_filters(false)
        .build();
    for result in walker {
        let entry = result.map_err(|e| DiffError::Read {
            path: root.to_path_buf(),
            source: std::io::Error::other(e),
        })?;
        let p = entry.path();
        if p.is_file()
            && p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        {
            files.push(p.to_path_buf());
        }
    }
    files.sort();
    Ok(files)
}
