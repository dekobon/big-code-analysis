//! `bca exemptions` — a unified audit of everything the `bca check`
//! gate skips (issue #386).
//!
//! Three tiers of exemption are reported in one view so a reviewer
//! asking "what offenders is the gate hiding from me?" gets a single
//! answer instead of running three commands:
//!
//! - **In-source markers** (`bca: suppress`, `bca: suppress-file`,
//!   `#lizard forgives`, …) — per-function or per-file silencers,
//!   collected from the AST so a marker inside a string literal is not
//!   miscounted.
//! - **`[check.exclude]` globs** — per-glob exemptions that drop whole
//!   categories of files from the gate (#378).
//! - **`.bca-baseline.toml` entries** — per-`(path, symbol, metric)`
//!   grandfathered offenders (#376, #377).
//!
//! Each section is independently suppressible via `--only-markers` /
//! `--only-excludes` / `--only-baseline`, and the JSON form nests all
//! three under a single `suppressions` envelope so cross-tier filtering
//! with `jq` does not require three invocations.

use std::fmt::Write as _;

use serde::Serialize;

use big_code_analysis::{
    SuppressionDialect, SuppressionMarker, SuppressionScope, SuppressionTarget,
};

use crate::OutputFormat;
use crate::baseline::DiffEntry;
use crate::format_util::MetricScalar;

/// Per-file marker batch streamed from the walk worker pool to the
/// post-walk aggregator. `path` is the display path (already
/// `strip_prefix`-trimmed at collection time is *not* done here — the
/// renderer applies the prefix so the raw walked path stays intact for
/// JSON consumers that want the on-disk path).
pub(crate) struct FileMarkers {
    /// File the markers were found in (UTF-8 path as walked).
    pub(crate) path: String,
    /// Markers found in the file, already sorted by line.
    pub(crate) markers: Vec<SuppressionMarker>,
}

/// One in-source marker flattened with its file path for display.
pub(crate) struct MarkerRow {
    pub(crate) path: String,
    pub(crate) marker: SuppressionMarker,
}

/// One baseline entry flattened for display.
pub(crate) struct BaselineRow {
    pub(crate) path: String,
    pub(crate) qualified: String,
    pub(crate) metric: String,
    pub(crate) value: f64,
    pub(crate) start_line: usize,
}

impl From<DiffEntry> for BaselineRow {
    fn from(e: DiffEntry) -> Self {
        Self {
            path: e.path,
            qualified: e.qualified,
            metric: e.metric,
            value: e.value,
            start_line: e.start_line,
        }
    }
}

/// The baseline section: the audited file's path plus its flattened
/// entries. The file path is shown in the section header so a reader
/// knows *which* baseline was audited (it can be overridden via
/// `--baseline` or `bca.toml`).
pub(crate) struct BaselineSection {
    pub(crate) path: String,
    pub(crate) entries: Vec<BaselineRow>,
}

/// The assembled report. `None` for a section means "not requested" — a
/// `--only-*` flag excluded it. `Some(_)` means requested; the contained
/// collection may still be empty when nothing was found, which the
/// renderers surface as an explicit "none" rather than an absent
/// section.
pub(crate) struct ExemptionsReport {
    pub(crate) markers: Option<Vec<MarkerRow>>,
    pub(crate) excludes: Option<Vec<String>>,
    pub(crate) baseline: Option<BaselineSection>,
}

impl ExemptionsReport {
    /// Render the report in the requested format. JSON serialization of
    /// a fixed-shape struct of owned scalars cannot fail in practice;
    /// the `Result` lets the caller surface any future serde error as a
    /// tool error rather than panicking.
    pub(crate) fn render(
        &self,
        format: OutputFormat,
        strip_prefix: &str,
    ) -> Result<String, serde_json::Error> {
        match format {
            OutputFormat::Tty => Ok(self.render_tty(strip_prefix)),
            OutputFormat::Markdown => Ok(self.render_markdown(strip_prefix)),
            OutputFormat::Json => self.render_json(strip_prefix),
        }
    }

    fn render_tty(&self, strip_prefix: &str) -> String {
        let mut out = String::new();
        if let Some(rows) = &self.markers {
            let _ = writeln!(out, "# In-source markers ({})", rows.len());
            if rows.is_empty() {
                out.push_str("  (none)\n");
            } else {
                render_marker_rows_tty(&mut out, rows, strip_prefix);
            }
        }
        if let Some(globs) = &self.excludes {
            if !out.is_empty() {
                out.push('\n');
            }
            let _ = writeln!(out, "# [check.exclude] globs ({})", globs.len());
            if globs.is_empty() {
                out.push_str("  (none)\n");
            } else {
                for g in globs {
                    let _ = writeln!(out, "  {g}");
                }
            }
        }
        if let Some(section) = &self.baseline {
            if !out.is_empty() {
                out.push('\n');
            }
            let _ = writeln!(
                out,
                "# Baseline ({}, {})",
                section.path,
                entry_count(section.entries.len())
            );
            if section.entries.is_empty() {
                out.push_str("  (none)\n");
            } else {
                for e in &section.entries {
                    let path = strip(&e.path, strip_prefix);
                    let _ = writeln!(
                        out,
                        "  {path}:{} {} {} {}",
                        e.start_line,
                        e.qualified,
                        e.metric,
                        MetricScalar(e.value),
                    );
                }
            }
        }
        out
    }

    fn render_markdown(&self, strip_prefix: &str) -> String {
        let mut out = String::new();
        if let Some(rows) = &self.markers {
            let _ = writeln!(out, "## In-source markers ({})\n", rows.len());
            if rows.is_empty() {
                out.push_str("_None._\n");
            } else {
                out.push_str("| File | Line | Marker | Metrics | Function |\n");
                out.push_str("| --- | ---: | --- | --- | --- |\n");
                for row in rows {
                    let m = &row.marker;
                    let path = strip(&row.path, strip_prefix);
                    let _ = writeln!(
                        out,
                        "| {} | {} | `{}` | {} | {} |",
                        path,
                        m.line,
                        marker_label(m.target, m.dialect),
                        scope_metrics(&m.scope),
                        function_cell(m),
                    );
                }
            }
        }
        if let Some(globs) = &self.excludes {
            if !out.is_empty() {
                out.push('\n');
            }
            let _ = writeln!(out, "## [check.exclude] globs ({})\n", globs.len());
            if globs.is_empty() {
                out.push_str("_None._\n");
            } else {
                for g in globs {
                    let _ = writeln!(out, "- `{g}`");
                }
            }
        }
        if let Some(section) = &self.baseline {
            if !out.is_empty() {
                out.push('\n');
            }
            let _ = writeln!(
                out,
                "## Baseline (`{}`, {})\n",
                section.path,
                entry_count(section.entries.len())
            );
            if section.entries.is_empty() {
                out.push_str("_None._\n");
            } else {
                out.push_str("| File | Line | Symbol | Metric | Value |\n");
                out.push_str("| --- | ---: | --- | --- | ---: |\n");
                for e in &section.entries {
                    let path = strip(&e.path, strip_prefix);
                    let _ = writeln!(
                        out,
                        "| {} | {} | {} | {} | {} |",
                        path,
                        e.start_line,
                        e.qualified,
                        e.metric,
                        MetricScalar(e.value),
                    );
                }
            }
        }
        out
    }

    fn render_json(&self, strip_prefix: &str) -> Result<String, serde_json::Error> {
        let markers = self.markers.as_ref().map(|rows| {
            rows.iter()
                .map(|row| JsonMarker {
                    path: strip(&row.path, strip_prefix),
                    line: row.marker.line,
                    target: row.marker.target,
                    scope: &row.marker.scope,
                    dialect: row.marker.dialect,
                    function: row.marker.function.as_deref(),
                })
                .collect()
        });
        let baseline = self.baseline.as_ref().map(|section| {
            section
                .entries
                .iter()
                .map(|e| JsonBaseline {
                    path: strip(&e.path, strip_prefix),
                    line: e.start_line,
                    qualified: &e.qualified,
                    metric: &e.metric,
                    value: e.value,
                })
                .collect()
        });
        let envelope = JsonEnvelope {
            suppressions: JsonSections {
                markers,
                excludes: self.excludes.as_deref(),
                baseline,
            },
        };
        serde_json::to_string_pretty(&envelope)
    }
}

/// Render the in-source marker rows as an aligned plain-text block.
/// Column widths are computed in one pass so the marker label and
/// `path:line` columns line up regardless of input.
fn render_marker_rows_tty(out: &mut String, rows: &[MarkerRow], strip_prefix: &str) {
    // Pre-format the location column so width is measured on the final
    // string (after prefix stripping), not the raw path.
    let locs: Vec<String> = rows
        .iter()
        .map(|r| format!("{}:{}", strip(&r.path, strip_prefix), r.marker.line))
        .collect();
    let loc_w = locs.iter().map(String::len).max().unwrap_or(0);
    let label_w = rows
        .iter()
        .map(|r| marker_label(r.marker.target, r.marker.dialect).len())
        .max()
        .unwrap_or(0);
    for (loc, row) in locs.iter().zip(rows) {
        let m = &row.marker;
        let label = marker_label(m.target, m.dialect);
        let _ = writeln!(
            out,
            "  {loc:loc_w$}  {label:label_w$}  metrics={}  {}",
            scope_metrics(&m.scope),
            function_cell(m),
        );
    }
}

/// The marker's source syntax, derived from its target and dialect.
/// Mirrors the exact tokens an author would write, so the audit doubles
/// as a "here is the literal comment" reference.
fn marker_label(target: SuppressionTarget, dialect: SuppressionDialect) -> &'static str {
    match (dialect, target) {
        (SuppressionDialect::Native, SuppressionTarget::Function) => "bca: suppress",
        (SuppressionDialect::Native, SuppressionTarget::File) => "bca: suppress-file",
        (SuppressionDialect::Lizard, SuppressionTarget::Function) => "#lizard forgives",
        (SuppressionDialect::Lizard, SuppressionTarget::File) => "#lizard forgive global",
    }
}

/// Human-readable metric coverage: `all`, `none` (an empty explicit
/// list — a marker that silences nothing), or the comma-separated list.
fn scope_metrics(scope: &SuppressionScope) -> String {
    match scope {
        SuppressionScope::All => "all".to_owned(),
        SuppressionScope::Some(set) if set.is_empty() => "none".to_owned(),
        SuppressionScope::Some(set) => set
            .iter()
            .map(|m| m.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// The "function" display cell. File-scoped markers read `(whole file)`;
/// function-scoped markers with no enclosing function read
/// `(no enclosing fn)` — a dead marker worth flagging in an audit.
fn function_cell(m: &SuppressionMarker) -> String {
    match (m.target, m.function.as_deref()) {
        (SuppressionTarget::File, _) => "(whole file)".to_owned(),
        (SuppressionTarget::Function, Some(name)) => name.to_owned(),
        (SuppressionTarget::Function, None) => "(no enclosing fn)".to_owned(),
    }
}

/// Strip `prefix` from the front of `path` for display. A no-op when
/// `prefix` is empty or does not match, so callers can pass an empty
/// prefix unconditionally.
fn strip<'a>(path: &'a str, prefix: &str) -> &'a str {
    if prefix.is_empty() {
        path
    } else {
        path.strip_prefix(prefix).unwrap_or(path)
    }
}

/// `"1 entry"` / `"N entries"` — the count phrase used in section
/// headers.
fn entry_count(n: usize) -> String {
    format!("{n} {}", if n == 1 { "entry" } else { "entries" })
}

#[derive(Serialize)]
struct JsonEnvelope<'a> {
    suppressions: JsonSections<'a>,
}

/// The three sections under the `suppressions` envelope. A `null`
/// section means it was not requested (`--only-*`); an empty array means
/// requested but empty — a distinction `jq` filters can rely on.
#[derive(Serialize)]
struct JsonSections<'a> {
    markers: Option<Vec<JsonMarker<'a>>>,
    excludes: Option<&'a [String]>,
    baseline: Option<Vec<JsonBaseline<'a>>>,
}

#[derive(Serialize)]
struct JsonMarker<'a> {
    path: &'a str,
    line: usize,
    target: SuppressionTarget,
    scope: &'a SuppressionScope,
    dialect: SuppressionDialect,
    function: Option<&'a str>,
}

#[derive(Serialize)]
struct JsonBaseline<'a> {
    path: &'a str,
    line: usize,
    qualified: &'a str,
    metric: &'a str,
    value: f64,
}

#[cfg(test)]
#[path = "exemptions_tests.rs"]
mod tests;
