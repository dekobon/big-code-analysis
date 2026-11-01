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
//! sub-metric rows (`sloc`, `ploc`, `lloc`, `cloc`, `blank`, `dloc`) — matching
//! `list-metrics` exactly. A file lands in a bucket when a scalar leaf
//! under that metric's JSON subtree is numeric on both sides and moved by
//! at least the configured threshold. A leaf numeric on one side only
//! has no delta to bucket: it is listed as an added or removed field
//! beside the added and removed files instead (#1623).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use big_code_analysis::metric_catalog::{FAMILIES, MetricFamily};
use serde::Serialize;
use serde_json::Value;

use crate::format_util::{MetricScalar, strip_path_prefix};
// Which family expands into row-named buckets, and the bucket a leaf of
// it lands in, are read from `metric_alias`, so diff bucketing and
// `--metric` aliasing cannot drift apart.
use crate::metric_alias::{diff_bucket, expanded_rows};

// Reading a side off disk or out of a `--since` walk; this file
// compares and renders what it loaded.
mod load;

use load::load_set;
pub(crate) use load::set_from_spaces;

/// Which of the two trees a `bca diff --since` walk was analysing.
/// Named rather than a bare `&str` so a caller cannot label the
/// extracted `<ref>` tree "after" — the two sides are otherwise
/// interchangeable at the [`crate::walk_metric_set`] signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffSide {
    /// The materialized `<ref>` tree (see `diff::materialize_tree`).
    Before,
    /// The working tree (or the explicit directory positional).
    After,
}

impl std::fmt::Display for DiffSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Before => "before",
            Self::After => "after",
        })
    }
}

/// Error surfaced while loading or diffing the two metric-output sets.
/// Rendered by the caller as a tool error (exit 1); a successful diff
/// exits 0 by default, or 2 under `--exit-code` when it is non-empty.
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
    /// A `--since` walk produced a `FuncSpace` that would not serialize
    /// to a JSON value (#1116). Unreachable in practice — the same
    /// `Serialize` impl backs `bca metrics --format json`, and
    /// `serde_json` renders even a non-finite float as `null` rather
    /// than failing — but `to_value` is fallible and the project bans
    /// `unwrap` outside tests.
    Serialize {
        path: PathBuf,
        source: serde_json::Error,
    },
    /// A path component was not valid UTF-8, so it cannot serve as a
    /// stable pairing key. Identifier paths must round-trip losslessly
    /// (no `to_string_lossy`), so this is a hard error rather than a
    /// silent rename.
    NonUtf8Path { path: PathBuf },
    /// The `--since` collector thread panicked, so that side's set is
    /// incomplete. Surfaced as an error rather than resumed: a partial
    /// set would report its missing files as added or removed, which is
    /// the wrong-answer failure mode #1098 exists to prevent.
    CollectorPanicked { side: DiffSide },
    /// A `--since` walk streamed an aggregate item that was not a
    /// metrics space. Unreachable today — `--since` only ever runs
    /// `Action::Metrics`, and `ops` is a separate subcommand — but
    /// dropping the item would silently shorten the set, so it is
    /// refused rather than skipped.
    UnexpectedAggregate,
    /// A `--since` walk could not read every file on one side, so that
    /// side's set is missing files the other side has — which the
    /// comparison would otherwise report as added or removed rather
    /// than as the I/O failure it is (#1098).
    UnreadableInputs { side: DiffSide, count: usize },
    /// A `--since` walk could not *list* every directory on one side, so
    /// that side's set is short a whole subtree rather than a single
    /// file (#1131). Distinct from [`Self::UnreadableInputs`] because
    /// the count means something different — unreadable entries, not
    /// files — and because the two are reported by different layers: the
    /// traversal, and the worker that opened what the traversal found.
    UnwalkableInputs { side: DiffSide, count: usize },
    /// A `--since` walk could not write every per-file document it later
    /// reloads, so that side's set is short the same way — and for the
    /// same reason it must not be reported as a code change.
    UnwritableOutputs { side: DiffSide, count: usize },
    /// A `--since` step outside the metric walk failed once the extracted
    /// `<ref>` tree existed: materializing the tree, or locating the
    /// working tree's repository root. `reason` is the complete message
    /// the `diff` module built. Returned rather than exited on, so the
    /// partial extraction is removed by its `TempTree` dropping, as on
    /// every other error path.
    Since { reason: String },
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
            Self::Serialize { path, source } => {
                write!(
                    f,
                    "failed to serialize metrics for {}: {source}",
                    path.display()
                )
            }
            Self::NonUtf8Path { path } => {
                write!(f, "path is not valid UTF-8: {}", path.display())
            }
            Self::CollectorPanicked { side } => {
                write!(f, "diff --since {side} tree: metric collector panicked")
            }
            Self::UnexpectedAggregate => {
                f.write_str("diff --since walk produced a non-metrics result")
            }
            Self::UnwritableOutputs { side, count } => {
                write!(
                    f,
                    "diff --since {side} tree: {}",
                    crate::write_failure_summary(*count)
                )
            }
            Self::UnreadableInputs { side, count } => {
                write!(
                    f,
                    "diff --since {side} tree: {}",
                    crate::read_failure_summary(*count)
                )
            }
            Self::UnwalkableInputs { side, count } => {
                write!(
                    f,
                    "diff --since {side} tree: {}",
                    crate::walk_failure_summary(*count)
                )
            }
            Self::Since { reason } => f.write_str(reason),
        }
    }
}

/// A single per-file, per-metric-field delta. `field` is the dotted
/// path to the scalar within the metric subtree (e.g. `sum`,
/// `modified.sum`) so a reviewer can see *which* component moved. Both
/// sides are always real numbers read from the documents: a leaf with
/// a number on one side only is not a delta and never reaches here
/// (see [`FieldPresence`]).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct FieldDelta {
    pub(crate) file: String,
    pub(crate) field: String,
    pub(crate) old: f64,
    pub(crate) new: f64,
}

/// A scalar leaf that carries a number on exactly one side, so there is
/// nothing to compare it against (#1623). Two captures taken with
/// different `--metrics` sets, or by two bca versions whose emitters
/// differ, produce whole families of these; diffing each against an
/// invented `0.0` printed them as `0 → n` movements, which for `mi.*`
/// read as a twenty-point improvement. `metric` is the bucket name the
/// `--metric` filter matches, and `field` is the dotted path within
/// that metric's subtree, exactly as in [`FieldDelta`] — empty when the
/// family key itself held the number. The field order is the sort order
/// of every list the rows appear in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) struct FieldPresence {
    pub(crate) file: String,
    pub(crate) metric: String,
    pub(crate) field: String,
}

/// All deltas for one metric bucket. Files that appeared or vanished
/// between the two sets, and fields numeric on one side only, are
/// set-level lists on [`MetricDiff`] rather than bucket members: a file
/// is added or removed wholesale, not per metric, and a one-sided field
/// has no delta to bucket.
#[derive(Debug, Default, Serialize)]
pub(crate) struct Bucket {
    pub(crate) changed: Vec<FieldDelta>,
}

/// The complete per-metric diff: a bucket per metric name that has at
/// least one change, plus the set-level added/removed file and field
/// lists. Counts in the summary line always reflect every bucket and
/// every list.
#[derive(Debug, Default, Serialize)]
pub(crate) struct MetricDiff {
    /// Buckets keyed by metric name, ordered for deterministic output.
    pub(crate) buckets: BTreeMap<String, Bucket>,
    /// Files present in the new set but absent from the old.
    pub(crate) added_files: Vec<String>,
    /// Files present in the old set but absent from the new.
    pub(crate) removed_files: Vec<String>,
    /// Scalar leaves numeric in the new document but not the old, for
    /// files present on both sides. Subject to `--metric` but not to
    /// `--min-change`, which measures a delta these do not have.
    pub(crate) added_fields: Vec<FieldPresence>,
    /// Scalar leaves numeric in the old document but not the new.
    pub(crate) removed_fields: Vec<FieldPresence>,
}

/// A loaded metric-output set: a map from pairing key (the document's
/// `name`, or its path within the set when it has none) to the parsed
/// top-level `metrics` object for that file.
pub(crate) type MetricSet = BTreeMap<String, Value>;

/// Sorted (added, removed) file-key sets between two metric sets: keys
/// present only in `new` are additions, keys present only in `old` are
/// removals. Split out of `from_sets` so the file-presence diff reads as
/// one step, separate from the scalar-bucketing loop (#969).
fn file_set_deltas(old: &MetricSet, new: &MetricSet) -> (Vec<String>, Vec<String>) {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    for key in new.keys() {
        if !old.contains_key(key) {
            added.push(key.clone());
        }
    }
    for key in old.keys() {
        if !new.contains_key(key) {
            removed.push(key.clone());
        }
    }
    added.sort();
    removed.sort();
    (added, removed)
}

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

        // Accept the dotted `check --threshold` spelling as an alias for
        // a bucket name (issue #514): `loc.sloc` -> `sloc`,
        // `halstead.volume` -> `halstead`. Bare bucket names pass through
        // unchanged, so existing `--metric sloc` keeps working.
        let metric_filter: Vec<String> = metric_filter
            .iter()
            .map(|m| crate::metric_alias::normalize_for_diff(m).to_string())
            .collect();
        let metric_filter = metric_filter.as_slice();

        (diff.added_files, diff.removed_files) = file_set_deltas(old, new);

        // Files present on both sides: walk the metric tree and bucket
        // each scalar leaf whose value moved past the threshold; a leaf
        // numeric on one side only is listed, not diffed.
        for (key, old_metrics) in old {
            let Some(new_metrics) = new.get(key) else {
                continue;
            };
            for leaf in metric_leaves(old_metrics, new_metrics) {
                if !metric_filter.is_empty() && !metric_filter.contains(&leaf.bucket) {
                    continue;
                }
                diff.record_leaf(key, leaf, min_change);
            }
        }

        for bucket in diff.buckets.values_mut() {
            bucket
                .changed
                .sort_by(|a, b| (&a.file, &a.field).cmp(&(&b.file, &b.field)));
        }
        // `(file, metric, field)` is unique per row, so the order is
        // total and a stable sort buys nothing.
        diff.added_fields.sort_unstable();
        diff.removed_fields.sort_unstable();
        diff
    }

    /// File `file` carries one scalar leaf: a pair that moved by at
    /// least `min_change` joins its metric bucket; a leaf numeric on one
    /// side only has no delta to measure and is listed as added or
    /// removed instead (#1623).
    fn record_leaf(&mut self, file: &str, leaf: MetricLeaf, min_change: f64) {
        let MetricLeaf {
            bucket,
            field,
            values,
        } = leaf;
        match values {
            LeafValues::Both { old, new } => {
                // A field that did not move is never interesting (even
                // at the `min_change == 0` default, which means "any
                // change", not "every field"). A positive `min_change`
                // additionally suppresses sub-threshold movement.
                let delta = (new - old).abs();
                if delta == 0.0 || delta < min_change {
                    return;
                }
                self.buckets
                    .entry(bucket)
                    .or_default()
                    .changed
                    .push(FieldDelta {
                        file: file.to_owned(),
                        field,
                        old,
                        new,
                    });
            }
            LeafValues::NewOnly => self.added_fields.push(FieldPresence {
                file: file.to_owned(),
                metric: bucket,
                field,
            }),
            LeafValues::OldOnly => self.removed_fields.push(FieldPresence {
                file: file.to_owned(),
                metric: bucket,
                field,
            }),
        }
    }

    /// Total field-level changes across every bucket.
    fn total_changes(&self) -> usize {
        self.buckets.values().map(|b| b.changed.len()).sum()
    }

    /// Whether the diff carries no surviving delta after the active
    /// `--min-change` / `--metric` filtering: no per-metric field
    /// changes, no added/removed files and no added/removed fields.
    /// Drives `bca diff --exit-code` (#692), which exits with the
    /// metric-gate code when this is `false`.
    pub(crate) fn is_empty(&self) -> bool {
        self.total_changes() == 0
            && self.added_files.is_empty()
            && self.removed_files.is_empty()
            && self.added_fields.is_empty()
            && self.removed_fields.is_empty()
    }

    /// The non-empty set-level lists in print order (added and removed
    /// files, then added and removed fields), each under the title both
    /// human renderers give its section and rendered to its display rows,
    /// paths trimmed by `strip_prefix`. One iterator feeds both renderers,
    /// so a list cannot be titled, ordered or fenced differently in one.
    fn listed_sections<'a>(
        &'a self,
        strip_prefix: &'a str,
    ) -> impl Iterator<Item = (&'static str, Vec<String>)> + 'a {
        let files = [
            ("Added files", &self.added_files),
            ("Removed files", &self.removed_files),
        ]
        .into_iter()
        .filter(|(_, files)| !files.is_empty())
        .map(move |(title, files)| {
            let rows = files
                .iter()
                .map(|f| strip_path_prefix(f, strip_prefix).to_owned())
                .collect();
            (title, rows)
        });
        let fields = [
            ("Added fields", &self.added_fields),
            ("Removed fields", &self.removed_fields),
        ]
        .into_iter()
        .filter(|(_, rows)| !rows.is_empty())
        .map(move |(title, rows)| (title, presence_rows(rows, strip_prefix).collect()));
        files.chain(fields)
    }

    /// One-line headline reported by every renderer.
    fn summary_line(&self) -> String {
        format!(
            "{} metric(s) changed, {} added file(s), {} removed file(s), \
             {} added field(s), {} removed field(s)",
            self.buckets.len(),
            self.added_files.len(),
            self.removed_files.len(),
            self.added_fields.len(),
            self.removed_fields.len(),
        )
    }

    /// Human-readable, column-aligned form for a terminal. Empty
    /// sections are omitted; an all-empty diff renders just the summary
    /// line.
    pub(crate) fn render_tty(&self, strip_prefix: &str) -> String {
        let mut out = self.summary_line();
        out.push('\n');
        for (title, rows) in self.listed_sections(strip_prefix) {
            let _ = write!(out, "\n## {title}\n");
            for row in rows {
                let _ = writeln!(out, "  {row}");
            }
        }
        for (metric, bucket) in &self.buckets {
            let _ = write!(out, "\n## {metric} ({} change(s))\n", bucket.changed.len());
            let ids: Vec<String> = bucket
                .changed
                .iter()
                .map(|d| delta_row_id(strip_path_prefix(&d.file, strip_prefix), &d.field))
                .collect();
            let width = ids.iter().map(|id| id.chars().count()).max().unwrap_or(0);
            for (id, d) in ids.iter().zip(&bucket.changed) {
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
    /// `## Section` header per non-empty list and bucket with its rows in
    /// a fenced `text` block, so column alignment survives Markdown's
    /// whitespace collapsing and a path is never read as inline markup
    /// (`pkg/__init__.py` as a bulleted item renders `init` in bold).
    pub(crate) fn render_markdown(&self, strip_prefix: &str) -> String {
        let mut out = self.summary_line();
        out.push('\n');
        for (title, rows) in self.listed_sections(strip_prefix) {
            let _ = write!(out, "\n## {title}\n\n```text\n");
            for row in rows {
                let _ = writeln!(out, "{row}");
            }
            out.push_str("```\n");
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
                    "{}  {} \u{2192} {}",
                    delta_row_id(strip_path_prefix(&d.file, strip_prefix), &d.field),
                    MetricScalar(d.old),
                    MetricScalar(d.new),
                );
            }
            out.push_str("```\n");
        }
        out
    }

    /// Pretty-printed JSON of the complete diff. The `--metric` filter
    /// still applies (it changes which buckets exist and which
    /// `added_fields` / `removed_fields` rows survive), but every
    /// surviving bucket is emitted in full — a machine consumer reads the
    /// bucket it cares about from a stable schema. `summary.total_changes`
    /// and `metrics_changed` count numeric movement only; the one-sided
    /// rows are counted by `summary.added_fields` / `removed_fields`, and
    /// `--exit-code` fires on any of the four.
    pub(crate) fn render_json(&self) -> Result<String, serde_json::Error> {
        let mut s = serde_json::to_string_pretty(&JsonOut {
            summary: Summary {
                metrics_changed: self.buckets.len(),
                total_changes: self.total_changes(),
                added_files: self.added_files.len(),
                removed_files: self.removed_files.len(),
                added_fields: self.added_fields.len(),
                removed_fields: self.removed_fields.len(),
            },
            diff: self,
        })?;
        s.push('\n');
        Ok(s)
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
    added_fields: usize,
    removed_fields: usize,
}

/// Column-aligned `<file>  <metric>  <field>` rows for a one-sided-field
/// section, each file trimmed by `strip_prefix` for display. Shared by
/// the TTY and Markdown renderers so the two cannot drift.
fn presence_rows<'a>(
    rows: &'a [FieldPresence],
    strip_prefix: &str,
) -> impl Iterator<Item = String> + 'a {
    let files: Vec<&str> = rows
        .iter()
        .map(|p| strip_path_prefix(&p.file, strip_prefix))
        .collect();
    let file_width = files.iter().map(|f| f.chars().count()).max().unwrap_or(0);
    let metric_width = rows
        .iter()
        .map(|p| p.metric.chars().count())
        .max()
        .unwrap_or(0);
    rows.iter()
        .zip(files)
        .map(move |(FieldPresence { metric, field, .. }, file)| {
            let field = field_label(field);
            format!("{file:<file_width$}  {metric:<metric_width$}  {field}")
        })
}

/// What a row calls a leaf with no field path: a family whose value is
/// itself a bare number rather than an object. Only JSON bca did not
/// write carries one, but a blank would end a one-sided row in the
/// column padding's trailing spaces and a delta row in a dangling `.`.
const SCALAR_FIELD_LABEL: &str = "(scalar)";

/// The display form of a leaf's dotted field path, naming the empty one.
fn field_label(field: &str) -> &str {
    if field.is_empty() {
        SCALAR_FIELD_LABEL
    } else {
        field
    }
}

/// The `<file>.<field>` identity a delta row leads with, or
/// `<file> (scalar)` for a family that is a bare number on both sides,
/// matching how the one-sided sections name the same leaf.
fn delta_row_id(file: &str, field: &str) -> String {
    if field.is_empty() {
        format!("{file} {SCALAR_FIELD_LABEL}")
    } else {
        format!("{file}.{field}")
    }
}

/// How a top-level metric family key maps to bucket name(s).
enum FamilyBucket<'a> {
    /// The whole family is one bucket of this name (every family except
    /// `loc`, plus any unknown key falling back to itself).
    Family(&'a str),
    /// A family whose `list-metrics` rows name its sub-measurements
    /// (`loc`) — bucket each leaf by its row instead.
    Expand(&'static MetricFamily),
}

/// Classify a top-level metric family key for bucketing.
fn family_bucket(family_key: &str) -> FamilyBucket<'_> {
    match FAMILIES.iter().find(|f| f.name == family_key) {
        Some(family) if expanded_rows(family).is_some() => FamilyBucket::Expand(family),
        Some(f) => FamilyBucket::Family(f.name),
        // A key with no catalog family (a future metric, or a non-metric
        // sibling the emitter adds) buckets under its own key rather than
        // being silently dropped or misfiled under `loc`.
        None => FamilyBucket::Family(family_key),
    }
}

/// The bucket for `field`, a dotted leaf path under an expanded family's
/// subtree: the row it names (`sloc`, or `sloc_average` / `_min` /
/// `_max` as the emitter suffixes it), or the family name for a leaf no
/// row names. [`diff_bucket`] does the matching, so the leaves a delta
/// carries and the names `--metric` validates go through one rule.
fn row_bucket(family: &MetricFamily, field: &str) -> &'static str {
    diff_bucket(family, field)
}

/// What the two documents hold at one scalar leaf. Built only when at
/// least one side is numeric, so the "neither" state is unrepresentable
/// and no caller has to invent a value for a missing side (#1623).
#[derive(Debug, Clone, Copy, PartialEq)]
enum LeafValues {
    /// Numeric on both sides: a comparable pair.
    Both { old: f64, new: f64 },
    /// Numeric on the new side only.
    NewOnly,
    /// Numeric on the old side only.
    OldOnly,
}

impl LeafValues {
    /// Classify a leaf from what each side holds. "Present" means
    /// numeric: a key whose value is not a number (`serde_json` writes a
    /// non-finite float as `null`) carries nothing to compare on that
    /// side, so it reads as absent there. A leaf numeric on neither side
    /// is not a metric scalar at all, hence `None`.
    fn classify(old: Option<f64>, new: Option<f64>) -> Option<Self> {
        match (old, new) {
            (Some(old), Some(new)) => Some(Self::Both { old, new }),
            (None, Some(_)) => Some(Self::NewOnly),
            (Some(_), None) => Some(Self::OldOnly),
            (None, None) => None,
        }
    }
}

/// One scalar leaf of a file's `metrics` object, resolved to the bucket
/// its family maps to. `field` is the dotted path within the family
/// subtree (`sum`, `modified.sum`).
struct MetricLeaf {
    bucket: String,
    field: String,
    values: LeafValues,
}

/// Walk both `metrics` objects in lock-step and yield every scalar leaf
/// numeric on at least one side, with its bucket resolved. A leaf
/// missing on one side is yielded as such rather than diffed against an
/// invented `0.0`, which is indistinguishable from a real zero. A side
/// whose `metrics` is not an object holds no families, so every family
/// of the other side is one-sided.
fn metric_leaves(old_metrics: &Value, new_metrics: &Value) -> Vec<MetricLeaf> {
    let mut out = Vec::new();
    let old_obj = old_metrics.as_object();
    let new_obj = new_metrics.as_object();

    for family_key in union_keys(old_obj, new_obj) {
        let old_sub = old_obj.and_then(|m| m.get(family_key));
        let new_sub = new_obj.and_then(|m| m.get(family_key));
        let family_bucket = family_bucket(family_key);
        let mut leaves = Vec::new();
        collect_leaves(old_sub, new_sub, String::new(), &mut leaves);
        for (field, values) in leaves {
            let bucket = match family_bucket {
                FamilyBucket::Family(name) => name.to_string(),
                // `loc` expands per sub-metric; a future loc field with
                // no catalog row falls back to the `loc` family name so
                // it is never silently dropped.
                FamilyBucket::Expand(family) => row_bucket(family, &field).to_string(),
            };
            out.push(MetricLeaf {
                bucket,
                field,
                values,
            });
        }
    }
    out
}

/// The keys of both sides' objects, sorted and deduplicated, so a
/// lock-step walk visits a key present on either side once. A side that
/// is not an object contributes nothing.
fn union_keys<'a>(
    old: Option<&'a serde_json::Map<String, Value>>,
    new: Option<&'a serde_json::Map<String, Value>>,
) -> Vec<&'a String> {
    let mut keys: Vec<&String> = old
        .into_iter()
        .chain(new)
        .flat_map(serde_json::Map::keys)
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

fn collect_leaves(
    old: Option<&Value>,
    new: Option<&Value>,
    prefix: String,
    out: &mut Vec<(String, LeafValues)>,
) {
    // A number on either side is a leaf at this path whatever the other
    // side holds — including an object, which the recursion below walks
    // without ever seeing the number facing it.
    let values = LeafValues::classify(old.and_then(Value::as_f64), new.and_then(Value::as_f64));
    let old_obj = old.and_then(Value::as_object);
    let new_obj = new.and_then(Value::as_object);
    // Recurse over the union of keys so an added/removed nested field is
    // still walked.
    for k in union_keys(old_obj, new_obj) {
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
    if let Some(values) = values {
        out.push((prefix, values));
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // Tests compare bit-exact metric values.
#[path = "metric_diff_tests.rs"]
mod tests;
