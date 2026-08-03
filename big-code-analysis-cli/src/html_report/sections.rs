//! Per-section HTML report renderers (summary, overview, hotspots).

use super::*;

/// Collects heading ids during body rendering: assigns a unique, slug-based
/// `id` to every `h2`/`h3` (deduplicating collisions with a `-2`, `-3`, …
/// suffix so two languages' "Summary" headings never share a fragment), and
/// records the `h2` sections plus their nested `h3` subsections for the
/// table-of-contents `<nav>`.
///
/// The TOC nests each language's `h3` hotspot subsections under its `h2`
/// entry as a collapsible list (issue #685), reusing the per-language-unique
/// ids `unique_id` mints so a reader can jump straight to one hotspot table.
/// (The original `h2`-only "compact nav" — issue #622 — abandoned a reader to
/// scrolling once per-language sections grew this many subsections.)
#[derive(Default)]
pub(crate) struct Headings {
    /// Slug -> number of times already emitted, for `-N` de-duplication.
    seen: HashMap<String, usize>,
    /// One entry per `h2`, in document order, each carrying its `h3` children.
    pub(crate) toc: Vec<TocSection>,
}

impl Headings {
    /// Reserve a unique id for `basis`, recording the collision count so a
    /// repeated basis (e.g. each language's "Summary") gets `-2`, `-3`, ….
    fn unique_id(&mut self, basis: &str) -> String {
        let slug = slugify(basis);
        let count = self.seen.entry(slug.clone()).or_insert(0);
        *count += 1;
        if *count == 1 {
            slug
        } else {
            format!("{slug}-{count}")
        }
    }

    /// Record a TOC `h3` child under the most recent `h2`. A stray `h3` with
    /// no preceding `h2` is dropped from the TOC (it cannot nest under
    /// anything) — this never happens in practice, since every `h3` the
    /// renderers emit sits inside a language `<section>` whose `h2` came
    /// first.
    fn push_child(&mut self, text: String, id: String) {
        if let Some(section) = self.toc.last_mut() {
            section.children.push((text, id));
        }
    }

    /// Emit `<h2 id="…">text</h2>` and open a new TOC section. `id_basis` is
    /// the slug source (the raw language slug or a section title);
    /// `display_text` is the already-`escape_html`-ed visible text.
    pub(crate) fn emit_h2(&mut self, out: &mut String, id_basis: &str, display_text: &str) {
        let id = self.unique_id(id_basis);
        let _ = write!(out, "<h2 id=\"{id}\">{display_text}</h2>");
        self.toc.push(TocSection {
            text: display_text.to_owned(),
            id,
            children: Vec::new(),
        });
    }

    /// Emit `<h3 id="…">text</h3>` and nest it under the current `h2` in the
    /// TOC. `id_basis` is the slug source; `display_text` is already
    /// `escape_html`-ed.
    pub(crate) fn emit_h3(&mut self, out: &mut String, id_basis: &str, display_text: &str) {
        let id = self.unique_id(id_basis);
        let _ = writeln!(out, "<h3 id=\"{id}\">{display_text}</h3>");
        self.push_child(display_text.to_owned(), id);
    }

    /// Emit `<hN id="…">text</hN>` at a caller-chosen `level` (used by the
    /// VCS report, whose bus-factor subsection sits at `h2` on the
    /// standalone page and `h3` when embedded). A level-2 heading opens a new
    /// TOC section; a level-3 heading nests under the current one, matching
    /// `emit_h2`/`emit_h3`. `display_text` is already `escape_html`-ed.
    pub(crate) fn emit_heading(
        &mut self,
        out: &mut String,
        level: usize,
        id_basis: &str,
        display_text: &str,
    ) {
        let id = self.unique_id(id_basis);
        let _ = writeln!(out, "<h{level} id=\"{id}\">{display_text}</h{level}>");
        if level == 2 {
            self.toc.push(TocSection {
                text: display_text.to_owned(),
                id,
                children: Vec::new(),
            });
        } else if level == 3 {
            self.push_child(display_text.to_owned(), id);
        }
    }
}

/// Render a hotspot table from the shared [`Column`] descriptors. Builds the
/// parallel `headers`/`aligns`/`rows` arrays and delegates to [`write_table`],
/// the single source of truth for table bytes and escaping. HTML escapes
/// every cell uniformly, so the raw [`Cell`] payload (regardless of kind) is
/// handed to `write_table`, whose `escape_html` runs exactly once per cell.
fn write_hotspot_table(out: &mut String, spec: &HotspotSpec, entries: &[&FunctionSummary]) {
    let columns = spec.columns;
    let mut headers = Vec::with_capacity(columns.len());
    let mut aligns = Vec::with_capacity(columns.len());
    let mut tooltips = Vec::with_capacity(columns.len());
    for col in columns {
        headers.push(col.header);
        aligns.push(col.align);
        tooltips.push(col.tooltip);
    }
    let mut rows: Vec<Vec<String>> = Vec::with_capacity(entries.len());
    for s in entries {
        let mut row: Vec<String> = Vec::with_capacity(columns.len());
        for col in columns {
            let (Cell::Name(text) | Cell::Path(text) | Cell::Num(text)) = (col.cell)(s);
            row.push(text);
        }
        rows.push(row);
    }
    // The column spec carries the authoritative tooltip; pass it
    // positionally so each header's `title=` matches the legend exactly.
    // The spec also names the pre-ranked column and its direction, so the
    // table announces its initial sort order without renderer guesswork.
    let ranked = RankedColumn {
        index: spec.rank_col,
        dir: spec.dir.aria_sort(),
    };
    write_table_with_tooltips(out, &headers, &aligns, &tooltips, Some(ranked), &rows);
}

/// Per-language grouping of summaries, keyed by `LANG::name()` and
/// ordered alphabetically (so the report sections are deterministic).
pub(crate) type LangGroups<'a> = BTreeMap<&'a str, Vec<&'a FunctionSummary>>;

/// Group summaries by language name. The `BTreeMap` ordering drives the
/// alphabetical section order asserted by
/// `two_language_well_formed_and_alphabetical`.
pub(crate) fn group_by_language(summaries: &[FunctionSummary]) -> LangGroups<'_> {
    let mut map = LangGroups::new();
    for s in summaries {
        map.entry(s.language.name()).or_default().push(s);
    }
    map
}

/// Comment lines as a percentage of source lines, guarding the
/// zero-SLOC case. Shared by the global and per-language roll-ups so the
/// formula lives in one place.
fn comment_ratio_percent(sloc: usize, cloc: usize) -> f64 {
    if sloc > 0 {
        (cloc as f64 / sloc as f64) * 100.0
    } else {
        0.0
    }
}

/// Whole-walk roll-up shown in the global `<div class="summary">` block.
/// Only `SpaceKind::Unit` summaries contribute file-level line counts;
/// functions and class-likes are counted separately.
pub(crate) struct GlobalTotals {
    files: usize,
    sloc: usize,
    ploc: usize,
    cloc: usize,
    functions: usize,
    classes: usize,
}

impl GlobalTotals {
    pub(crate) fn from_summaries(summaries: &[FunctionSummary]) -> Self {
        let mut t = Self {
            files: 0,
            sloc: 0,
            ploc: 0,
            cloc: 0,
            functions: 0,
            classes: 0,
        };
        for s in summaries {
            match s.kind {
                SpaceKind::Unit => {
                    t.files += 1;
                    t.sloc += s.sloc;
                    t.ploc += s.ploc;
                    t.cloc += s.cloc;
                }
                SpaceKind::Function => t.functions += 1,
                _ => {}
            }
            if is_class_like(s.kind) {
                t.classes += 1;
            }
        }
        t
    }

    fn comment_ratio(&self) -> f64 {
        comment_ratio_percent(self.sloc, self.cloc)
    }
}

pub(crate) fn write_global_summary(
    out: &mut String,
    totals: &GlobalTotals,
    by_lang: &LangGroups<'_>,
) {
    let languages_list: String = by_lang
        .keys()
        .map(|k| language_display_name(k))
        .collect::<Vec<_>>()
        .join(", ");

    let _ = out.write_str("<div class=\"summary\">\n");
    let _ = writeln!(
        out,
        "<p><strong>Files analyzed:</strong> {} <strong>Languages:</strong> {}</p>",
        escape_html(&thousands(totals.files)),
        escape_html(&languages_list),
    );
    // PLOC / Comments carry hover tooltips (the legend defines them too —
    // issue #679); SLOC's definition already rides the hotspot SLOC column.
    let _ = writeln!(
        out,
        "<p><strong>Total SLOC:</strong> {} \
         <strong title=\"{}\">PLOC:</strong> {} \
         <strong title=\"{}\">Comments:</strong> {}</p>",
        escape_html(&thousands(totals.sloc)),
        escape_html(hotspot::PLOC_TOOLTIP),
        escape_html(&thousands(totals.ploc)),
        escape_html(hotspot::COMMENTS_TOOLTIP),
        escape_html(&thousands(totals.cloc)),
    );
    let _ = writeln!(
        out,
        "<p><strong>Functions/methods:</strong> {} <strong>Types:</strong> {}</p>",
        escape_html(&thousands(totals.functions)),
        escape_html(&thousands(totals.classes)),
    );
    let _ = writeln!(
        out,
        "<p><strong>Comment ratio:</strong> {:.1}%</p>",
        totals.comment_ratio()
    );
    let _ = out.write_str("</div>\n");
}

/// Build the seven-cell per-language overview row (Files / SLOC /
/// Functions averaged from the unit and function summaries of one
/// language).
fn overview_row(lang_name: &str, lang_summaries: &[&FunctionSummary]) -> Vec<String> {
    let mut unit_count = 0usize;
    let mut sloc = 0usize;
    let mut mi_numerator = 0.0f64;
    let mut func_count = 0usize;
    let mut cc_sum = 0.0f64;
    let mut cog_sum = 0.0f64;
    for s in lang_summaries {
        match s.kind {
            SpaceKind::Unit => {
                unit_count += 1;
                sloc += s.sloc;
                mi_numerator += mi_weight_numerator(s);
            }
            SpaceKind::Function => {
                func_count += 1;
                cc_sum += s.cyclomatic;
                cog_sum += s.cognitive;
            }
            _ => {}
        }
    }
    let avg_mi = sloc_weighted_avg_mi(mi_numerator, sloc);
    let (avg_cc, avg_cog) = if func_count > 0 {
        (cc_sum / func_count as f64, cog_sum / func_count as f64)
    } else {
        (0.0, 0.0)
    };
    vec![
        language_display_name(lang_name).into_owned(),
        thousands(unit_count),
        thousands(sloc),
        thousands(func_count),
        format!("{avg_mi:.1}"),
        format!("{avg_cc:.1}"),
        format!("{avg_cog:.1}"),
    ]
}

pub(crate) fn write_overview_table(
    out: &mut String,
    headings: &mut Headings,
    by_lang: &LangGroups<'_>,
) {
    headings.emit_h2(out, "Per-language overview", "Per-language overview");
    let _ = out.write_str("\n");
    // One discoverability hint near the first table: the columns are
    // sortable, which is otherwise invisible until a reader happens to
    // click a header (issue #622).
    let _ = out.write_str("<p class=\"sort-hint\">Click a column header to sort any table.</p>\n");
    let mut rows: Vec<Vec<String>> = Vec::with_capacity(by_lang.len());
    for (&lang_name, lang_summaries) in by_lang {
        rows.push(overview_row(lang_name, lang_summaries));
    }
    write_table(
        out,
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
        &rows,
    );
}

/// Render the global cross-language Actionable Summary near the TOC: the
/// per-language counts summed over every function in the walk (issue #678).
/// Suppression policy does not gate these counts — like the per-language
/// Actionable Summary, this is a raw whole-codebase health signal, not a
/// gate (issue #501). Renders the "no concerns" line when nothing clears a
/// threshold, mirroring the per-language block.
pub(crate) fn write_global_actionable_summary(
    out: &mut String,
    summaries: &[FunctionSummary],
    advisory: AdvisoryThresholds,
) {
    let funcs: Vec<&FunctionSummary> = summaries
        .iter()
        .filter(|s| s.kind == SpaceKind::Function)
        .collect();
    let counts = advisory.count_over(&funcs);
    let _ = out.write_str(
        "<div class=\"summary global-actionable\">\n<p><strong>Actionable summary \
         (all languages)</strong></p>\n",
    );
    // Provenance so the global roll-up's cutoffs are attributable (issue #630).
    let _ = writeln!(
        out,
        "<p class=\"note\">{}</p>",
        escape_html(advisory.provenance_line())
    );
    if counts.all_clear() {
        let _ = out.write_str("<p class=\"note\">No major quality concerns detected.</p>\n");
        let _ = out.write_str("</div>\n");
        return;
    }
    write_actionable_bullets(out, counts, advisory);
    let _ = out.write_str("</div>\n");
}

/// Emit a visible legend listing each metric column's abbreviation and its
/// one-line definition, drawn from the same `(header, tooltip)` pairs the
/// hover tooltips use so the two cannot drift. The `<details>` is rendered
/// **open** — the legend's whole purpose is to survive print, mobile, and
/// screen readers, all of which a collapsed `<details>` defeats just as the
/// hover tooltips do (issue #679); the page bottom is cheap vertical space.
/// Renders nothing when `entries` is empty.
pub(crate) fn write_legend_html(out: &mut String, entries: &[(&str, &str)]) {
    if entries.is_empty() {
        return;
    }
    let _ = out.write_str("<details class=\"legend\" open>\n<summary>Legend</summary>\n<dl>\n");
    for (header, tip) in entries {
        // Link the term to its hosted metric chapter when one exists (issue
        // #675); VCS / identity headers have no anchor and render bare.
        let term = match hotspot::metric_doc_url(header) {
            Some(url) => format!(
                "<a href=\"{}\">{}</a>",
                escape_html(&url),
                escape_html(header)
            ),
            None => escape_html(header).into_owned(),
        };
        let _ = writeln!(out, "<dt>{term}</dt><dd>{}</dd>", escape_html(tip));
    }
    let _ = out.write_str("</dl>\n</details>\n");
}

/// File-level roll-up backing one language's `<h3>Summary</h3>` note.
/// Only `SpaceKind::Unit` summaries feed it.
struct LanguageTotals {
    files: usize,
    sloc: usize,
    ploc: usize,
    cloc: usize,
    /// SLOC-weighted numerator of the unclamped Visual Studio MI: the sum
    /// of [`mi_weight_numerator`] over the units. Divided by `sloc` (not
    /// `files`) to form the headline average (issue #725).
    mi_numerator: f64,
}

impl LanguageTotals {
    fn from_units(units: &[&FunctionSummary]) -> Self {
        let mut t = Self {
            files: 0,
            sloc: 0,
            ploc: 0,
            cloc: 0,
            mi_numerator: 0.0,
        };
        for s in units {
            t.files += 1;
            t.sloc += s.sloc;
            t.ploc += s.ploc;
            t.cloc += s.cloc;
            t.mi_numerator += mi_weight_numerator(s);
        }
        t
    }

    fn comment_ratio(&self) -> f64 {
        comment_ratio_percent(self.sloc, self.cloc)
    }

    fn avg_mi(&self) -> f64 {
        sloc_weighted_avg_mi(self.mi_numerator, self.sloc)
    }
}

fn write_language_header(out: &mut String, headings: &mut Headings, lang_name: &str) {
    let display_name = language_display_name(lang_name);
    // `slug` is sourced from `LANGUAGE_PALETTE` (or the literal "other"
    // fallback) — always lowercase ASCII, so it is interpolated raw
    // into the class attribute without `escape_html`.
    let slug = language_palette_slug(lang_name);
    let _ = write!(out, "<section class=\"lang-section lang-{slug}\">");
    // The heading id is slug-based off the raw language name (`lang_name`,
    // e.g. "cpp"/"csharp"), NOT the display name — so `C++` deep-links to a
    // valid `#cpp` fragment rather than the punctuation-laden display text.
    headings.emit_h2(out, lang_name, &escape_html(&display_name));
    let _ = out.write_str("\n");
}

fn write_language_summary(
    out: &mut String,
    headings: &mut Headings,
    id_prefix: &str,
    units: &[&FunctionSummary],
) {
    let totals = LanguageTotals::from_units(units);
    let cr = totals.comment_ratio();
    let avg_mi = totals.avg_mi();
    let rating = mi_rating(avg_mi);

    headings.emit_h3(out, &format!("{id_prefix}-summary"), "Summary");
    let _ = writeln!(
        out,
        "<p class=\"note\">Files: {} | SLOC: {} | PLOC: {} | Comment ratio: {cr:.1}%</p>",
        escape_html(&thousands(totals.files)),
        escape_html(&thousands(totals.sloc)),
        escape_html(&thousands(totals.ploc)),
    );
    let _ = writeln!(
        out,
        "<p class=\"note\">{}: {avg_mi:.1} ({rating})</p>",
        crate::markdown_report::AVG_MI_LABEL
    );
}

/// Emit one hotspot section: an `<h3>` heading followed by the column-driven
/// table, from a shared [`HotspotSpec`] and its already-selected `rows`. The
/// logical title is `escape_html`-ed here defensively (a `>` in a `concept`
/// would become `&gt;`); since no current concept carries one (see
/// [`hotspot::HotspotTitle::render`]), it is a no-op for today's
/// metachar-free titles.
fn emit_html_section(
    out: &mut String,
    headings: &mut Headings,
    id_prefix: &str,
    spec: &HotspotSpec,
    top_n: usize,
    rows: &[&FunctionSummary],
) {
    let title = spec.title.render(top_n);
    // The id basis is the language slug plus the section's *stable* basis
    // ("<Concept> hotspots", without the `(top N by …)` clause), so the
    // fragment (`#rust-cyclomatic-complexity-hotspots`) does not shift with
    // `--top` (issue #677). The displayed text is the full rendered title.
    headings.emit_h3(
        out,
        &format!("{id_prefix}-{}", spec.title.id_basis()),
        &escape_html(&title),
    );
    write_hotspot_table(out, spec, rows);
}

/// The cyclomatic summary note under the CC hotspot table. A caption over the
/// same suppression-filtered set the table shows (see [`hotspot::select_cc`]),
/// matching the Markdown report; the raw, suppression-independent CC count
/// lives in the Actionable Summary instead. When `policy` honors suppression
/// the line is captioned `(excluding suppressed functions)` so a reader can
/// tell the two CC figures apart (issue #616).
fn emit_cc_note_html(
    out: &mut String,
    stats: &hotspot::CyclomaticStats,
    policy: SuppressionPolicy,
) {
    let caption = hotspot::cc_note_caption(policy)
        .map(|c| format!(" ({})", escape_html(c)))
        .unwrap_or_default();
    // Bands use the resolved advisory CC cutoff and its severe multiple
    // (issue #630): `> 10` / `> 20` by default, shifted by a manifest
    // `cyclomatic = N`.
    let _ = writeln!(
        out,
        "<p class=\"note\">Average CC: {:.1} | Max: {:.0} | CC &gt; {:.0}: {} functions | CC &gt; {:.0}: {} functions{caption}</p>",
        stats.avg(),
        stats.max,
        stats.primary_cutoff,
        stats.over_primary,
        stats.severe_cutoff,
        stats.over_severe,
    );
}

/// Emit the `<ul>` of advisory bullets shared by the per-language and global
/// Actionable Summaries, the resolved advisory cutoff embedded in each label
/// (issue #630). The caller has already emitted the heading / provenance /
/// caption and confirmed `!counts.all_clear()`.
fn write_actionable_bullets(
    out: &mut String,
    counts: AdvisoryCounts,
    advisory: AdvisoryThresholds,
) {
    let _ = out.write_str("<ul>\n");
    if counts.cc > 0 {
        let _ = writeln!(
            out,
            "<li><strong>{}</strong> functions with CC &gt; {:.0}</li>",
            counts.cc, advisory.cc
        );
    }
    if counts.cognitive > 0 {
        let _ = writeln!(
            out,
            "<li><strong>{}</strong> functions with cognitive complexity &gt; {:.0}</li>",
            counts.cognitive, advisory.cognitive
        );
    }
    if counts.sloc > 0 {
        let _ = writeln!(
            out,
            "<li><strong>{}</strong> functions with SLOC &gt; {}</li>",
            counts.sloc, advisory.sloc
        );
    }
    if counts.nargs > 0 {
        let _ = writeln!(
            out,
            "<li><strong>{}</strong> functions with more than {} parameters</li>",
            counts.nargs, advisory.nargs
        );
    }
    if counts.bugs > 0 {
        let _ = writeln!(
            out,
            "<li><strong>{}</strong> functions with estimated Halstead bugs &gt; {:.1}</li>",
            counts.bugs, advisory.bugs
        );
    }
    let _ = out.write_str("</ul>\n");
}

fn write_actionable_summary(
    out: &mut String,
    headings: &mut Headings,
    id_prefix: &str,
    funcs: &[&FunctionSummary],
    policy: SuppressionPolicy,
    advisory: AdvisoryThresholds,
) {
    let counts = advisory.count_over(funcs);
    headings.emit_h3(
        out,
        &format!("{id_prefix}-actionable-summary"),
        "Actionable Summary",
    );
    // Provenance so the cutoffs are always attributable (issue #630).
    let _ = writeln!(
        out,
        "<p class=\"note\">{}</p>",
        escape_html(advisory.provenance_line())
    );
    let breakdown = hotspot::suppressed_metric_breakdown(funcs, policy, advisory);
    let _ = writeln!(
        out,
        "<p class=\"note\">{}</p>",
        escape_html(&hotspot::actionable_summary_caption(&breakdown))
    );
    if counts.all_clear() {
        let _ = out.write_str("<p class=\"note\">No major quality concerns detected.</p>\n");
        return;
    }
    write_actionable_bullets(out, counts, advisory);
}

/// Emit the section heading (`<h3>` + id) followed by the "table omitted: all
/// N matching functions suppressed" caption, in place of a hotspot table that
/// was rendered empty *solely because* suppression hid every matching row
/// (`count > 0`). The heading uses the same id basis `emit_html_section` does,
/// so deep links resolve regardless of suppression state (issue #681).
/// Mirrors the Markdown renderer's `emit_fully_suppressed_note_md` so a
/// summary bullet never points at a table absent from the document (issue
/// #616). A no-op when `count == 0`.
fn emit_fully_suppressed_note_html(
    out: &mut String,
    headings: &mut Headings,
    id_prefix: &str,
    spec: &HotspotSpec,
    top_n: usize,
    count: usize,
) {
    if count == 0 {
        return;
    }
    let title = spec.title.render(top_n);
    // Emit the section heading with the same stable id basis
    // `emit_html_section` uses, so a fully-suppressed section keeps its place
    // in the heading/id sequence and deep links resolve regardless of
    // suppression state (issue #681). The omission note is the section body.
    headings.emit_h3(
        out,
        &format!("{id_prefix}-{}", spec.title.id_basis()),
        &escape_html(&title),
    );
    let _ = writeln!(
        out,
        "<p class=\"note\">{}</p>",
        escape_html(&hotspot::fully_suppressed_caption(&title, count))
    );
}

pub(crate) fn write_language_section(
    out: &mut String,
    headings: &mut Headings,
    lang_name: &str,
    entries: &[&FunctionSummary],
    top_n: usize,
    policy: SuppressionPolicy,
    advisory: AdvisoryThresholds,
) {
    write_language_header(out, headings, lang_name);
    // Slug-based id prefix for this language's `h3` headings, so a hotspot
    // table deep-links to e.g. `#rust-cyclomatic-complexity-hotspots`. Built
    // from the raw language slug, matching the `h2` section id.
    let id_prefix = slugify(lang_name);
    let (units, funcs) = hotspot::partition_by_kind(entries);
    write_language_summary(out, headings, &id_prefix, &units);

    // The Actionable Summary leads the section (directly after Summary,
    // before any hotspot table) so a reader sees the highest-altitude counts
    // first (issue #678).
    write_actionable_summary(out, headings, &id_prefix, &funcs, policy, advisory);

    // Drive every hotspot section from the shared `SPECS` table so the HTML
    // and Markdown reports cannot diverge in membership/order/suppression.
    // `select_for` owns the whole format-independent half, leaving the four
    // emit calls below as the only thing this renderer decides (#1190).
    for spec in SPECS {
        match hotspot::select_for(spec, &units, &funcs, entries, top_n, policy, advisory) {
            hotspot::SpecOutcome::FullySuppressed(suppressed) => {
                emit_fully_suppressed_note_html(out, headings, &id_prefix, spec, top_n, suppressed);
            }
            hotspot::SpecOutcome::Rows { rows, cc_stats } => {
                emit_html_section(out, headings, &id_prefix, spec, top_n, &rows);
                if let Some(stats) = cc_stats {
                    emit_cc_note_html(out, &stats, policy);
                } else if spec.mi_note {
                    // HTML wraps the note in a styled `<p>`; the Markdown
                    // writer has a dedicated emitter for the same text.
                    let _ = writeln!(
                        out,
                        "<p class=\"note\">{}</p>",
                        escape_html(hotspot::MI_NOTE)
                    );
                }
            }
        }
    }

    let _ = out.write_str("</section>\n");
}
