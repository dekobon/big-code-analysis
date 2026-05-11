use std::collections::BTreeMap;
use std::fmt::Write;

use big_code_analysis::{FuncSpace, LANG, SpaceKind};

use crate::format_util::MetricScalar;

/// Compact per-function/class metric record for the markdown report pipeline.
#[derive(Debug)]
pub(crate) struct FunctionSummary {
    pub file: String,
    pub name: String,
    pub kind: SpaceKind,
    pub language: LANG,
    pub start_line: usize,
    #[allow(dead_code)]
    pub end_line: usize,
    pub sloc: usize,
    pub ploc: usize,
    #[expect(dead_code)]
    pub lloc: usize,
    pub cloc: usize,
    pub tokens: usize,
    pub cyclomatic: f64,
    pub cognitive: f64,
    pub halstead_volume: f64,
    #[expect(dead_code)]
    pub halstead_difficulty: f64,
    pub halstead_effort: f64,
    pub halstead_bugs: f64,
    #[expect(dead_code)]
    pub halstead_time: f64,
    #[expect(dead_code)]
    pub mi_original: f64,
    #[expect(dead_code)]
    pub mi_sei: f64,
    pub mi_visual_studio: f64,
    pub nargs: usize,
    pub nexits: usize,
    pub nom: usize,
    pub abc: f64,
    pub wmc: f64,
    pub npa: f64,
    pub npm: f64,
}

/// Recursively extract [`FunctionSummary`] records from a [`FuncSpace`] tree.
///
/// `strip_prefix` is applied to `file` using `str::strip_prefix` semantics:
/// if the file path starts with the prefix it is removed, otherwise the path
/// is kept as-is.
pub(crate) fn extract_summaries(
    space: &FuncSpace,
    file: &str,
    language: LANG,
    strip_prefix: &str,
    out: &mut Vec<FunctionSummary>,
) {
    let display_file = file.strip_prefix(strip_prefix).unwrap_or(file);
    extract_summaries_inner(space, display_file, language, out);
}

fn extract_summaries_inner(
    space: &FuncSpace,
    display_file: &str,
    language: LANG,
    out: &mut Vec<FunctionSummary>,
) {
    let m = &space.metrics;
    out.push(FunctionSummary {
        file: display_file.to_string(),
        name: space.name.clone().unwrap_or_default(),
        kind: space.kind,
        language,
        start_line: space.start_line,
        end_line: space.end_line,
        sloc: m.loc.sloc() as usize,
        ploc: m.loc.ploc() as usize,
        lloc: m.loc.lloc() as usize,
        cloc: m.loc.cloc() as usize,
        tokens: m.tokens.tokens_sum() as usize,
        cyclomatic: m.cyclomatic.cyclomatic(),
        cognitive: m.cognitive.cognitive(),
        halstead_volume: m.halstead.volume(),
        halstead_difficulty: m.halstead.difficulty(),
        halstead_effort: m.halstead.effort(),
        halstead_bugs: m.halstead.bugs(),
        halstead_time: m.halstead.time(),
        mi_original: m.mi.mi_original(),
        mi_sei: m.mi.mi_sei(),
        mi_visual_studio: m.mi.mi_visual_studio(),
        nargs: m.nargs.nargs_total() as usize,
        nexits: m.nexits.exit_sum() as usize,
        nom: m.nom.total() as usize,
        abc: m.abc.magnitude(),
        wmc: m.wmc.total_wmc(),
        npa: m.npa.total_npa(),
        npm: m.npm.total_npm(),
    });

    for child in &space.spaces {
        extract_summaries_inner(child, display_file, language, out);
    }
}

// ---------------------------------------------------------------------------
// Report generation
// ---------------------------------------------------------------------------

fn escape_cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\n', '\r'], " ")
}

fn escape_name(s: &str) -> String {
    let sanitized = s.replace('`', "\u{02CB}");
    format!("`{}`", escape_cell(&sanitized))
}

pub(super) fn thousands(n: usize) -> String {
    let s = n.to_string();
    let len = s.len();
    if len <= 3 {
        return s;
    }
    let mut result = String::with_capacity(len + (len - 1) / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(ch);
    }
    result
}

pub(super) fn title_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for c in s.chars() {
        if capitalize_next {
            result.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
        if matches!(c, '/' | ' ' | '-') {
            capitalize_next = true;
        }
    }
    result
}

pub(super) fn sort_by_metric_desc(
    items: &mut [&FunctionSummary],
    metric: impl Fn(&FunctionSummary) -> f64,
) {
    items.sort_by(|a, b| {
        metric(b)
            .total_cmp(&metric(a))
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.start_line.cmp(&b.start_line))
            .then_with(|| a.name.cmp(&b.name))
    });
}

pub(super) fn sort_by_metric_asc(
    items: &mut [&FunctionSummary],
    metric: impl Fn(&FunctionSummary) -> f64,
) {
    items.sort_by(|a, b| {
        metric(a)
            .total_cmp(&metric(b))
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.start_line.cmp(&b.start_line))
            .then_with(|| a.name.cmp(&b.name))
    });
}

pub(super) fn is_class_like(kind: SpaceKind) -> bool {
    matches!(
        kind,
        SpaceKind::Class
            | SpaceKind::Struct
            | SpaceKind::Trait
            | SpaceKind::Impl
            | SpaceKind::Namespace
            | SpaceKind::Interface
    )
}

pub(super) fn mi_rating(mi: f64) -> &'static str {
    if mi >= 20.0 {
        "GOOD"
    } else if mi >= 10.0 {
        "MODERATE"
    } else {
        "LOW"
    }
}

#[derive(Clone, Copy)]
enum Align {
    Left,
    Right,
}

/// Write a GFM pipe table with column widths padded to the longest
/// header / cell, so the raw text aligns when viewed in a plain-text
/// terminal. Padded tables remain valid GFM and render identically in
/// GitHub, mdBook, and pulldown-cmark.
///
/// Each row must have the same length as `headers` and `aligns`.
/// Cells are taken verbatim — escape any pipes / newlines with
/// [`escape_cell`] / [`escape_name`] before calling.
fn write_table(out: &mut String, headers: &[&str], aligns: &[Align], rows: &[Vec<String>]) {
    debug_assert_eq!(headers.len(), aligns.len());
    let widths: Vec<usize> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let cell_w = rows.iter().map(|r| r[i].chars().count()).max().unwrap_or(0);
            // Min 3 keeps the separator (`---` / `--:`) unambiguous for GFM.
            h.chars().count().max(cell_w).max(3)
        })
        .collect();

    let push_cell = |out: &mut String, cell: &str, width: usize, align: Align| {
        let pad = width - cell.chars().count();
        out.push(' ');
        match align {
            Align::Left => {
                out.push_str(cell);
                out.extend(std::iter::repeat_n(' ', pad));
            }
            Align::Right => {
                out.extend(std::iter::repeat_n(' ', pad));
                out.push_str(cell);
            }
        }
        out.push(' ');
    };

    out.push('|');
    for (i, h) in headers.iter().enumerate() {
        push_cell(out, h, widths[i], aligns[i]);
        out.push('|');
    }
    out.push('\n');

    out.push('|');
    for (i, &a) in aligns.iter().enumerate() {
        out.push(' ');
        match a {
            Align::Left => out.extend(std::iter::repeat_n('-', widths[i])),
            Align::Right => {
                out.extend(std::iter::repeat_n('-', widths[i] - 1));
                out.push(':');
            }
        }
        out.push(' ');
        out.push('|');
    }
    out.push('\n');

    for row in rows {
        debug_assert_eq!(row.len(), headers.len());
        out.push('|');
        for (i, cell) in row.iter().enumerate() {
            push_cell(out, cell, widths[i], aligns[i]);
            out.push('|');
        }
        out.push('\n');
    }
}

/// Produce a Markdown quality-metrics report from the collected summaries.
///
/// `top_n` controls how many entries appear in each hotspot table.
pub(crate) fn generate_report(summaries: &[FunctionSummary], top_n: usize) -> String {
    let mut out = String::new();

    // Group by language display name (BTreeMap → deterministic alphabetical order).
    let by_lang = {
        let mut map = BTreeMap::<&str, Vec<&FunctionSummary>>::new();
        for s in summaries {
            map.entry(s.language.get_name()).or_default().push(s);
        }
        map
    };

    // ── Global header ───────────────────────────────────────────────────
    let (total_files, total_sloc, total_ploc, total_cloc, total_functions, total_classes) =
        summaries.iter().fold(
            (0usize, 0usize, 0usize, 0usize, 0usize, 0usize),
            |(files, sloc, ploc, cloc, funcs, classes), s| {
                (
                    files + usize::from(s.kind == SpaceKind::Unit),
                    sloc + if s.kind == SpaceKind::Unit { s.sloc } else { 0 },
                    ploc + if s.kind == SpaceKind::Unit { s.ploc } else { 0 },
                    cloc + if s.kind == SpaceKind::Unit { s.cloc } else { 0 },
                    funcs + usize::from(s.kind == SpaceKind::Function),
                    classes + usize::from(is_class_like(s.kind)),
                )
            },
        );
    let comment_ratio = if total_sloc > 0 {
        (total_cloc as f64 / total_sloc as f64) * 100.0
    } else {
        0.0
    };

    let languages_list: String = by_lang
        .keys()
        .map(|k| title_case(k))
        .collect::<Vec<_>>()
        .join(", ");

    let _ = writeln!(out, "# Code Quality Metrics Summary\n");
    let _ = writeln!(
        out,
        "**Files analyzed:** {}    **Languages:** {}",
        thousands(total_files),
        languages_list,
    );
    let _ = writeln!(
        out,
        "**Total SLOC:** {}  **PLOC:** {}  **Comments:** {}",
        thousands(total_sloc),
        thousands(total_ploc),
        thousands(total_cloc),
    );
    let _ = writeln!(
        out,
        "**Functions/methods:** {}    **Classes/impls/traits:** {}",
        thousands(total_functions),
        thousands(total_classes),
    );
    let _ = writeln!(out, "**Comment ratio:** {comment_ratio:.1}%");

    if by_lang.is_empty() {
        return out;
    }

    // ── Per-language overview table ─────────────────────────────────────
    let _ = writeln!(out, "\n## Per-language overview\n");
    let mut overview_rows: Vec<Vec<String>> = Vec::with_capacity(by_lang.len());
    for (&lang_name, lang_summaries) in &by_lang {
        let (lang_unit_count, lang_sloc, mi_sum) = lang_summaries
            .iter()
            .filter(|s| s.kind == SpaceKind::Unit)
            .fold((0usize, 0usize, 0.0f64), |(c, sl, mi), s| {
                (c + 1, sl + s.sloc, mi + s.mi_visual_studio)
            });
        let avg_mi = if lang_unit_count > 0 {
            mi_sum / lang_unit_count as f64
        } else {
            0.0
        };
        let (func_count, avg_cc, avg_cog) = {
            let (count, cc_sum, cog_sum) = lang_summaries
                .iter()
                .filter(|s| s.kind == SpaceKind::Function)
                .fold((0usize, 0.0f64, 0.0f64), |(c, cc, cog), s| {
                    (c + 1, cc + s.cyclomatic, cog + s.cognitive)
                });
            if count > 0 {
                (count, cc_sum / count as f64, cog_sum / count as f64)
            } else {
                (0, 0.0, 0.0)
            }
        };

        overview_rows.push(vec![
            title_case(lang_name),
            thousands(lang_unit_count),
            thousands(lang_sloc),
            thousands(func_count),
            format!("{avg_mi:.1}"),
            format!("{avg_cc:.1}"),
            format!("{avg_cog:.1}"),
        ]);
    }
    write_table(
        &mut out,
        &[
            "Language",
            "Files",
            "SLOC",
            "Functions",
            "Avg MI",
            "Avg CC",
            "Avg Cognitive",
        ],
        &[
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
        ],
        &overview_rows,
    );

    // ── Per-language sections ───────────────────────────────────────────
    for (&lang_name, lang_summaries) in &by_lang {
        write_language_section(&mut out, lang_name, lang_summaries, top_n);
    }

    out
}

fn write_language_section(
    out: &mut String,
    lang_name: &str,
    entries: &[&FunctionSummary],
    top_n: usize,
) {
    let display_name = title_case(lang_name);
    let _ = writeln!(out, "\n## {display_name}\n");

    let units: Vec<&FunctionSummary> = entries
        .iter()
        .filter(|s| s.kind == SpaceKind::Unit)
        .copied()
        .collect();
    let funcs: Vec<&FunctionSummary> = entries
        .iter()
        .filter(|s| s.kind == SpaceKind::Function)
        .copied()
        .collect();

    // ── Summary ────────────────────────────────────────────────────
    {
        let (files, sloc, ploc, cloc, mi_sum) = units.iter().fold(
            (0usize, 0usize, 0usize, 0usize, 0.0f64),
            |(f, sl, pl, cl, mi), s| {
                (
                    f + 1,
                    sl + s.sloc,
                    pl + s.ploc,
                    cl + s.cloc,
                    mi + s.mi_visual_studio,
                )
            },
        );
        let cr = if sloc > 0 {
            (cloc as f64 / sloc as f64) * 100.0
        } else {
            0.0
        };
        let avg_mi = if files > 0 {
            mi_sum / files as f64
        } else {
            0.0
        };
        let rating = mi_rating(avg_mi);

        let _ = writeln!(out, "### Summary\n");
        let _ = writeln!(
            out,
            "Files: {} | SLOC: {} | PLOC: {} | Comment ratio: {cr:.1}%",
            thousands(files),
            thousands(sloc),
            thousands(ploc),
        );
        let _ = writeln!(out, "Average MI: {avg_mi:.1} ({rating})");
    }

    // ── Maintainability Index (lowest files) ───────────────────────
    {
        let mut mi_entries: Vec<&FunctionSummary> = units
            .iter()
            .filter(|s| s.mi_visual_studio > 0.0)
            .copied()
            .collect();
        if !mi_entries.is_empty() {
            sort_by_metric_asc(&mut mi_entries, |s| s.mi_visual_studio);
            let count = mi_entries.len().min(top_n);

            let _ = writeln!(
                out,
                "\n### Maintainability Index (lowest files, top-{top_n})\n"
            );
            let rows: Vec<Vec<String>> = mi_entries[..count]
                .iter()
                .map(|s| {
                    vec![
                        escape_cell(&s.file),
                        format!("{:.1}", s.mi_visual_studio),
                        thousands(s.sloc),
                        thousands(s.tokens),
                    ]
                })
                .collect();
            write_table(
                out,
                &["File", "MI", "SLOC", "Tokens"],
                &[Align::Left, Align::Right, Align::Right, Align::Right],
                &rows,
            );
        }
    }

    // ── Cyclomatic Complexity Hotspots ──────────────────────────────
    {
        let mut cc_entries: Vec<&FunctionSummary> = funcs
            .iter()
            .filter(|s| s.cyclomatic > 0.0)
            .copied()
            .collect();
        if !cc_entries.is_empty() {
            let (cc_sum, cc_count, max_cc, count_gt10, count_gt20) = cc_entries.iter().fold(
                (0.0f64, 0usize, f64::NAN, 0usize, 0usize),
                |(sum, cnt, mx, g10, g20), s| {
                    let c = s.cyclomatic;
                    (
                        sum + c,
                        cnt + 1,
                        f64::max(mx, c),
                        g10 + usize::from(c > 10.0),
                        g20 + usize::from(c > 20.0),
                    )
                },
            );
            let avg_cc = if cc_count > 0 {
                cc_sum / cc_count as f64
            } else {
                0.0
            };

            sort_by_metric_desc(&mut cc_entries, |s| s.cyclomatic);
            let count = cc_entries.len().min(top_n);

            let _ = writeln!(out, "\n### Cyclomatic Complexity Hotspots\n");
            let rows: Vec<Vec<String>> = cc_entries[..count]
                .iter()
                .map(|s| {
                    vec![
                        escape_name(&s.name),
                        escape_cell(&s.file),
                        s.start_line.to_string(),
                        MetricScalar(s.cyclomatic).to_string(),
                        MetricScalar(s.cognitive).to_string(),
                        thousands(s.sloc),
                        thousands(s.tokens),
                    ]
                })
                .collect();
            write_table(
                out,
                &[
                    "Function",
                    "File",
                    "Line",
                    "CC",
                    "Cognitive",
                    "SLOC",
                    "Tokens",
                ],
                &[
                    Align::Left,
                    Align::Left,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                ],
                &rows,
            );
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "Average CC: {avg_cc:.1} | Max: {max_cc:.0} | CC > 10: {count_gt10} functions | CC > 20: {count_gt20} functions",
            );
        }
    }

    // ── Cognitive Complexity Hotspots ───────────────────────────────
    {
        let mut cog_entries: Vec<&FunctionSummary> = funcs
            .iter()
            .filter(|s| s.cognitive > 0.0)
            .copied()
            .collect();
        if !cog_entries.is_empty() {
            sort_by_metric_desc(&mut cog_entries, |s| s.cognitive);
            let count = cog_entries.len().min(top_n);

            let _ = writeln!(out, "\n### Cognitive Complexity Hotspots\n");
            let rows: Vec<Vec<String>> = cog_entries[..count]
                .iter()
                .map(|s| {
                    vec![
                        escape_name(&s.name),
                        escape_cell(&s.file),
                        s.start_line.to_string(),
                        MetricScalar(s.cognitive).to_string(),
                        MetricScalar(s.cyclomatic).to_string(),
                        thousands(s.sloc),
                        thousands(s.tokens),
                    ]
                })
                .collect();
            write_table(
                out,
                &[
                    "Function",
                    "File",
                    "Line",
                    "Cognitive",
                    "CC",
                    "SLOC",
                    "Tokens",
                ],
                &[
                    Align::Left,
                    Align::Left,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                ],
                &rows,
            );
        }
    }

    // ── Halstead Effort Hotspots ───────────────────────────────────
    {
        let mut hal_entries: Vec<&FunctionSummary> = funcs
            .iter()
            .filter(|s| s.halstead_effort > 0.0)
            .copied()
            .collect();
        if !hal_entries.is_empty() {
            sort_by_metric_desc(&mut hal_entries, |s| s.halstead_effort);
            let count = hal_entries.len().min(top_n);

            let _ = writeln!(out, "\n### Halstead Effort Hotspots\n");
            let rows: Vec<Vec<String>> = hal_entries[..count]
                .iter()
                .map(|s| {
                    vec![
                        escape_name(&s.name),
                        escape_cell(&s.file),
                        MetricScalar(s.halstead_effort).to_string(),
                        MetricScalar(s.halstead_volume).to_string(),
                        format!("{:.2}", s.halstead_bugs),
                        thousands(s.sloc),
                        thousands(s.tokens),
                    ]
                })
                .collect();
            write_table(
                out,
                &[
                    "Function",
                    "File",
                    "Effort",
                    "Volume",
                    "Est. Bugs",
                    "SLOC",
                    "Tokens",
                ],
                &[
                    Align::Left,
                    Align::Left,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                ],
                &rows,
            );
        }
    }

    // ── Largest Functions by SLOC ──────────────────────────────────
    {
        let mut sloc_entries: Vec<&FunctionSummary> =
            funcs.iter().filter(|s| s.sloc > 0).copied().collect();
        if !sloc_entries.is_empty() {
            sort_by_metric_desc(&mut sloc_entries, |s| s.sloc as f64);
            let count = sloc_entries.len().min(top_n);

            let _ = writeln!(out, "\n### Largest Functions by SLOC\n");
            let rows: Vec<Vec<String>> = sloc_entries[..count]
                .iter()
                .map(|s| {
                    vec![
                        escape_name(&s.name),
                        escape_cell(&s.file),
                        s.start_line.to_string(),
                        thousands(s.sloc),
                        thousands(s.tokens),
                        MetricScalar(s.cyclomatic).to_string(),
                        MetricScalar(s.cognitive).to_string(),
                    ]
                })
                .collect();
            write_table(
                out,
                &[
                    "Function",
                    "File",
                    "Line",
                    "SLOC",
                    "Tokens",
                    "CC",
                    "Cognitive",
                ],
                &[
                    Align::Left,
                    Align::Left,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                ],
                &rows,
            );
        }
    }

    // ── Functions With Many Parameters (>3) ────────────────────────
    {
        let mut nargs_entries: Vec<&FunctionSummary> =
            funcs.iter().filter(|s| s.nargs > 3).copied().collect();
        if !nargs_entries.is_empty() {
            sort_by_metric_desc(&mut nargs_entries, |s| s.nargs as f64);
            let count = nargs_entries.len().min(top_n);

            let _ = writeln!(out, "\n### Functions With Many Parameters (>3)\n");
            let rows: Vec<Vec<String>> = nargs_entries[..count]
                .iter()
                .map(|s| {
                    vec![
                        escape_name(&s.name),
                        escape_cell(&s.file),
                        s.nargs.to_string(),
                        thousands(s.sloc),
                        thousands(s.tokens),
                    ]
                })
                .collect();
            write_table(
                out,
                &["Function", "File", "Args", "SLOC", "Tokens"],
                &[
                    Align::Left,
                    Align::Left,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                ],
                &rows,
            );
        }
    }

    // ── Actionable Summary ─────────────────────────────────────────
    {
        let (cc_gt10, cog_gt15, sloc_gt100, nargs_gt3, bugs_gt1) = funcs.iter().fold(
            (0usize, 0usize, 0usize, 0usize, 0usize),
            |(a, b, c, d, e), s| {
                (
                    a + usize::from(s.cyclomatic > 10.0),
                    b + usize::from(s.cognitive > 15.0),
                    c + usize::from(s.sloc > 100),
                    d + usize::from(s.nargs > 3),
                    e + usize::from(s.halstead_bugs > 1.0),
                )
            },
        );

        let _ = writeln!(out, "\n### Actionable Summary\n");
        if cc_gt10 == 0 && cog_gt15 == 0 && sloc_gt100 == 0 && nargs_gt3 == 0 && bugs_gt1 == 0 {
            let _ = writeln!(out, "No major quality concerns detected.");
        } else {
            if cc_gt10 > 0 {
                let _ = writeln!(out, "- **{cc_gt10}** functions with CC > 10");
            }
            if cog_gt15 > 0 {
                let _ = writeln!(
                    out,
                    "- **{cog_gt15}** functions with cognitive complexity > 15"
                );
            }
            if sloc_gt100 > 0 {
                let _ = writeln!(out, "- **{sloc_gt100}** functions with SLOC > 100");
            }
            if nargs_gt3 > 0 {
                let _ = writeln!(
                    out,
                    "- **{nargs_gt3}** functions with more than 3 parameters"
                );
            }
            if bugs_gt1 > 0 {
                let _ = writeln!(
                    out,
                    "- **{bugs_gt1}** functions with estimated Halstead bugs > 1.0"
                );
            }
        }
    }

    // ── Class/Trait/Impl Hotspots (WMC) ────────────────────────────
    {
        let mut class_entries: Vec<&FunctionSummary> = entries
            .iter()
            .filter(|s| is_class_like(s.kind) && s.wmc > 0.0)
            .copied()
            .collect();
        if !class_entries.is_empty() {
            sort_by_metric_desc(&mut class_entries, |s| s.wmc);
            let count = class_entries.len().min(top_n);

            let _ = writeln!(out, "\n### Class/Trait/Impl Hotspots (WMC)\n");
            let rows: Vec<Vec<String>> = class_entries[..count]
                .iter()
                .map(|s| {
                    vec![
                        escape_name(&s.name),
                        escape_cell(&s.file),
                        s.start_line.to_string(),
                        MetricScalar(s.wmc).to_string(),
                        s.nom.to_string(),
                        MetricScalar(s.npa).to_string(),
                        MetricScalar(s.npm).to_string(),
                        thousands(s.sloc),
                        thousands(s.tokens),
                    ]
                })
                .collect();
            write_table(
                out,
                &[
                    "Class", "File", "Line", "WMC", "Methods", "NPA", "NPM", "SLOC", "Tokens",
                ],
                &[
                    Align::Left,
                    Align::Left,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                ],
                &rows,
            );
        }
    }

    // ── Functions with the most exit points (NEXITS) ───────────────
    {
        let mut nexits_entries: Vec<&FunctionSummary> =
            funcs.iter().filter(|s| s.nexits > 0).copied().collect();
        if !nexits_entries.is_empty() {
            sort_by_metric_desc(&mut nexits_entries, |s| s.nexits as f64);
            let count = nexits_entries.len().min(top_n);

            let _ = writeln!(out, "\n### Functions with the most exit points (NEXITS)\n");
            let rows: Vec<Vec<String>> = nexits_entries[..count]
                .iter()
                .map(|s| {
                    vec![
                        escape_name(&s.name),
                        escape_cell(&s.file),
                        s.start_line.to_string(),
                        s.nexits.to_string(),
                        MetricScalar(s.cyclomatic).to_string(),
                        thousands(s.sloc),
                        thousands(s.tokens),
                    ]
                })
                .collect();
            write_table(
                out,
                &["Function", "File", "Line", "Exits", "CC", "SLOC", "Tokens"],
                &[
                    Align::Left,
                    Align::Left,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                ],
                &rows,
            );
        }
    }

    // ── ABC Magnitude Hotspots ─────────────────────────────────────
    {
        let mut abc_entries: Vec<&FunctionSummary> =
            funcs.iter().filter(|s| s.abc > 0.0).copied().collect();
        if !abc_entries.is_empty() {
            sort_by_metric_desc(&mut abc_entries, |s| s.abc);
            let count = abc_entries.len().min(top_n);

            let _ = writeln!(out, "\n### ABC Magnitude Hotspots\n");
            let rows: Vec<Vec<String>> = abc_entries[..count]
                .iter()
                .map(|s| {
                    vec![
                        escape_name(&s.name),
                        escape_cell(&s.file),
                        s.start_line.to_string(),
                        format!("{:.1}", s.abc),
                        thousands(s.sloc),
                        thousands(s.tokens),
                    ]
                })
                .collect();
            write_table(
                out,
                &["Function", "File", "Line", "ABC", "SLOC", "Tokens"],
                &[
                    Align::Left,
                    Align::Left,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                    Align::Right,
                ],
                &rows,
            );
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]
mod tests {
    use super::*;
    use big_code_analysis::{CodeMetrics, FuncSpace, SpaceKind};

    /// Collapse runs of spaces to a single space so assertions can match
    /// the logical row content regardless of column-padding width.
    fn collapse_spaces(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut prev_space = false;
        for c in s.chars() {
            if c == ' ' {
                if !prev_space {
                    out.push(' ');
                }
                prev_space = true;
            } else {
                out.push(c);
                prev_space = false;
            }
        }
        out
    }

    fn make_space(name: &str, kind: SpaceKind, start: usize, end: usize) -> FuncSpace {
        FuncSpace {
            name: Some(name.to_string()),
            name_was_lossy: false,
            start_line: start,
            end_line: end,
            kind,
            spaces: Vec::new(),
            metrics: CodeMetrics::default(),
        }
    }

    fn make_summary(name: &str, file: &str, kind: SpaceKind, language: LANG) -> FunctionSummary {
        FunctionSummary {
            file: file.to_string(),
            name: name.to_string(),
            kind,
            language,
            start_line: 1,
            end_line: 10,
            sloc: 20,
            ploc: 25,
            lloc: 15,
            cloc: 5,
            tokens: 30,
            cyclomatic: 3.0,
            cognitive: 2.0,
            halstead_volume: 100.0,
            halstead_difficulty: 5.0,
            halstead_effort: 500.0,
            halstead_bugs: 0.1,
            halstead_time: 28.0,
            mi_original: 80.0,
            mi_sei: 85.0,
            mi_visual_studio: 50.0,
            nargs: 2,
            nexits: 1,
            nom: 1,
            abc: 5.0,
            wmc: 3.0,
            npa: 0.0,
            npm: 0.0,
        }
    }

    // ── extract_summaries tests ────────────────────────────────────

    #[test]
    fn extract_single_space() {
        let space = make_space("root.rs", SpaceKind::Unit, 1, 10);
        let mut out = Vec::new();
        extract_summaries(&space, "src/root.rs", LANG::Rust, "", &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].file, "src/root.rs");
        assert_eq!(out[0].name, "root.rs");
        assert_eq!(out[0].kind, SpaceKind::Unit);
        assert_eq!(out[0].start_line, 1);
        assert_eq!(out[0].end_line, 10);
    }

    #[test]
    fn extract_nested_spaces() {
        let mut root = make_space("root.rs", SpaceKind::Unit, 1, 20);
        let func_a = make_space("func_a", SpaceKind::Function, 2, 8);
        let mut class_b = make_space("ClassB", SpaceKind::Class, 10, 18);
        let func_c = make_space("method_c", SpaceKind::Function, 12, 16);
        class_b.spaces.push(func_c);
        root.spaces.push(func_a);
        root.spaces.push(class_b);

        let mut out = Vec::new();
        extract_summaries(&root, "src/root.rs", LANG::Rust, "", &mut out);

        assert_eq!(out.len(), 4);
        assert_eq!(out[0].kind, SpaceKind::Unit);
        assert_eq!(out[1].kind, SpaceKind::Function);
        assert_eq!(out[1].name, "func_a");
        assert_eq!(out[2].kind, SpaceKind::Class);
        assert_eq!(out[2].name, "ClassB");
        assert_eq!(out[3].kind, SpaceKind::Function);
        assert_eq!(out[3].name, "method_c");
        assert_eq!(out[3].start_line, 12);
        assert_eq!(out[3].end_line, 16);
    }

    #[test]
    fn strip_prefix_removes_matching_prefix() {
        let space = make_space("root.rs", SpaceKind::Unit, 1, 5);
        let mut out = Vec::new();
        extract_summaries(&space, "src/lib/root.rs", LANG::Rust, "src/lib/", &mut out);
        assert_eq!(out[0].file, "root.rs");
    }

    #[test]
    fn strip_prefix_passthrough_on_mismatch() {
        let space = make_space("root.rs", SpaceKind::Unit, 1, 5);
        let mut out = Vec::new();
        extract_summaries(&space, "other/root.rs", LANG::Rust, "src/lib/", &mut out);
        assert_eq!(out[0].file, "other/root.rs");
    }

    #[test]
    fn empty_tree_produces_one_summary() {
        let space = make_space("empty.rs", SpaceKind::Unit, 0, 0);
        let mut out = Vec::new();
        extract_summaries(&space, "empty.rs", LANG::Rust, "", &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn language_propagated_to_all_children() {
        let mut root = make_space("root.py", SpaceKind::Unit, 1, 10);
        root.spaces.push(make_space("f", SpaceKind::Function, 2, 5));

        let mut out = Vec::new();
        extract_summaries(&root, "root.py", LANG::Python, "", &mut out);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|s| s.language == LANG::Python));
    }

    // ── generate_report tests ──────────────────────────────────────

    #[test]
    fn two_language_report_contains_both_sections() {
        let summaries = vec![
            make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust),
            make_summary("do_stuff", "src/lib.rs", SpaceKind::Function, LANG::Rust),
            make_summary("main.py", "main.py", SpaceKind::Unit, LANG::Python),
            make_summary("run", "main.py", SpaceKind::Function, LANG::Python),
        ];
        let report = generate_report(&summaries, 20);

        assert!(report.contains("## Rust"), "missing Rust section header");
        assert!(
            report.contains("## Python"),
            "missing Python section header"
        );
        assert!(
            report.contains("## Per-language overview"),
            "missing overview"
        );

        // Overview table has a row for each language. Padding can vary,
        // so collapse runs of spaces before matching.
        let normalized = collapse_spaces(&report);
        assert!(
            normalized.contains("| Rust |"),
            "missing Rust overview row in:\n{report}"
        );
        assert!(
            normalized.contains("| Python |"),
            "missing Python overview row in:\n{report}"
        );

        // Global header reflects correct totals.
        assert!(report.contains("**Files analyzed:** 2"));
        assert!(report.contains("**Functions/methods:** 2"));
    }

    #[test]
    fn halstead_section_omitted_when_no_effort() {
        let mut unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        unit.halstead_effort = 0.0;
        let mut func = make_summary("f", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        func.halstead_effort = 0.0;
        func.halstead_volume = 0.0;
        func.halstead_bugs = 0.0;

        let report = generate_report(&[unit, func], 20);
        assert!(
            !report.contains("### Halstead Effort Hotspots"),
            "Halstead section should be omitted"
        );
    }

    #[test]
    fn top_n_truncation() {
        let mut summaries = Vec::new();
        summaries.push(make_summary(
            "lib.rs",
            "src/lib.rs",
            SpaceKind::Unit,
            LANG::Rust,
        ));
        for i in 0..30 {
            let mut f = make_summary(
                &format!("func_{i}"),
                "src/lib.rs",
                SpaceKind::Function,
                LANG::Rust,
            );
            f.start_line = i + 1;
            f.cyclomatic = (i + 1) as f64;
            f.cognitive = (i + 1) as f64;
            f.halstead_effort = (i + 1) as f64 * 100.0;
            f.sloc = (i + 1) * 5;
            summaries.push(f);
        }
        let report = generate_report(&summaries, 5);

        // Count data rows (lines starting with "| `") in each section.
        let sections = [
            "### Cyclomatic Complexity Hotspots",
            "### Cognitive Complexity Hotspots",
            "### Halstead Effort Hotspots",
            "### Largest Functions by SLOC",
        ];
        for section_hdr in sections {
            let section_start = report
                .find(section_hdr)
                .unwrap_or_else(|| panic!("missing section: {section_hdr}"));
            let section_text = &report[section_start..];
            // Section ends at the next "###" or "##" or end of string.
            let section_end = section_text[1..]
                .find("\n## ")
                .or_else(|| section_text[1..].find("\n### "))
                .map_or(section_text.len(), |p| p + 1);
            let section_body = &section_text[..section_end];
            let data_rows = section_body
                .lines()
                .filter(|l| l.starts_with("| `"))
                .count();
            assert_eq!(
                data_rows, 5,
                "expected 5 data rows in {section_hdr}, got {data_rows}"
            );
        }
    }

    #[test]
    fn determinism() {
        let summaries = vec![
            make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust),
            make_summary("alpha", "src/lib.rs", SpaceKind::Function, LANG::Rust),
            make_summary("beta", "src/lib.rs", SpaceKind::Function, LANG::Rust),
            make_summary("main.py", "main.py", SpaceKind::Unit, LANG::Python),
            make_summary("run", "main.py", SpaceKind::Function, LANG::Python),
        ];
        let a = generate_report(&summaries, 10);
        let b = generate_report(&summaries, 10);
        assert_eq!(a, b, "report must be byte-equal across runs");
    }

    #[test]
    fn cell_escaping_pipe() {
        let mut f = make_summary("foo|bar", "dir/a|b.rs", SpaceKind::Function, LANG::Rust);
        f.cyclomatic = 5.0;
        let unit = make_summary("a|b.rs", "dir/a|b.rs", SpaceKind::Unit, LANG::Rust);
        let report = generate_report(&[unit, f], 20);
        // The pipe inside the name must be escaped.
        assert!(
            report.contains("foo\\|bar"),
            "pipe in name not escaped: {report}"
        );
        assert!(
            report.contains("a\\|b.rs"),
            "pipe in file not escaped: {report}"
        );
    }

    #[test]
    fn cell_escaping_backtick() {
        let mut f = make_summary("foo`bar", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        f.cyclomatic = 5.0;
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let report = generate_report(&[unit, f], 20);
        // Backtick in a name is replaced with modifier letter grave accent.
        assert!(
            report.contains("foo\u{02CB}bar"),
            "backtick in name not replaced"
        );
    }

    #[test]
    fn nan_safe_sort_does_not_panic() {
        let mut unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        unit.mi_visual_studio = f64::NAN;
        let mut f = make_summary("f", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        f.cyclomatic = f64::NAN;
        f.cognitive = f64::NAN;
        f.halstead_effort = f64::NAN;
        // Must not panic.
        let report = generate_report(&[unit, f], 20);
        assert!(report.contains("# Code Quality Metrics Summary"));
    }

    #[test]
    fn sort_by_metric_desc_handles_nan() {
        let mut a = make_summary("a", "a.rs", SpaceKind::Function, LANG::Rust);
        a.cyclomatic = f64::NAN;
        let mut b = make_summary("b", "b.rs", SpaceKind::Function, LANG::Rust);
        b.cyclomatic = 5.0;
        let mut c = make_summary("c", "c.rs", SpaceKind::Function, LANG::Rust);
        c.cyclomatic = 10.0;

        let mut items: Vec<&FunctionSummary> = vec![&a, &b, &c];
        sort_by_metric_desc(&mut items, |s| s.cyclomatic);
        // Must not panic. total_cmp treats NaN as greater than all values,
        // so NaN sorts first in descending order.
        assert_eq!(items[0].name, "a");
        // Non-NaN values are in descending order after NaN.
        assert_eq!(items[1].name, "c");
        assert_eq!(items[2].name, "b");
    }

    #[test]
    fn sort_by_metric_asc_handles_nan() {
        let mut a = make_summary("a", "a.rs", SpaceKind::Unit, LANG::Rust);
        a.mi_visual_studio = f64::NAN;
        let mut b = make_summary("b", "b.rs", SpaceKind::Unit, LANG::Rust);
        b.mi_visual_studio = 30.0;
        let mut c = make_summary("c", "c.rs", SpaceKind::Unit, LANG::Rust);
        c.mi_visual_studio = 10.0;

        let mut items: Vec<&FunctionSummary> = vec![&a, &b, &c];
        sort_by_metric_asc(&mut items, |s| s.mi_visual_studio);
        // Must not panic. Non-NaN values sort ascending, NaN sorts last.
        assert_eq!(items[0].name, "c");
        assert_eq!(items[1].name, "b");
        assert_eq!(items[2].name, "a");
    }

    #[test]
    fn empty_input() {
        let report = generate_report(&[], 20);
        assert!(report.contains("**Files analyzed:** 0"));
        assert!(report.contains("**Functions/methods:** 0"));
        // No per-language sections.
        assert!(!report.contains("## Per-language overview"));
    }

    #[test]
    fn thousands_formatting() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(10_000_000), "10,000,000");
    }

    // ── write_table tests ──────────────────────────────────────────

    #[test]
    fn write_table_pads_left_and_right_columns() {
        let mut out = String::new();
        write_table(
            &mut out,
            &["Name", "Count"],
            &[Align::Left, Align::Right],
            &[
                vec!["a".to_string(), "1".to_string()],
                vec!["longname".to_string(), "1234".to_string()],
            ],
        );
        let expected = "\
| Name     | Count |
| -------- | ----: |
| a        |     1 |
| longname |  1234 |
";
        assert_eq!(out, expected);
    }

    #[test]
    fn write_table_handles_empty_rows() {
        let mut out = String::new();
        write_table(&mut out, &["A", "B"], &[Align::Left, Align::Right], &[]);
        // Header (1-char) and right-align separator both expand to the
        // GFM-minimum width of 3.
        let expected = "\
| A   |   B |
| --- | --: |
";
        assert_eq!(out, expected);
    }

    #[test]
    fn write_table_widens_to_longest_cell() {
        let mut out = String::new();
        write_table(
            &mut out,
            &["X", "Y"],
            &[Align::Left, Align::Right],
            &[vec!["wide-cell".to_string(), "100".to_string()]],
        );
        // X's column widens to 9 (longest cell), Y's to 3 (min).
        let expected = "\
| X         |   Y |
| --------- | --: |
| wide-cell | 100 |
";
        assert_eq!(out, expected);
    }

    #[test]
    fn write_table_counts_chars_not_bytes_for_multibyte_cells() {
        // The grave-accent replacement char (\u{02CB}) is one column in a
        // monospace renderer but takes 3 bytes in UTF-8 — width must use
        // chars().count(), not byte length.
        let mut out = String::new();
        write_table(
            &mut out,
            &["Name"],
            &[Align::Left],
            &[vec!["abc".to_string()], vec!["a\u{02CB}c".to_string()]],
        );
        // Both cells are 3 chars; column width is 3.
        let expected = "\
| Name |
| ---- |
| abc  |
| a\u{02CB}c  |
";
        assert_eq!(out, expected);
    }

    #[test]
    fn title_case_basic() {
        assert_eq!(title_case("rust"), "Rust");
        assert_eq!(title_case("python"), "Python");
        assert_eq!(title_case("c/c++"), "C/C++");
        assert_eq!(title_case(""), "");
    }

    #[test]
    fn escape_name_wraps_in_backticks() {
        assert_eq!(escape_name("hello"), "`hello`");
        assert_eq!(escape_name("a|b"), "`a\\|b`");
        assert_eq!(escape_name("a`b"), "`a\u{02CB}b`");
        assert_eq!(escape_name("a\nb"), "`a b`");
    }

    #[test]
    fn actionable_summary_clean() {
        let summaries = vec![
            make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust),
            make_summary("f", "src/lib.rs", SpaceKind::Function, LANG::Rust),
        ];
        let report = generate_report(&summaries, 20);
        assert!(
            report.contains("No major quality concerns detected."),
            "clean codebase should show no-concerns message"
        );
    }

    #[test]
    fn actionable_summary_with_concerns() {
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut f = make_summary("big_func", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        f.cyclomatic = 25.0;
        f.cognitive = 20.0;
        f.sloc = 150;
        f.nargs = 5;
        f.halstead_bugs = 2.0;

        let report = generate_report(&[unit, f], 20);
        assert!(report.contains("functions with CC > 10"));
        assert!(report.contains("functions with cognitive complexity > 15"));
        assert!(report.contains("functions with SLOC > 100"));
        assert!(report.contains("functions with more than 3 parameters"));
        assert!(report.contains("functions with estimated Halstead bugs > 1.0"));
    }

    #[test]
    fn mi_table_shows_lowest_first() {
        let mut unit_good = make_summary("good.rs", "good.rs", SpaceKind::Unit, LANG::Rust);
        unit_good.mi_visual_studio = 80.0;
        let mut unit_bad = make_summary("bad.rs", "bad.rs", SpaceKind::Unit, LANG::Rust);
        unit_bad.mi_visual_studio = 15.0;

        let report = generate_report(&[unit_good, unit_bad], 20);
        // The bad file should appear first in the MI table.
        let mi_section = report
            .find("### Maintainability Index")
            .expect("MI section missing");
        let after_mi = &report[mi_section..];
        let bad_pos = after_mi.find("bad.rs").expect("bad.rs missing in MI");
        let good_pos = after_mi.find("good.rs").expect("good.rs missing in MI");
        assert!(
            bad_pos < good_pos,
            "lowest MI file should appear first in MI table"
        );
    }

    // ── WMC / NEXITS / ABC section tests ───────────────────────────

    #[test]
    fn wmc_section_present_with_class_summaries() {
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut cls = make_summary("MyClass", "src/lib.rs", SpaceKind::Class, LANG::Rust);
        cls.wmc = 12.0;
        cls.nom = 4;
        cls.npa = 2.0;
        cls.npm = 3.0;
        cls.sloc = 80;
        let func = make_summary("f", "src/lib.rs", SpaceKind::Function, LANG::Rust);

        let report = generate_report(&[unit, cls, func], 20);
        assert!(
            report.contains("### Class/Trait/Impl Hotspots (WMC)"),
            "WMC section should be present when class-kind summaries exist"
        );
        // Verify the row renders the correct metric values. Padding may
        // pad cells with spaces; collapse runs of spaces before matching.
        let normalized = collapse_spaces(&report);
        assert!(
            normalized.contains("| `MyClass`"),
            "class name should appear as backtick-wrapped cell"
        );
        assert!(
            normalized.contains("| 12 | 4 | 2 | 3 | 80 | 30 |"),
            "WMC row should contain wmc=12, nom=4, npa=2, npm=3, sloc=80, tokens=30 in:\n{report}"
        );
    }

    #[test]
    fn wmc_section_omitted_without_classes() {
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let func = make_summary("f", "src/lib.rs", SpaceKind::Function, LANG::Rust);

        let report = generate_report(&[unit, func], 20);
        assert!(
            !report.contains("### Class/Trait/Impl Hotspots (WMC)"),
            "WMC section should be absent when no class-kind summaries exist"
        );
    }

    #[test]
    fn nexits_section_present() {
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut func = make_summary("multi_exit", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        func.nexits = 3;
        func.cyclomatic = 7.0;
        func.sloc = 40;

        let report = generate_report(&[unit, func], 20);
        assert!(
            report.contains("### Functions with the most exit points (NEXITS)"),
            "NEXITS section should be present when functions have exits > 0"
        );
        let normalized = collapse_spaces(&report);
        assert!(
            normalized.contains("| `multi_exit`"),
            "function name should appear as backtick-wrapped cell"
        );
        assert!(
            normalized.contains("| 3 | 7 | 40 | 30 |"),
            "NEXITS row should contain exits=3, cc=7, sloc=40, tokens=30 in:\n{report}"
        );
    }

    #[test]
    fn abc_section_present() {
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut func = make_summary("complex", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        func.abc = 15.5;
        func.sloc = 35;

        let report = generate_report(&[unit, func], 20);
        assert!(
            report.contains("### ABC Magnitude Hotspots"),
            "ABC section should be present when functions have abc > 0"
        );
        let normalized = collapse_spaces(&report);
        assert!(
            normalized.contains("| `complex`"),
            "function name should appear as backtick-wrapped cell"
        );
        assert!(
            normalized.contains("| 15.5 | 35 | 30 |"),
            "ABC row should contain abc=15.5, sloc=35, tokens=30 in:\n{report}"
        );
    }

    #[test]
    fn top_n_truncation_wmc_nexits_abc() {
        let mut summaries = Vec::new();
        summaries.push(make_summary(
            "lib.rs",
            "src/lib.rs",
            SpaceKind::Unit,
            LANG::Rust,
        ));
        // 10 classes for WMC truncation.
        for i in 0..10 {
            let mut cls = make_summary(
                &format!("Class_{i}"),
                "src/lib.rs",
                SpaceKind::Class,
                LANG::Rust,
            );
            cls.wmc = (i + 1) as f64;
            cls.start_line = 100 + i;
            summaries.push(cls);
        }
        // 10 functions for NEXITS and ABC truncation.
        for i in 0..10 {
            let mut f = make_summary(
                &format!("func_{i}"),
                "src/lib.rs",
                SpaceKind::Function,
                LANG::Rust,
            );
            f.nexits = i + 1;
            f.abc = (i + 1) as f64 * 2.0;
            f.start_line = 200 + i;
            summaries.push(f);
        }
        let report = generate_report(&summaries, 3);

        let sections = [
            "### Class/Trait/Impl Hotspots (WMC)",
            "### Functions with the most exit points (NEXITS)",
            "### ABC Magnitude Hotspots",
        ];
        for section_hdr in sections {
            let section_start = report
                .find(section_hdr)
                .unwrap_or_else(|| panic!("missing section: {section_hdr}"));
            let section_text = &report[section_start..];
            let section_end = section_text[1..]
                .find("\n## ")
                .or_else(|| section_text[1..].find("\n### "))
                .map_or(section_text.len(), |p| p + 1);
            let section_body = &section_text[..section_end];
            let data_rows = section_body
                .lines()
                .filter(|l| l.starts_with("| `"))
                .count();
            assert_eq!(
                data_rows, 3,
                "expected 3 data rows in {section_hdr}, got {data_rows}"
            );
        }
    }

    #[test]
    fn tokens_column_present_in_hotspot_tables() {
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut func = make_summary("hot", "src/lib.rs", SpaceKind::Function, LANG::Rust);
        func.cyclomatic = 5.0;
        func.cognitive = 4.0;
        func.halstead_effort = 200.0;
        func.nargs = 4;
        func.nexits = 2;
        func.abc = 8.0;
        func.tokens = 42;
        let mut cls = make_summary("Cls", "src/lib.rs", SpaceKind::Class, LANG::Rust);
        cls.wmc = 6.0;
        cls.tokens = 99;

        let report = generate_report(&[unit, func, cls], 20);

        for header in [
            "### Maintainability Index",
            "### Cyclomatic Complexity Hotspots",
            "### Cognitive Complexity Hotspots",
            "### Halstead Effort Hotspots",
            "### Largest Functions by SLOC",
            "### Functions With Many Parameters (>3)",
            "### Class/Trait/Impl Hotspots (WMC)",
            "### Functions with the most exit points (NEXITS)",
            "### ABC Magnitude Hotspots",
        ] {
            let start = report
                .find(header)
                .unwrap_or_else(|| panic!("missing section: {header}"));
            let header_row = report[start..]
                .lines()
                .find(|l| l.starts_with('|'))
                .expect("header row");
            assert!(
                header_row.contains("Tokens"),
                "Tokens column missing from {header} header row:\n{header_row}"
            );
        }

        let normalized = collapse_spaces(&report);
        assert!(
            normalized.contains("| 42 |"),
            "function token count should appear in normalized report"
        );
        assert!(
            normalized.contains("| 99 |"),
            "class token count should appear in normalized report"
        );
    }

    #[test]
    fn nexits_present_abc_absent() {
        let unit = make_summary("lib.rs", "src/lib.rs", SpaceKind::Unit, LANG::Rust);
        let mut func = make_summary(
            "early_return",
            "src/lib.rs",
            SpaceKind::Function,
            LANG::Rust,
        );
        func.nexits = 2;
        func.abc = 0.0;

        let report = generate_report(&[unit, func], 20);
        assert!(
            report.contains("### Functions with the most exit points (NEXITS)"),
            "NEXITS section should be present"
        );
        assert!(
            !report.contains("### ABC Magnitude Hotspots"),
            "ABC section should be absent when all abc values are 0"
        );
    }
}
