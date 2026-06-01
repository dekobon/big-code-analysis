//! Per-metric structured diff between two `bca metrics -O json` runs
//! (issue #487). Replaces the legacy grammar-bump glue chain — the
//! external `json-minimal-tests` binary plus `split-minimal-tests.py` —
//! with a native command that buckets per-file metric deltas by metric
//! name, exactly the artifact `split-minimal-tests.py` produced (one
//! "directory" of changed files per metric).
//!
//! ## What is compared
//!
//! Each side is either a single per-file JSON document or a directory
//! tree of them (the shape `bca metrics -O json --output <dir>` writes).
//! Inputs are parsed as raw [`serde_json::Value`] rather than the
//! library's [`big_code_analysis::FuncSpace`] type: `FuncSpace` /
//! `CodeMetrics` derive only `Serialize` (with a *custom* `CodeMetrics`
//! impl), so there is no `Deserialize` to reuse. Walking the JSON tree
//! keeps the diff in lock-step with whatever shape the emitter produces
//! — a new metric field shows up automatically without a code change
//! here.
//!
//! Only the file-level (top-level) `metrics` object is diffed. A grammar
//! change that shifts any nested function's value ripples into the
//! file-level aggregate, so the file-level view answers the operative
//! question — "which files moved for metric X?" — without the
//! combinatorial nested-space matching that `json-minimal-tests` did.
//!
//! ## Bucketing taxonomy
//!
//! Buckets are named after the metric names `bca list-metrics` prints,
//! sourced from the library's canonical
//! [`big_code_analysis::metric_catalog::FAMILIES`]. Every family yields
//! one bucket named after the family, except `loc`, which expands to its
//! sub-metric rows (`sloc`, `ploc`, `lloc`, `cloc`, `blank`) — matching
//! `list-metrics` exactly. A file lands in a bucket when any scalar leaf
//! under that metric's JSON subtree changed by at least the configured
//! threshold.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use big_code_analysis::metric_catalog::FAMILIES;
use serde::Serialize;
use serde_json::Value;

use crate::format_util::MetricScalar;

/// JSON key under which each per-file document nests its metric values.
const METRICS_KEY: &str = "metrics";

/// JSON key carrying a per-file document's display name (used as the
/// pairing identity when a single file — not a directory — is diffed).
const NAME_KEY: &str = "name";

/// Family name whose rows expand into distinct buckets (the only family
/// `list-metrics` does not surface under its own name). Every other
/// family contributes a single bucket named after the family.
const EXPANDED_FAMILY: &str = "loc";

/// Error surfaced while loading or diffing the two metric-output sets.
/// Rendered by the caller as a tool error (exit 1); the diff itself
/// always exits 0 on success.
#[derive(Debug)]
pub(crate) enum DiffError {
    /// A path could not be read (missing, permission denied, …).
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    /// A file under a metric-output set was not valid JSON.
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    /// A path component was not valid UTF-8, so it cannot serve as a
    /// stable pairing key. Identifier paths must round-trip losslessly
    /// (no `to_string_lossy`), so this is a hard error rather than a
    /// silent rename.
    NonUtf8Path { path: PathBuf },
}

impl std::fmt::Display for DiffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(f, "failed to read {}: {source}", path.display())
            }
            Self::Parse { path, source } => {
                write!(f, "failed to parse JSON {}: {source}", path.display())
            }
            Self::NonUtf8Path { path } => {
                write!(f, "path is not valid UTF-8: {}", path.display())
            }
        }
    }
}

/// A single per-file, per-metric-field delta. `field` is the dotted
/// path to the scalar within the metric subtree (e.g. `sum`,
/// `modified.sum`) so a reviewer can see *which* component moved.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct FieldDelta {
    pub(crate) file: String,
    pub(crate) field: String,
    pub(crate) old: f64,
    pub(crate) new: f64,
}

/// All deltas for one metric bucket, plus the files that appeared or
/// vanished between the two sets (a file is added/removed wholesale, not
/// per metric, so those lists are deliberately shared across buckets via
/// [`MetricDiff::added_files`] / [`removed_files`]).
#[derive(Debug, Default, Serialize)]
pub(crate) struct Bucket {
    pub(crate) changed: Vec<FieldDelta>,
}

/// The complete per-metric diff: a bucket per metric name that has at
/// least one change, plus the set-level added/removed file lists. Counts
/// in the summary line always reflect every bucket.
#[derive(Debug, Default, Serialize)]
pub(crate) struct MetricDiff {
    /// Buckets keyed by metric name, ordered for deterministic output.
    pub(crate) buckets: BTreeMap<String, Bucket>,
    /// Files present in the new set but absent from the old.
    pub(crate) added_files: Vec<String>,
    /// Files present in the old set but absent from the new.
    pub(crate) removed_files: Vec<String>,
}

/// A loaded metric-output set: a map from pairing key (relative path for
/// a directory, the document `name` for a single file) to the parsed
/// top-level `metrics` object for that file.
pub(crate) type MetricSet = BTreeMap<String, Value>;

impl MetricDiff {
    /// Load both sides, then compute the bucketed diff. `min_change` is
    /// the inclusive absolute-delta threshold (`0.0` reports any
    /// change); `metric_filter`, when non-empty, restricts buckets to
    /// the named metrics (`bca list-metrics` names).
    pub(crate) fn compute(
        old_path: &Path,
        new_path: &Path,
        min_change: f64,
        metric_filter: &[String],
    ) -> Result<Self, DiffError> {
        let old = load_set(old_path)?;
        let new = load_set(new_path)?;
        Ok(Self::from_sets(&old, &new, min_change, metric_filter))
    }

    /// Diff two already-loaded sets. Split out from [`compute`] so tests
    /// can drive synthetic sets without touching the filesystem, and so
    /// the `--since` path (which materializes both sides in-memory from
    /// metric walks rather than from on-disk JSON sets) can reuse the
    /// exact same bucketing.
    pub(crate) fn from_sets(
        old: &MetricSet,
        new: &MetricSet,
        min_change: f64,
        metric_filter: &[String],
    ) -> Self {
        let mut diff = Self::default();

        for key in new.keys() {
            if !old.contains_key(key) {
                diff.added_files.push(key.clone());
            }
        }
        for key in old.keys() {
            if !new.contains_key(key) {
                diff.removed_files.push(key.clone());
            }
        }
        diff.added_files.sort();
        diff.removed_files.sort();

        // Files present on both sides: walk the metric tree and bucket
        // each scalar leaf whose value moved past the threshold.
        for (key, old_metrics) in old {
            let Some(new_metrics) = new.get(key) else {
                continue;
            };
            for (bucket, field, old_v, new_v) in metric_scalar_pairs(old_metrics, new_metrics) {
                if !metric_filter.is_empty() && !metric_filter.iter().any(|m| m == &bucket) {
                    continue;
                }
                // A field that did not move is never interesting (even
                // at the `min_change == 0` default, which means "any
                // change", not "every field"). A positive `min_change`
                // additionally suppresses sub-threshold movement.
                let delta = (new_v - old_v).abs();
                if delta == 0.0 || delta < min_change {
                    continue;
                }
                diff.buckets
                    .entry(bucket)
                    .or_default()
                    .changed
                    .push(FieldDelta {
                        file: key.clone(),
                        field,
                        old: old_v,
                        new: new_v,
                    });
            }
        }

        for bucket in diff.buckets.values_mut() {
            bucket
                .changed
                .sort_by(|a, b| (&a.file, &a.field).cmp(&(&b.file, &b.field)));
        }
        diff
    }

    /// Total field-level changes across every bucket.
    fn total_changes(&self) -> usize {
        self.buckets.values().map(|b| b.changed.len()).sum()
    }

    /// One-line headline reported by every renderer.
    fn summary_line(&self) -> String {
        format!(
            "{} metric(s) changed, {} added file(s), {} removed file(s)",
            self.buckets.len(),
            self.added_files.len(),
            self.removed_files.len(),
        )
    }

    /// Human-readable, column-aligned form for a terminal. Empty
    /// sections are omitted; an all-empty diff renders just the summary
    /// line.
    pub(crate) fn render_tty(&self) -> String {
        let mut out = self.summary_line();
        out.push('\n');
        Self::write_file_section(&mut out, "Added files", &self.added_files);
        Self::write_file_section(&mut out, "Removed files", &self.removed_files);
        for (metric, bucket) in &self.buckets {
            let _ = write!(out, "\n## {metric} ({} change(s))\n", bucket.changed.len());
            let width = bucket
                .changed
                .iter()
                .map(|d| d.file.chars().count() + d.field.chars().count() + 1)
                .max()
                .unwrap_or(0);
            for d in &bucket.changed {
                let id = format!("{}.{}", d.file, d.field);
                let _ = writeln!(
                    out,
                    "  {id:<width$}  {} \u{2192} {}",
                    MetricScalar(d.old),
                    MetricScalar(d.new),
                );
            }
        }
        out
    }

    /// Markdown form for a sticky PR comment: the summary line, then a
    /// `## Section` header per non-empty bucket with its rows in a fenced
    /// `text` block so column alignment survives Markdown's whitespace
    /// collapsing.
    pub(crate) fn render_markdown(&self) -> String {
        let mut out = self.summary_line();
        out.push('\n');
        if !self.added_files.is_empty() {
            let _ = write!(out, "\n## Added files\n\n");
            for f in &self.added_files {
                let _ = writeln!(out, "- {f}");
            }
        }
        if !self.removed_files.is_empty() {
            let _ = write!(out, "\n## Removed files\n\n");
            for f in &self.removed_files {
                let _ = writeln!(out, "- {f}");
            }
        }
        for (metric, bucket) in &self.buckets {
            let _ = write!(
                out,
                "\n## {metric} ({} change(s))\n\n```text\n",
                bucket.changed.len()
            );
            for d in &bucket.changed {
                let _ = writeln!(
                    out,
                    "{}.{}  {} \u{2192} {}",
                    d.file,
                    d.field,
                    MetricScalar(d.old),
                    MetricScalar(d.new),
                );
            }
            out.push_str("```\n");
        }
        out
    }

    /// Pretty-printed JSON of the complete diff. The `--metric` filter
    /// still applies (it changes which buckets exist), but every
    /// surviving bucket is emitted in full — a machine consumer reads the
    /// bucket it cares about from a stable schema.
    pub(crate) fn render_json(&self) -> Result<String, serde_json::Error> {
        let mut s = serde_json::to_string_pretty(&JsonOut {
            summary: Summary {
                metrics_changed: self.buckets.len(),
                total_changes: self.total_changes(),
                added_files: self.added_files.len(),
                removed_files: self.removed_files.len(),
            },
            diff: self,
        })?;
        s.push('\n');
        Ok(s)
    }

    /// Append a `## <title>` list of bare file paths to a TTY render.
    fn write_file_section(out: &mut String, title: &str, files: &[String]) {
        if files.is_empty() {
            return;
        }
        let _ = write!(out, "\n## {title}\n");
        for f in files {
            let _ = writeln!(out, "  {f}");
        }
    }
}

/// JSON envelope: a count summary alongside the structured diff.
#[derive(Serialize)]
struct JsonOut<'a> {
    summary: Summary,
    #[serde(flatten)]
    diff: &'a MetricDiff,
}

#[derive(Serialize)]
struct Summary {
    metrics_changed: usize,
    total_changes: usize,
    added_files: usize,
    removed_files: usize,
}

/// How a top-level metric family key maps to bucket name(s).
enum FamilyBucket<'a> {
    /// The whole family is one bucket of this name (every family except
    /// `loc`, plus any unknown key falling back to itself).
    Family(&'a str),
    /// `loc` — bucket each leaf by its sub-metric name instead.
    Expand,
}

/// Classify a top-level metric family key for bucketing.
fn family_bucket(family_key: &str) -> FamilyBucket<'_> {
    if family_key == EXPANDED_FAMILY {
        // `loc` is the one family `list-metrics` expands into distinct
        // sub-metric buckets; the caller buckets its leaves by name.
        return FamilyBucket::Expand;
    }
    match FAMILIES.iter().find(|f| f.name == family_key) {
        Some(f) => FamilyBucket::Family(f.name),
        // A key with no catalog family (a future metric, or a non-metric
        // sibling the emitter adds) buckets under its own key rather than
        // being silently dropped or misfiled under `loc`.
        None => FamilyBucket::Family(family_key),
    }
}

/// True when `field` (a dotted leaf path under the `loc` subtree) is a
/// recognised sub-metric bucket name. The leaf path's first segment is
/// the sub-metric (`sloc`, `ploc`, … — possibly with `_average` /
/// `_min` / `_max` suffixes appended by the emitter), so the bucket is
/// that first segment when it matches a `loc` row name.
fn loc_bucket(field: &str) -> Option<&'static str> {
    // The sub-metric is the segment before the first `_` suffix
    // (`sloc`, `sloc_average`, …); `split_once` yields the whole field
    // when there is no suffix.
    let head = field.split_once('_').map_or(field, |(head, _)| head);
    FAMILIES
        .iter()
        .find(|f| f.name == EXPANDED_FAMILY)?
        .rows
        .iter()
        .find(|r| r.name == head)
        .map(|r| r.name)
}

/// Walk both `metrics` objects in lock-step and yield
/// `(bucket, dotted-field, old, new)` for every scalar leaf present in
/// either side. Leaves missing on one side are reported as a change from
/// / to `0.0` (a metric field that appears or disappears between grammar
/// versions is a genuine delta worth surfacing).
fn metric_scalar_pairs(
    old_metrics: &Value,
    new_metrics: &Value,
) -> Vec<(String, String, f64, f64)> {
    let mut out = Vec::new();
    let (Some(old_obj), Some(new_obj)) = (old_metrics.as_object(), new_metrics.as_object()) else {
        return out;
    };
    let mut family_keys: Vec<&String> = old_obj.keys().chain(new_obj.keys()).collect();
    family_keys.sort();
    family_keys.dedup();

    for family_key in family_keys {
        let old_sub = old_obj.get(family_key);
        let new_sub = new_obj.get(family_key);
        let family_bucket = family_bucket(family_key);
        let mut leaves = Vec::new();
        collect_leaves(old_sub, new_sub, String::new(), &mut leaves);
        for (field, old_v, new_v) in leaves {
            let bucket = match family_bucket {
                FamilyBucket::Family(name) => name.to_string(),
                // `loc` expands per sub-metric; a future loc field with
                // no catalog row falls back to the `loc` family name so
                // it is never silently dropped.
                FamilyBucket::Expand => loc_bucket(&field).unwrap_or(EXPANDED_FAMILY).to_string(),
            };
            out.push((bucket, field, old_v, new_v));
        }
    }
    out
}

fn collect_leaves(
    old: Option<&Value>,
    new: Option<&Value>,
    prefix: String,
    out: &mut Vec<(String, f64, f64)>,
) {
    let old_obj = old.and_then(Value::as_object);
    let new_obj = new.and_then(Value::as_object);
    if old_obj.is_some() || new_obj.is_some() {
        // At least one side is a JSON object: recurse over the union of
        // keys so an added/removed nested field is still walked.
        let mut keys: Vec<&String> = Vec::new();
        if let Some(m) = old_obj {
            keys.extend(m.keys());
        }
        if let Some(m) = new_obj {
            keys.extend(m.keys());
        }
        keys.sort();
        keys.dedup();
        for k in keys {
            let child_prefix = if prefix.is_empty() {
                k.clone()
            } else {
                format!("{prefix}.{k}")
            };
            collect_leaves(
                old_obj.and_then(|m| m.get(k)),
                new_obj.and_then(|m| m.get(k)),
                child_prefix,
                out,
            );
        }
        return;
    }
    // Neither side is an object: treat as a scalar leaf. Only emit a
    // leaf that is numeric on at least one side; a field present on one
    // side only diffs against 0.0.
    let old_v = old.and_then(Value::as_f64);
    let new_v = new.and_then(Value::as_f64);
    if old_v.is_some() || new_v.is_some() {
        out.push((prefix, old_v.unwrap_or(0.0), new_v.unwrap_or(0.0)));
    }
}

/// Load a metric-output set from a file or directory. For a directory,
/// every `*.json` under it is loaded and keyed by its path relative to
/// the directory root, so the old/new sides pair on the same relative
/// layout. For a single file, the document's `name` field is the key
/// (falling back to the file's own name when absent).
fn load_set(path: &Path) -> Result<MetricSet, DiffError> {
    if path.is_dir() {
        return load_dir_set(path);
    }
    let mut set = MetricSet::new();
    let value = read_json(path)?;
    let key = value
        .get(NAME_KEY)
        .and_then(Value::as_str)
        .map(str::to_string)
        .map_or_else(|| path_to_key(path), Ok)?;
    set.insert(key, extract_metrics(value));
    Ok(set)
}

/// Load every per-file JSON document under `root` into a [`MetricSet`].
/// Each entry is keyed by its document's own `name` field — the
/// root-relative source path bca emits — so a directory set pairs with
/// a single-file set (which keys the same way) and with the
/// working-tree / `--since` sides regardless of the `.json` output-file
/// suffix or output-dir layout. Falls back to the path relative to
/// `root` when `name` is absent (`walk_json_files` only yields paths
/// under `root`, so `strip_prefix` cannot fail in practice). Shared by
/// [`load_set`]'s directory branch and the `--since` walk in `lib.rs`.
pub(crate) fn load_dir_set(root: &Path) -> Result<MetricSet, DiffError> {
    let mut set = MetricSet::new();
    for entry in walk_json_files(root)? {
        let value = read_json(&entry)?;
        let key = value.get(NAME_KEY).and_then(Value::as_str).map_or_else(
            || path_to_key(entry.strip_prefix(root).unwrap_or(&entry)),
            |name| Ok(name.to_string()),
        )?;
        set.insert(key, extract_metrics(value));
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
fn path_to_key(path: &Path) -> Result<String, DiffError> {
    path.to_str()
        .map(str::to_string)
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

#[cfg(test)]
#[allow(clippy::float_cmp)] // Tests compare bit-exact metric values.
#[path = "metric_diff_tests.rs"]
mod tests;
