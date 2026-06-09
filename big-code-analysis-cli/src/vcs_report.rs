// bca: suppress-file(halstead, nargs, nom)
// The `VCS_SPECS` table is declarative data: ~17 capture-free cell-projector
// closures inflate the file-level nom/nargs/halstead sums (each closure is a
// one-arg "function"), the same string-formatting / many-fn aggregation
// artifact the sibling report renderers suppress (see
// `markdown_report/hotspot.rs`) — not per-function logic complexity
// (cognitive/cyclomatic stay enforced).

//! Rendered change-history (VCS) report (issue #573).
//!
//! Produces the Markdown and HTML pages for `bca vcs --format
//! markdown|html`. Mirrors the AST report's shared-spec design: the
//! column set lives in one [`VCS_SPECS`] table that both formats
//! consume, so Markdown and HTML cannot drift (guarded by
//! `markdown_and_html_columns_match`). Table markup and escaping are
//! delegated to the existing renderers' `write_table` helpers
//! (`crate::markdown_report::write_table` /
//! `crate::html_report::write_table`, or `write_table_classed` for the
//! HTML severity-heat tint on the `risk_score` cell, see
//! [`risk_heat_class`]) and the shared HTML scaffolding
//! (`write_html_head` / `write_html_tail`), so the page carries
//! `report.html`'s styling and inline click-to-sort behaviour.

use std::fmt::Write as _;

use crate::html_report::{
    write_html_head, write_html_tail, write_table as write_html_table,
    write_table_classed as write_html_table_classed,
};
use crate::markdown_report::hotspot::Cell;
use crate::markdown_report::{Align, render_cell_md, write_table as write_md_table};
use crate::vcs_command::{FileEntry, Report};

/// One column of the change-history table: header, alignment, and a
/// capture-free projector to a [`Cell`]. The `rank` argument (1-based)
/// lets the Rank column be part of the shared spec rather than
/// special-cased in each renderer, where it could drift.
struct VcsColumn {
    header: &'static str,
    align: Align,
    cell: fn(rank: usize, &FileEntry) -> Cell,
}

/// The change-history columns, defined once and rendered identically by
/// both formats. Order and content mirror the structured CSV record (so
/// the rendered page is the complete, sortable view of the same data),
/// with a leading Rank column.
const VCS_SPECS: &[VcsColumn] = &[
    VcsColumn {
        header: "Rank",
        align: Align::Right,
        cell: |rank, _| Cell::Num(rank.to_string()),
    },
    VcsColumn {
        header: "File",
        align: Align::Left,
        cell: |_, e| Cell::Path(e.path.clone()),
    },
    VcsColumn {
        header: "Risk",
        align: Align::Right,
        cell: |_, e| Cell::Num(format!("{:.1}", e.vcs.risk_score)),
    },
    VcsColumn {
        header: "Commits (recent)",
        align: Align::Right,
        cell: |_, e| Cell::Num(e.vcs.commits_recent.to_string()),
    },
    VcsColumn {
        header: "Commits (total)",
        align: Align::Right,
        cell: |_, e| Cell::Num(e.vcs.commits_long.to_string()),
    },
    VcsColumn {
        header: "Churn (recent)",
        align: Align::Right,
        cell: |_, e| Cell::Num(e.vcs.churn_recent.to_string()),
    },
    VcsColumn {
        header: "Churn (total)",
        align: Align::Right,
        cell: |_, e| Cell::Num(e.vcs.churn_long.to_string()),
    },
    VcsColumn {
        header: "Authors (recent)",
        align: Align::Right,
        cell: |_, e| Cell::Num(e.vcs.authors_recent.to_string()),
    },
    VcsColumn {
        header: "Authors (total)",
        align: Align::Right,
        cell: |_, e| Cell::Num(e.vcs.authors_long.to_string()),
    },
    VcsColumn {
        header: "Ownership",
        align: Align::Right,
        cell: |_, e| Cell::Num(format!("{:.2}", e.vcs.ownership_top_share)),
    },
    VcsColumn {
        header: "Burst",
        align: Align::Right,
        cell: |_, e| Cell::Num(format!("{:.2}", e.vcs.burst)),
    },
    VcsColumn {
        header: "Bug fixes",
        align: Align::Right,
        cell: |_, e| Cell::Num(e.vcs.bug_fix_commits.to_string()),
    },
    VcsColumn {
        header: "Sec fixes",
        align: Align::Right,
        cell: |_, e| Cell::Num(e.vcs.security_fix_commits.to_string()),
    },
    VcsColumn {
        header: "Reverts",
        align: Align::Right,
        cell: |_, e| Cell::Num(e.vcs.revert_commits.to_string()),
    },
    VcsColumn {
        header: "Age (d)",
        align: Align::Right,
        cell: |_, e| Cell::Num(e.vcs.age_days.to_string()),
    },
    VcsColumn {
        header: "Last mod (d)",
        align: Align::Right,
        cell: |_, e| Cell::Num(e.vcs.last_modified_days.to_string()),
    },
    VcsColumn {
        header: "Change entropy (recent)",
        align: Align::Right,
        cell: |_, e| Cell::Num(format!("{:.2}", e.vcs.change_entropy_recent)),
    },
    VcsColumn {
        header: "Change entropy (total)",
        align: Align::Right,
        cell: |_, e| Cell::Num(format!("{:.2}", e.vcs.change_entropy_long)),
    },
    VcsColumn {
        header: "Co-change entropy (recent)",
        align: Align::Right,
        cell: |_, e| Cell::Num(format!("{:.2}", e.vcs.cochange_entropy_recent)),
    },
    VcsColumn {
        header: "Co-change entropy (total)",
        align: Align::Right,
        cell: |_, e| Cell::Num(format!("{:.2}", e.vcs.cochange_entropy_long)),
    },
    VcsColumn {
        header: "Hotspot",
        align: Align::Right,
        // Empty when AST metrics are not joined (plain `bca vcs`).
        cell: |_, e| {
            Cell::Num(
                e.vcs
                    .hotspot_score
                    .map(|h| format!("{h:.1}"))
                    .unwrap_or_default(),
            )
        },
    },
];

/// Header of the column that carries the severity-heat tint in the HTML
/// report. Matched structurally against [`VCS_SPECS`] (see
/// [`risk_column_index`]) so the heat lands on the right `<td>` even if
/// the column order changes; the Markdown path ignores it entirely.
const RISK_COLUMN_HEADER: &str = "Risk";

/// Number of discrete severity-heat bands for the `risk_score` cell.
/// Five bands give a green→yellow→red gradient coarse enough to stay
/// WCAG-contrast-correct and snapshot-stable, yet fine enough to separate
/// the riskiest files. `risk_score` is *ordinal*, so the band derives
/// from each row's rank, never the absolute value — see [`risk_heat_class`].
const HEAT_BAND_COUNT: usize = 5;

/// CSS class names for the five severity-heat bands, most-severe first.
/// Index 0 (`risk-heat-0`, red) tints the top-ranked rows and index 4
/// (`risk-heat-4`, green) the lowest-ranked. Matching rules live in
/// `html_report::INLINE_CSS`.
const HEAT_CLASSES: [&str; HEAT_BAND_COUNT] = [
    "risk-heat-0",
    "risk-heat-1",
    "risk-heat-2",
    "risk-heat-3",
    "risk-heat-4",
];

/// Index of the risk column within [`VCS_SPECS`], or `None` if no column
/// carries [`RISK_COLUMN_HEADER`] (a spec edit would have to remove it).
fn risk_column_index() -> Option<usize> {
    VCS_SPECS
        .iter()
        .position(|c| c.header == RISK_COLUMN_HEADER)
}

/// Severity-heat CSS class for the risk cell of the `row`-th file in a
/// report of `n_rows` risk-ranked files, or `None` when no band applies.
///
/// The band is a function of *rank*, not the raw `risk_score`:
/// `report.files` is sorted by descending risk, so `row` (0-based) is the
/// rank. Splitting `[0, n_rows)` into [`HEAT_BAND_COUNT`] equal-width
/// slices maps the top rows to band 0 (most severe) and the bottom rows to
/// the last band (least severe), independent of the absolute values.
///
/// Edge case: a single row has no ranking to express, so it gets the
/// least-severe band (a lone file should not look alarming); `n_rows == 0`
/// never calls this (no rows are rendered).
fn risk_heat_class(row: usize, n_rows: usize) -> Option<&'static str> {
    if n_rows <= 1 {
        return HEAT_CLASSES.last().copied();
    }
    // Equal-width rank slices: floor(row * BANDS / n_rows), in range since `row < n_rows`.
    let band = (row * HEAT_BAND_COUNT) / n_rows;
    HEAT_CLASSES.get(band).copied()
}

/// The shared page/section heading.
const HEADING: &str = "Change-history risk";

/// Shown in both formats when no tracked file matched the walk filters.
const EMPTY_MESSAGE: &str = "No tracked files matched.";

fn headers() -> Vec<&'static str> {
    VCS_SPECS.iter().map(|c| c.header).collect()
}

fn aligns() -> Vec<Align> {
    VCS_SPECS.iter().map(|c| c.align).collect()
}

/// The ranked rows as `Cell`s — one inner `Vec` per file, in column
/// order. `report.files` is already risk-ranked, so the enumeration
/// index drives the 1-based Rank column.
fn cell_rows(report: &Report) -> Vec<Vec<Cell>> {
    report
        .files
        .iter()
        .enumerate()
        .map(|(i, entry)| VCS_SPECS.iter().map(|c| (c.cell)(i + 1, entry)).collect())
        .collect()
}

/// Extract a `Cell`'s text payload for HTML, where `write_table` escapes
/// every kind uniformly (so the kind itself does not matter here).
fn cell_text(cell: Cell) -> String {
    let (Cell::Name(t) | Cell::Path(t) | Cell::Num(t)) = cell;
    t
}

/// One-line provenance shared by both formats: window lengths and the
/// formula / schema version stamps.
fn provenance(report: &Report) -> String {
    format!(
        "Long window {}d, recent window {}d, risk formula v{}, schema v{}.",
        report.long_window_days,
        report.recent_window_days,
        report.risk_score_version,
        report.vcs_schema_version,
    )
}

const SHALLOW_NOTE: &str =
    "Shallow clone detected: history is truncated, so counts are lower bounds.";

// --- Markdown ---------------------------------------------------------

/// Render the standalone Markdown change-history page (`#` title +
/// provenance + table).
pub(crate) fn render_markdown(report: &Report) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# {HEADING}\n");
    write_markdown_body(&mut out, report);
    out
}

/// Append the change-history section under a level-2 heading, for
/// embedding in the aggregated `bca report --vcs` page (whose global
/// header owns the `#` H1). Paths are the canonical repo-relative git
/// paths from the index — `bca report --strip-prefix` deliberately does
/// not rewrite them, since they are not filesystem walk paths.
pub(crate) fn push_markdown_section(out: &mut String, report: &Report) {
    let _ = writeln!(out, "\n## {HEADING}\n");
    write_markdown_body(out, report);
}

/// Provenance line + ranked table (or the empty-set message). No
/// heading — the caller supplies the right level.
fn write_markdown_body(out: &mut String, report: &Report) {
    let _ = writeln!(out, "_{}_", provenance(report));
    if report.truncated_shallow_clone {
        let _ = writeln!(out, "\n> **Note:** {SHALLOW_NOTE}");
    }
    out.push('\n');
    if report.files.is_empty() {
        let _ = writeln!(out, "{EMPTY_MESSAGE}");
    } else {
        let rows: Vec<Vec<String>> = cell_rows(report)
            .into_iter()
            .map(|row| row.into_iter().map(render_cell_md).collect())
            .collect();
        write_md_table(out, &headers(), &aligns(), &rows);
    }
    if let Some(aggregate) = &report.vcs_aggregate {
        write_markdown_bus_factor(out, &aggregate.bus_factor);
    }
}

/// Column headers / alignments for the per-directory bus-factor table,
/// shared by both formats so they cannot drift.
fn bus_factor_headers() -> [&'static str; 3] {
    ["Directory", "Bus factor", "Files"]
}

/// Alignments matching [`bus_factor_headers`].
fn bus_factor_aligns() -> [Align; 3] {
    [Align::Left, Align::Right, Align::Right]
}

/// The per-directory rows (directory, bus factor, files) for the shared
/// table renderers.
fn bus_factor_rows(bf: &big_code_analysis::vcs::BusFactor) -> Vec<Vec<String>> {
    bf.by_directory
        .iter()
        .map(|dir| {
            vec![
                dir.directory.clone(),
                dir.group.bus_factor.to_string(),
                dir.group.files.to_string(),
            ]
        })
        .collect()
}

/// Append the bus-factor subsection: a sentence for the repo, then the
/// per-directory breakdown via the shared Markdown table renderer (which
/// escapes cells).
fn write_markdown_bus_factor(out: &mut String, bf: &big_code_analysis::vcs::BusFactor) {
    let _ = writeln!(
        out,
        "\n### Bus factor\n\n_Avelino Degree-of-Authorship, coverage threshold {:.2}._\n",
        bf.coverage_threshold,
    );
    let _ = writeln!(
        out,
        "**Repository:** {} (over {} file(s)).\n",
        bf.repo.bus_factor, bf.repo.files,
    );
    if !bf.by_directory.is_empty() {
        write_md_table(
            out,
            &bus_factor_headers(),
            &bus_factor_aligns(),
            &bus_factor_rows(bf),
        );
    }
}

// --- HTML -------------------------------------------------------------

/// Render the standalone HTML change-history page: the shared scaffolding
/// (inline CSS + click-to-sort JS, so the page sorts like `report.html`)
/// wrapping the change-history section. The page `<h1>` (from
/// `write_html_head`) is the heading, so the body carries no `<h2>`.
pub(crate) fn render_html(report: &Report) -> String {
    let mut out = String::with_capacity(8 * 1024 + report.files.len() * 128);
    write_html_head(&mut out, HEADING, HEADING);
    let _ = out.write_str("<section>\n");
    write_html_body(&mut out, report);
    let _ = out.write_str("</section>\n");
    write_html_tail(&mut out);
    out
}

/// Append the change-history `<section>` under a level-2 heading, for
/// embedding in the aggregated `bca report --vcs` page (before its
/// closing tail).
pub(crate) fn push_html_section(out: &mut String, report: &Report) {
    let _ = out.write_str("<section>\n");
    let _ = writeln!(out, "<h2>{HEADING}</h2>");
    write_html_body(out, report);
    let _ = out.write_str("</section>\n");
}

/// Provenance summary + sortable table (or the empty-set message). No
/// `<section>` wrapper or heading — the caller supplies those.
fn write_html_body(out: &mut String, report: &Report) {
    let _ = out.write_str("<div class=\"summary\">\n");
    let _ = writeln!(out, "<p>{}</p>", provenance(report));
    if report.truncated_shallow_clone {
        let _ = writeln!(out, "<p class=\"note\">{SHALLOW_NOTE}</p>");
    }
    let _ = out.write_str("</div>\n");
    if report.files.is_empty() {
        let _ = writeln!(out, "<p>{EMPTY_MESSAGE}</p>");
    } else {
        let rows: Vec<Vec<String>> = cell_rows(report)
            .into_iter()
            .map(|row| row.into_iter().map(cell_text).collect())
            .collect();
        // Tint only the risk cell, by relative rank. `report.files` is
        // risk-ranked, so the row index is the rank; the Markdown path
        // (which never calls this) stays plain text.
        let risk_col = risk_column_index();
        let n_rows = rows.len();
        write_html_table_classed(out, &headers(), &aligns(), &rows, |r, c| {
            if Some(c) == risk_col {
                risk_heat_class(r, n_rows)
            } else {
                None
            }
        });
    }
    if let Some(aggregate) = &report.vcs_aggregate {
        write_html_bus_factor(out, &aggregate.bus_factor);
    }
}

/// Append the bus-factor subsection (repo sentence + per-directory table)
/// to the HTML body, delegating the table to the shared renderer (which
/// escapes cells).
fn write_html_bus_factor(out: &mut String, bf: &big_code_analysis::vcs::BusFactor) {
    let _ = out.write_str("<section class=\"bus-factor\">\n");
    let _ = writeln!(out, "<h3>Bus factor</h3>");
    let _ = writeln!(
        out,
        "<p class=\"summary\">Avelino Degree-of-Authorship, coverage threshold {:.2}. \
         Repository: <strong>{}</strong> (over {} file(s)).</p>",
        bf.coverage_threshold, bf.repo.bus_factor, bf.repo.files,
    );
    if !bf.by_directory.is_empty() {
        write_html_table(
            out,
            &bus_factor_headers(),
            &bus_factor_aligns(),
            &bus_factor_rows(bf),
        );
    }
    let _ = out.write_str("</section>\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use big_code_analysis::wire;

    /// Build a `FileEntry` with the given path/risk and a couple of
    /// signal fields varied so the columns carry distinct, checkable
    /// values across rows.
    fn entry(path: &str, risk: f64, recent: u32, hotspot: Option<f64>) -> FileEntry {
        FileEntry {
            path: path.to_owned(),
            vcs: wire::Vcs {
                vcs_schema_version: 1,
                risk_score_version: 1,
                long_window_days: 365,
                recent_window_days: 90,
                commits_long: recent + 10,
                commits_recent: recent,
                churn_long: u64::from(recent) * 100,
                churn_recent: u64::from(recent) * 40,
                authors_long: 3,
                authors_recent: 2,
                ownership_top_share: 0.75,
                burst: 0.5,
                bug_fix_commits: 4,
                security_fix_commits: 1,
                revert_commits: 0,
                age_days: 200,
                last_modified_days: 5,
                change_entropy_long: 2.5,
                change_entropy_recent: 1.5,
                cochange_entropy_long: 1.2,
                cochange_entropy_recent: 0.8,
                risk_score: risk,
                hotspot_score: hotspot,
                author_ids: None,
            },
        }
    }

    /// A small ranked report: descending risk, one row with a hotspot
    /// score and one without (the `bca vcs` vs `metrics --vcs` split).
    fn rich_report() -> Report {
        Report {
            long_window_days: 365,
            recent_window_days: 90,
            risk_score_version: 1,
            vcs_schema_version: 1,
            truncated_shallow_clone: false,
            vcs_aggregate: Some(sample_aggregate()),
            files: vec![
                entry("src/hot.rs", 9.4, 50, Some(123.0)),
                entry("src/warm.rs", 6.1, 20, None),
                entry("docs/with|pipe.md", 2.0, 1, None),
            ],
        }
    }

    /// A small, fixed bus-factor aggregate so the rich-report snapshots
    /// exercise the rendered `### Bus factor` / `<section>` blocks.
    fn sample_aggregate() -> big_code_analysis::vcs::VcsAggregate {
        use big_code_analysis::vcs::{
            BUS_FACTOR_SCHEMA_VERSION, BusFactor, DirectoryBusFactor, GroupBusFactor, VcsAggregate,
        };
        let group = |bus_factor, files, authors| GroupBusFactor {
            bus_factor,
            files,
            authors,
            key_author_ids: None,
        };
        VcsAggregate {
            bus_factor: BusFactor {
                schema_version: BUS_FACTOR_SCHEMA_VERSION,
                coverage_threshold: 0.5,
                doa_threshold: 0.75,
                repo: group(2, 3, 4),
                by_directory: vec![
                    DirectoryBusFactor {
                        directory: "docs".to_owned(),
                        group: group(1, 1, 1),
                    },
                    DirectoryBusFactor {
                        directory: "src".to_owned(),
                        group: group(2, 2, 3),
                    },
                ],
            },
        }
    }

    /// Inner text of every `<td …>…</td>` cell, in document order.
    fn html_cell_texts(html: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = html;
        while let Some(open) = rest.find("<td") {
            rest = &rest[open..];
            let Some(gt) = rest.find('>') else { break };
            rest = &rest[gt + 1..];
            let Some(close) = rest.find("</td>") else {
                break;
            };
            out.push(rest[..close].to_owned());
            rest = &rest[close + "</td>".len()..];
        }
        out
    }

    /// Unescaped File-column value of every Markdown data row, in order.
    /// Layout is `| Rank | File | … |`, so the File cell is field index 2.
    /// A GFM-escaped `\|` inside a cell is hidden before the naive split
    /// (otherwise it would be mistaken for a column separator) and
    /// restored after, so the returned path is the logical (unescaped)
    /// value.
    fn md_file_column(md: &str) -> Vec<String> {
        const PIPE_SENTINEL: char = '\u{0}';
        // Scope to the ranked-files table, before the bus-factor
        // subsection (whose own pipe table would otherwise be parsed as
        // extra file rows).
        let files_md = md.split("### Bus factor").next().unwrap_or(md);
        files_md
            .lines()
            .filter(|l| l.starts_with('|'))
            .skip(2) // header + separator
            .map(|l| {
                l.replace("\\|", &PIPE_SENTINEL.to_string())
                    .split('|')
                    .nth(2)
                    .unwrap_or("")
                    .trim()
                    .replace(PIPE_SENTINEL, "|")
            })
            .collect()
    }

    #[test]
    fn markdown_report_renders_all_rows() {
        let report = rich_report();
        let md = render_markdown(&report);
        // expected: heading, provenance, and one data row per file in
        // ranked order; the `|`-bearing path is GFM-escaped.
        assert!(md.starts_with("# Change-history risk\n"));
        assert!(md.contains("risk formula v1, schema v1."));
        assert_eq!(
            md_file_column(&md),
            ["src/hot.rs", "src/warm.rs", "docs/with|pipe.md"],
        );
        // The `|` in the path must be GFM-escaped so it does not break the
        // table column structure.
        assert!(md.contains("docs/with\\|pipe.md"));
        insta::assert_snapshot!("vcs_report_markdown_rich", md);
    }

    #[test]
    fn html_report_is_self_contained_and_sortable() {
        let report = rich_report();
        let html = render_html(&report);
        // expected: shared scaffolding (doctype + inline sort JS + the
        // sortable hotspot table the AST report uses) so the page sorts
        // on header click exactly like report.html.
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<table class=\"hotspot\">"));
        assert!(html.contains("aria-sort")); // inline sort JS present
        // Standalone page heading comes from the shared scaffolding `<h1>`.
        assert!(html.contains("<h1>Change-history risk</h1>"));
        // The `|` in the path is a literal here (HTML needs no GFM
        // escaping), proving each format escapes for its own target.
        assert!(html.contains("docs/with|pipe.md"));
        insta::assert_snapshot!("vcs_report_html_rich", html);
    }

    #[test]
    fn markdown_and_html_columns_match() {
        // The cross-format parity guard (mirrors the AST report's
        // `html_and_markdown_report_identical_section_membership`): both
        // renderers consume the one `VCS_SPECS`, so headers and row order
        // must be identical. A renderer that dropped or reordered a
        // column would diverge here.
        let report = rich_report();
        let md = render_markdown(&report);
        let html = render_html(&report);

        for header in headers() {
            assert!(
                md.contains(header),
                "Markdown missing column header {header:?}"
            );
            assert!(
                html.contains(&format!(">{header}</th>")),
                "HTML missing column header {header:?}"
            );
        }

        // File column appears at the same index in every row of both
        // formats, so the ordered File lists must match. Scope to the
        // ranked-files table, before the bus-factor section's own table.
        let files_html = html
            .split("<section class=\"bus-factor\">")
            .next()
            .unwrap_or(&html);
        let html_files: Vec<String> = html_cell_texts(files_html)
            .chunks(VCS_SPECS.len())
            .map(|row| row[1].clone())
            .collect();
        // Both extractors return the logical (unescaped) path, so the
        // parity check is about row order, not per-format escaping.
        let md_files = md_file_column(&md);
        assert_eq!(md_files, html_files);
        assert_eq!(md_files, ["src/hot.rs", "src/warm.rs", "docs/with|pipe.md"]);
    }

    #[test]
    fn empty_report_emits_message_not_table() {
        let report = Report {
            files: Vec::new(),
            // No matched files means no authorship to aggregate, so the
            // bus-factor section is absent too.
            vcs_aggregate: None,
            ..rich_report()
        };
        let md = render_markdown(&report);
        let html = render_html(&report);
        assert!(md.contains(EMPTY_MESSAGE));
        assert!(!md.contains("| Rank |"));
        assert!(html.contains(EMPTY_MESSAGE));
        assert!(!html.contains("<table class=\"hotspot\">"));
    }

    #[test]
    fn every_vcs_header_carries_a_tooltip() {
        // Drive from the catalogue itself (mirrors the AST report's
        // `metric_headers_carry_tooltips`): every change-history column
        // tooltip must render as a `title="…"` attribute, so a new VCS
        // column is required to document itself without anyone
        // remembering to update this test.
        use crate::html_report::VCS_HEADER_TOOLTIPS;
        let html = render_html(&rich_report());
        // No VCS tooltip string contains an HTML metacharacter, so the
        // rendered attribute is the verbatim tip; embedding it ties the
        // title to its own header (a divergence surfaces as a miss).
        for &(header, tip) in VCS_HEADER_TOOLTIPS {
            let needle = format!(" title=\"{tip}\">{header}</th>");
            assert!(
                html.contains(&needle),
                "VCS header {header:?} should render with its tooltip; expected {needle:?}"
            );
        }
    }

    #[test]
    fn shallow_clone_note_appears_in_both_formats() {
        let report = Report {
            truncated_shallow_clone: true,
            ..rich_report()
        };
        assert!(render_markdown(&report).contains(SHALLOW_NOTE));
        assert!(render_html(&report).contains(SHALLOW_NOTE));
    }

    /// The `risk-heat-N` class on each ranked row's Risk `<td>`, in
    /// document order, read straight off the rendered tag so the test is
    /// tied to the actual attribute. Scoped to the `<tbody>` of the
    /// ranked-files table (before the bus-factor section, which has no
    /// risk column).
    fn risk_heat_classes(html: &str) -> Vec<String> {
        let risk_col = risk_column_index().expect("VCS_SPECS has a Risk column");
        let files_html = html
            .split("<section class=\"bus-factor\">")
            .next()
            .unwrap_or(html);
        let mut rest = files_html.split_once("<tbody>").map_or("", |(_, b)| b);
        let mut classes = Vec::new();
        while let Some(open) = rest.find("<tr>") {
            let row = &rest[open + "<tr>".len()..];
            let Some(close) = row.find("</tr>") else {
                break;
            };
            // The risk cell looks like ` class="numeric risk-heat-0">9.4`.
            let cell = row[..close].split("<td").nth(risk_col + 1).unwrap_or("");
            let heat = cell
                .split_once("risk-heat-")
                .and_then(|(_, c)| c.split(['"', ' ', '>']).next())
                .map(|n| format!("risk-heat-{n}"))
                .unwrap_or_default();
            classes.push(heat);
            rest = &row[close + "</tr>".len()..];
        }
        classes
    }

    #[test]
    fn risk_cell_is_heated_by_rank_not_value() {
        // Raw scores are tightly clustered and all high (98..94) — an
        // *absolute*-threshold scheme would paint every row the same
        // severe band. Relativity demands the top row get the most-severe
        // class and the bottom the least, purely by rank. Five rows over
        // five bands map rank r -> band r.
        let report = Report {
            vcs_aggregate: None,
            files: (0..5)
                .map(|i| entry(&format!("f{i}.rs"), 98.0 - f64::from(i), 5 - i, None))
                .collect(),
            ..rich_report()
        };
        let html = render_html(&report);
        assert_eq!(
            risk_heat_classes(&html),
            (0..5).map(|i| format!("risk-heat-{i}")).collect::<Vec<_>>(),
        );
        // Exactly one heated cell per row — the band lands on a single
        // column. Scan the body (the `<style>` block also names classes).
        let body = html.rsplit_once("</style>").map_or("", |(_, b)| b);
        assert_eq!(body.matches("risk-heat-").count(), report.files.len());
        // The Markdown path carries no heat classes whatsoever.
        assert!(!render_markdown(&report).contains("risk-heat-"));
    }

    #[test]
    fn risk_heat_band_edges_are_relative() {
        // Unit check of the rank->band mapping and documented edge cases.
        assert_eq!(risk_heat_class(0, 1), Some("risk-heat-4")); // lone row: least-severe
        assert_eq!(risk_heat_class(0, 5), Some("risk-heat-0")); // top: most-severe
        assert_eq!(risk_heat_class(4, 5), Some("risk-heat-4")); // bottom: least-severe
        // Remainder rows: 3 over 5 bands -> 0, 1, 3 (no div-by-zero, in range).
        assert_eq!(risk_heat_class(1, 3), Some("risk-heat-1"));
        assert_eq!(risk_heat_class(2, 3), Some("risk-heat-3"));
        // The rich report's three rows render bands 0, 1, 3 in order.
        assert_eq!(
            risk_heat_classes(&render_html(&rich_report())),
            ["risk-heat-0", "risk-heat-1", "risk-heat-3"],
        );
    }

    #[test]
    fn heat_classes_have_light_and_dark_css() {
        // Every `risk-heat-N` band needs a light-mode rule and a dark-mode
        // override so the value text stays WCAG-AA legible in both schemes.
        // Assert against the rendered `<style>`; the heat dark-mode rules
        // are the *last* `@media` adapter (the palette emits an earlier one).
        let html = render_html(&rich_report());
        let dark_block = html
            .rsplit_once("@media (prefers-color-scheme:dark){")
            .expect("dark-mode adapter present")
            .1;
        for class in HEAT_CLASSES {
            let light = format!("td.{class}{{background:");
            assert!(
                html.contains(&light),
                "missing light-mode CSS for {class:?}: expected substring {light:?}"
            );
            assert!(
                dark_block.contains(&light),
                "missing dark-mode override for {class:?}: expected {light:?} in @media block"
            );
        }
    }
}
