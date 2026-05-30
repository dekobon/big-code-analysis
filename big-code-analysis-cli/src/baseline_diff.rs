//! Structured diff between two baseline files for `bca diff-baseline`
//! (issue #382). Replaces the in-the-reviewer's-head TOML diff parsing
//! that `recipes/baselines.md` used to walk through: it pairs entries
//! across an old and a new baseline on their `(path, qualified, metric)`
//! identity and reports four buckets — `added`, `removed`, `worsened`,
//! `improved` — in TTY, Markdown, or JSON form.
//!
//! The identity deliberately omits `start_line` (mirroring the on-disk
//! matcher from issue #377): a function that drifts up or down the file
//! is the *same* entry, not an add+remove pair. When a single
//! `(path, qualified, metric)` triple carries several records (genuinely
//! ambiguous symbols — overloads, duplicate `impl` blocks), the records
//! on each side are sorted by `(value, start_line)` and paired
//! positionally; any surplus becomes added/removed. This is a
//! best-effort heuristic for an inherently ambiguous case, deterministic
//! and exact for the overwhelmingly common one-record-per-key shape.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::Serialize;

use crate::baseline::DiffEntry;
use crate::format_util::MetricScalar;

/// An entry present in exactly one of the two baselines (`added` /
/// `removed`).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct EntryDelta {
    pub(crate) path: String,
    pub(crate) qualified: String,
    pub(crate) metric: String,
    pub(crate) start_line: usize,
    pub(crate) value: f64,
}

/// An entry present in both baselines whose recorded value moved
/// (`worsened` = value rose, `improved` = value fell). `start_line` is
/// the *new* baseline's line, the one a reviewer would jump to.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ValueDelta {
    pub(crate) path: String,
    pub(crate) qualified: String,
    pub(crate) metric: String,
    pub(crate) start_line: usize,
    pub(crate) old: f64,
    pub(crate) new: f64,
}

/// The full structured diff. Counts in the summary line always reflect
/// every bucket; [`SectionFilter`] only narrows which sections render.
#[derive(Debug, Serialize)]
pub(crate) struct BaselineDiff {
    pub(crate) added: Vec<EntryDelta>,
    pub(crate) removed: Vec<EntryDelta>,
    pub(crate) worsened: Vec<ValueDelta>,
    pub(crate) improved: Vec<ValueDelta>,
}

/// Which sections to render. With no `--*-only` flag set, all four show;
/// any flag set switches to an explicit allow-list (the flags are
/// combinable, so `--worsened-only --added-only` shows both).
#[derive(Debug, Clone, Copy)]
pub(crate) struct SectionFilter {
    added: bool,
    removed: bool,
    worsened: bool,
    improved: bool,
}

impl SectionFilter {
    /// Build from the four CLI `--*-only` flags, in the order
    /// `[added, removed, worsened, improved]`. If none are set, every
    /// section is shown; otherwise only the flagged ones. Passed as an
    /// array rather than four `bool` parameters so the order is explicit
    /// at the call site and the flags can't be silently transposed.
    pub(crate) fn from_flags([added, removed, worsened, improved]: [bool; 4]) -> Self {
        if added || removed || worsened || improved {
            Self {
                added,
                removed,
                worsened,
                improved,
            }
        } else {
            Self {
                added: true,
                removed: true,
                worsened: true,
                improved: true,
            }
        }
    }
}

impl BaselineDiff {
    /// Pair `old` against `new` on `(path, qualified, metric)` and bucket
    /// the result. Both slices come from [`DiffEntry`], whose values are
    /// already finite (the non-finite/negative filter runs at load), so
    /// the `partial_cmp` fallbacks below are never exercised in practice.
    pub(crate) fn compute(old: &[DiffEntry], new: &[DiffEntry]) -> Self {
        // Borrow into a BTreeMap keyed by the identity triple so the
        // walk is deterministic before the per-bucket sort.
        type Key<'a> = (&'a str, &'a str, &'a str);
        let mut groups: BTreeMap<Key<'_>, (Vec<&DiffEntry>, Vec<&DiffEntry>)> = BTreeMap::new();
        for e in old {
            groups
                .entry((&e.path, &e.qualified, &e.metric))
                .or_default()
                .0
                .push(e);
        }
        for e in new {
            groups
                .entry((&e.path, &e.qualified, &e.metric))
                .or_default()
                .1
                .push(e);
        }

        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut worsened = Vec::new();
        let mut improved = Vec::new();

        for (_, (mut olds, mut news)) in groups {
            olds.sort_by(|a, b| cmp_for_pairing(a, b));
            news.sort_by(|a, b| cmp_for_pairing(a, b));
            let paired = olds.len().min(news.len());
            for i in 0..paired {
                let o = olds[i];
                let n = news[i];
                match n.value.partial_cmp(&o.value) {
                    Some(Ordering::Greater) => worsened.push(value_delta(n, o.value, n.value)),
                    Some(Ordering::Less) => improved.push(value_delta(n, o.value, n.value)),
                    // Equal (byte-identical re-baseline of unchanged
                    // code) — or, defensively, an incomparable NaN that
                    // the load filter should already have dropped —
                    // contributes nothing.
                    _ => {}
                }
            }
            removed.extend(olds[paired..].iter().map(|e| entry_delta(e)));
            added.extend(news[paired..].iter().map(|e| entry_delta(e)));
        }

        added.sort_by(cmp_entry_delta);
        removed.sort_by(cmp_entry_delta);
        worsened.sort_by(cmp_value_delta);
        improved.sort_by(cmp_value_delta);

        Self {
            added,
            removed,
            worsened,
            improved,
        }
    }

    /// Human-readable, column-aligned form for a terminal. Empty
    /// sections are omitted; an all-empty diff renders just the summary
    /// line.
    pub(crate) fn render_tty(&self, filter: SectionFilter) -> String {
        let (id_w, metric_w) = self.column_widths(filter);
        let mut out = self.summary_line();
        out.push('\n');
        for (title, rows) in self.sections(filter, id_w, metric_w) {
            let _ = write!(out, "\n## {title}\n");
            for row in rows {
                let _ = writeln!(out, "  {row}");
            }
        }
        out
    }

    /// Markdown form for a sticky PR comment: the summary line, then a
    /// `## Section` header per non-empty filtered bucket with its rows in
    /// a fenced `text` block so column alignment survives Markdown's
    /// whitespace collapsing.
    pub(crate) fn render_markdown(&self, filter: SectionFilter) -> String {
        let (id_w, metric_w) = self.column_widths(filter);
        let mut out = self.summary_line();
        out.push('\n');
        for (title, rows) in self.sections(filter, id_w, metric_w) {
            let _ = write!(out, "\n## {title}\n\n```text\n");
            for row in rows {
                let _ = writeln!(out, "{row}");
            }
            out.push_str("```\n");
        }
        out
    }

    /// Pretty-printed JSON of the complete diff. The `--*-only` flags
    /// intentionally do **not** apply here: a machine consumer reads the
    /// bucket it cares about and a stable schema beats a filtered one.
    pub(crate) fn render_json(&self) -> Result<String, serde_json::Error> {
        let mut s = serde_json::to_string_pretty(&JsonOut {
            summary: Summary {
                added: self.added.len(),
                removed: self.removed.len(),
                worsened: self.worsened.len(),
                improved: self.improved.len(),
            },
            diff: self,
        })?;
        s.push('\n');
        Ok(s)
    }

    /// One-line headline, always reporting the full (unfiltered) counts.
    fn summary_line(&self) -> String {
        format!(
            "{} added, {} removed, {} worsened, {} improved",
            self.added.len(),
            self.removed.len(),
            self.worsened.len(),
            self.improved.len(),
        )
    }

    /// The `(title, formatted rows)` for each non-empty, filtered-in
    /// section, in display order (added, removed, worsened, improved).
    /// Rows are pre-formatted to the shared column widths so both the
    /// TTY and Markdown renderers share one layout.
    fn sections(
        &self,
        filter: SectionFilter,
        id_w: usize,
        metric_w: usize,
    ) -> Vec<(&'static str, Vec<String>)> {
        let mut sections = Vec::new();
        if filter.added && !self.added.is_empty() {
            sections.push(("Added", entry_rows(&self.added, id_w, metric_w)));
        }
        if filter.removed && !self.removed.is_empty() {
            sections.push(("Removed", entry_rows(&self.removed, id_w, metric_w)));
        }
        if filter.worsened && !self.worsened.is_empty() {
            sections.push(("Worsened", value_rows(&self.worsened, id_w, metric_w)));
        }
        if filter.improved && !self.improved.is_empty() {
            sections.push(("Improved", value_rows(&self.improved, id_w, metric_w)));
        }
        sections
    }

    /// Width of the identity and metric columns across every row that
    /// `filter` will render, so TTY and Markdown bodies align.
    fn column_widths(&self, filter: SectionFilter) -> (usize, usize) {
        let entries = [(filter.added, &self.added), (filter.removed, &self.removed)]
            .into_iter()
            .filter_map(|(show, rows)| show.then_some(rows))
            .flatten()
            .map(|r| (id_width(&r.path, &r.qualified), r.metric.len()));
        let values = [
            (filter.worsened, &self.worsened),
            (filter.improved, &self.improved),
        ]
        .into_iter()
        .filter_map(|(show, rows)| show.then_some(rows))
        .flatten()
        .map(|r| (id_width(&r.path, &r.qualified), r.metric.len()));
        entries
            .chain(values)
            .fold((0, 0), |(id_w, m_w), (id, m)| (id_w.max(id), m_w.max(m)))
    }
}

/// JSON envelope: a count summary alongside the four flattened buckets.
#[derive(Serialize)]
struct JsonOut<'a> {
    summary: Summary,
    #[serde(flatten)]
    diff: &'a BaselineDiff,
}

#[derive(Serialize)]
struct Summary {
    added: usize,
    removed: usize,
    worsened: usize,
    improved: usize,
}

/// Separator between a row's path and its qualified symbol in the
/// rendered identity column.
const ID_SEP: &str = "::";

/// Display identity for a row: `path::qualified` (file-level metrics
/// carry the `<file>` sentinel in `qualified`, e.g. `src/x.rs::<file>`).
fn identity(path: &str, qualified: &str) -> String {
    format!("{path}{ID_SEP}{qualified}")
}

/// Rendered width of [`identity`] without allocating the string — used
/// only to size the alignment column. Counts `char`s, not bytes, to
/// match `format!`'s `{:<width$}`, which pads by character count: a
/// byte count would over-pad the whole column (still aligned, but wider
/// than needed) whenever the widest `qualified` symbol is non-ASCII
/// (paths are percent-encoded to ASCII, symbols are not).
fn id_width(path: &str, qualified: &str) -> usize {
    path.chars().count() + ID_SEP.len() + qualified.chars().count()
}

fn entry_rows(rows: &[EntryDelta], id_w: usize, metric_w: usize) -> Vec<String> {
    rows.iter()
        .map(|r| {
            format!(
                "{:<id_w$}  {:<metric_w$}  = {}",
                identity(&r.path, &r.qualified),
                r.metric,
                MetricScalar(r.value),
            )
        })
        .collect()
}

fn value_rows(rows: &[ValueDelta], id_w: usize, metric_w: usize) -> Vec<String> {
    rows.iter()
        .map(|r| {
            format!(
                "{:<id_w$}  {:<metric_w$}  {} \u{2192} {}",
                identity(&r.path, &r.qualified),
                r.metric,
                MetricScalar(r.old),
                MetricScalar(r.new),
            )
        })
        .collect()
}

fn entry_delta(e: &DiffEntry) -> EntryDelta {
    EntryDelta {
        path: e.path.clone(),
        qualified: e.qualified.clone(),
        metric: e.metric.clone(),
        start_line: e.start_line,
        value: e.value,
    }
}

fn value_delta(e: &DiffEntry, old: f64, new: f64) -> ValueDelta {
    ValueDelta {
        path: e.path.clone(),
        qualified: e.qualified.clone(),
        metric: e.metric.clone(),
        start_line: e.start_line,
        old,
        new,
    }
}

/// Sort entries within one identity group for positional pairing: by
/// value, then `start_line`. Both sides use the same order so the i-th
/// old pairs with the i-th new.
fn cmp_for_pairing(a: &DiffEntry, b: &DiffEntry) -> Ordering {
    a.value
        .partial_cmp(&b.value)
        .unwrap_or(Ordering::Equal)
        .then(a.start_line.cmp(&b.start_line))
}

fn cmp_entry_delta(a: &EntryDelta, b: &EntryDelta) -> Ordering {
    (&a.path, &a.qualified, &a.metric, a.start_line).cmp(&(
        &b.path,
        &b.qualified,
        &b.metric,
        b.start_line,
    ))
}

fn cmp_value_delta(a: &ValueDelta, b: &ValueDelta) -> Ordering {
    (&a.path, &a.qualified, &a.metric, a.start_line).cmp(&(
        &b.path,
        &b.qualified,
        &b.metric,
        b.start_line,
    ))
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // Tests compare bit-exact baseline values.
#[path = "baseline_diff_tests.rs"]
mod tests;
