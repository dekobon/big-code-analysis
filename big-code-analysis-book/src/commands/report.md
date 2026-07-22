# Report

`bca report [--format <FORMAT>]` produces an aggregated quality-metrics
report across every file walked. It is designed for pasting into pull
requests, wikis, or issue trackers.

Pick the format with `--format` / `-O` (`bca report --format html`).
When omitted, the report defaults to `markdown`. The bare positional
form (`bca report markdown`) still works as a deprecated alias and is
slated for removal in the next major; prefer `--format`.

> **CI integration.** For runnable GitHub Actions and GitLab CI
> recipes that post the Markdown report as a PR/MR comment, see the
> [CI integration recipe](../recipes/ci.md).

Two formats are available: `markdown` (plain-text, ideal for PR
comments) and `html` (a self-contained dashboard with sortable tables,
ideal for sharing as a build artifact).

> **Migrating?** This command replaces the pre-restructure `--metrics
> -O markdown` invocation. See the [migration guide](../migration.md).

## Quick start

Print to stdout:

```bash
bca report --paths /path/to/project markdown
```

Write to a file:

```bash
bca report --paths /path/to/project markdown --output report.md
```

> **Note:** `--output` must be a *file* path, not a directory.

## Flags

| Flag | Default | Description |
| --- | --- | --- |
| `--top N` | 20 | Maximum entries per hotspot table (`0` = all). |
| `--strip-prefix PATH` | *(empty)* | Prefix removed from file paths. |
| `--no-suppress` | *(off)* | Include functions silenced by in-source suppression markers (raw audit view). |
| `--vcs` | *(off)* | Append a "Change-history risk" section ranking files by VCS risk (default windows), mirroring `bca metrics --vcs`. The section ranks the same files-with-metrics as the AST hotspot tables (the `metrics` file-type scope, #576), so both halves describe one file universe. Ignored with a warning outside a git working tree. See [`bca vcs`](vcs.md). |
| `-o, --output FILE` | *(stdout)* | Output file. Parent directory must exist. |

## Suppression markers

By default, `bca report markdown|html` **honours** in-source suppression
markers — the same `// bca: suppress`, `// bca: suppress-file`, and
`#lizard forgives` comments that [`bca check`](check.md) and the SARIF
emitter respect (see [Suppression](suppression.md)). A function is
omitted from a metric's hotspot table when that metric is suppressed for
it, so the published report agrees with the threshold gate instead of
re-surfacing every silenced offender.

Suppression is per-metric: a `// bca: suppress(cyclomatic)` marker drops
the function from the Cyclomatic table only — it still appears in the
Cognitive, Halstead, and other tables. A bare `// bca: suppress` (or
`// bca: suppress-file`) covers every metric.

Pass `--no-suppress` for the raw audit view that lists every offender
regardless of markers. The setting can also be pinned in the
[`bca.toml` manifest](check.md):

```toml
[report]
no_suppress = true
```

The CLI flag wins; a bare `--no-suppress` can force the audit view on,
but the manifest never forces it off.

## Examples

Show only the five worst hotspots per section:

```bash
bca report -p src/ markdown --top 5
```

Strip the workspace root from displayed paths:

```bash
bca report -p /home/user/project markdown \
    --strip-prefix /home/user/project/
```

The user's daily-driver invocation:

```bash
bca report \
    --paths "$PWD" \
    markdown \
    --top 20 \
    --strip-prefix "$PWD/"
```

## Report structure

A generated report contains the following sections (each section is
omitted when no data exists for it). Every hotspot table includes a
`Tokens` column (Lizard-style leaf-token count, comments excluded)
alongside `SLOC` so two complementary size proxies are visible per row.

1. **Project summary** — files analyzed, languages, total SLOC / PLOC /
   comment counts, function and class counts, comment ratio.
2. **Per-language overview table** — one row per language with file
   count, SLOC, function count, the SLOC-weighted average
   Maintainability Index (MI), average Cyclomatic Complexity (CC), and
   average Cognitive Complexity. The MI average is size-weighted and
   uses the *unclamped* Visual Studio value, so a language whose files
   are mostly unmaintainable reads negative instead of saturating at the
   0 floor of the per-file `MI` column.
3. **Per-language hotspot sections** (repeated for each language). Every
   hotspot title follows one template, `<Concept> hotspots (top N by
   <column>)` — the truncation clause states the actual `--top` state
   (`top 20 by CC`, or `all, by CC` for `--top 0`):
   - *Summary* — file count, SLOC, PLOC, comment ratio, and
     `Average MI (SLOC-weighted)` with a GOOD / MODERATE / LOW rating.
     The headline is the SLOC-weighted mean of the *unclamped* Visual
     Studio MI: large files dominate, and a file whose displayed MI
     clamps to 0 still contributes its true (often negative)
     maintainability rather than a misleading 0.
   - *Actionable Summary* — counts of functions exceeding common
     thresholds (by default CC > 10, cognitive > 15, SLOC > 100,
     args > 3, Halstead bugs > 1; a manifest `[thresholds]` table
     overrides each cutoff). Emitted **first**, directly after the Summary
     and before any hotspot table, so a reader who stops after a table
     or two still sees the highest-altitude counts. These are **raw**
     counts that ignore suppression; the section is captioned to say
     so, naming how many suppressed functions are folded in. When
     suppression empties a hotspot table whose metric this summary
     still counts, the table is replaced by a one-line "table omitted:
     all N matching functions suppressed" note so a summary bullet
     never points at a missing table.
   - *Maintainability Index hotspots (lowest N by MI)* — files sorted
     ascending by MI.
   - *Cyclomatic complexity hotspots (top N by CC)* — functions sorted
     descending by CC, with summary statistics (average, max, counts
     above 10 and 20).
   - *Cognitive complexity hotspots (top N by Cognitive)* — functions
     sorted descending by cognitive complexity.
   - *Halstead effort hotspots (top N by Effort)* — functions sorted
     descending by Halstead effort, including volume and estimated
     bugs. Effort and Volume render as rounded integers with thousands
     separators (`8,845`); full precision lives in JSON/CSV.
   - *Function size hotspots (top N by SLOC)* — functions sorted
     descending by source lines of code.
   - *Many parameters hotspots (top N by Args)* — functions with more
     than three parameters, sorted descending.
   - *Type hotspots (top N by WMC)* — types sorted descending by
     Weighted Methods per Class, with NOM, NPA, and NPM. "Type" covers
     all six kinds the report counts: class, struct, trait, impl,
     interface, namespace (the legend's WMC entry lists them).
   - *Exit points hotspots (top N by Exits)* — functions with more than
     two exit points, sorted descending. A single `return` is the
     baseline, not a hotspot, so the table admits only `nexits > 2`;
     when nothing clears the floor the section is omitted.
   - *ABC magnitude hotspots (top N by ABC)* — functions sorted
     descending by ABC metric magnitude.

## Format consistency

The Markdown and HTML reports are two renderings of one underlying data
model — they always present the **same data**. Every shared figure
(project and per-language summaries, hotspot table membership, and each
hotspot caption such as the cyclomatic Average / Max / CC > 10 note) is
computed once and rendered by both, so a single run produces identical
numbers whether you emit `--format markdown` or `--format html`.

Both formats also carry a **Legend** that defines every metric column
abbreviation (`CC`, `MI`, `ABC`, `WMC`, …) plus the global-header stats
(`PLOC`, `Comments`, `Comment ratio`) — a `## Legend` section in Markdown
(its own outline entry, not nested under the last language) and an
expanded (`<details open>`) block in HTML, so it survives print, mobile,
and screen readers as well as a hover tooltip. Each entry links to its
chapter in the [Supported Metrics](../metrics.md) reference, so a one-line
definition can hand the reader the full explanation. The definitions come
from the same shared column specs the tooltips use, so the two formats
cannot drift.

Both formats also close with a **provenance footer** stating the `bca`
version, generation date, the seed paths scanned, the per-table `--top`
value, and whether suppression markers were honored — so a detached
artifact (a PR comment, a Pages deployment, a file on a ticket) records
what it was generated from. The date honors `SOURCE_DATE_EPOCH` for
reproducible builds. The HTML report additionally carries a
`<meta name="viewport">` tag and wraps every table in a horizontal-scroll
container so wide tables stay reachable on mobile and narrow windows, and
its table of contents nests each language's hotspot subsections under a
collapsible entry.

Suppression is applied uniformly across **every** output, not just the
reports. A function silenced for a metric — via an in-source marker or
the baseline — is dropped from `bca check`'s offender formats
(`code-climate`, `sarif`, `checkstyle`, `clang-warning`, `msvc-warning`)
and from the matching report hotspot table alike. The CodeClimate, SARIF, and
Checkstyle documents are themselves three renderings of one offender
set, so they agree by construction; the reports honour the same
per-metric suppression decisions.

The single deliberate exception is the **Actionable Summary**, a
whole-codebase health indicator that intentionally counts raw
measurements regardless of suppression — silencing a function in one
metric's hotspot table does not erase it from that aggregate concern
count. Every other figure, including each hotspot table's caption,
reflects the suppression-filtered set. To stop a reader mistaking the
two populations for a double-count, each is captioned: the cyclomatic
note adds "(excluding suppressed functions)", and the Actionable
Summary names the raw, suppression-ignoring basis of its counts.

## HTML format {#html-format}

`bca report html` emits a single self-contained HTML page covering the
same sections as the Markdown report. It is designed to be served as a
static artifact: inline CSS, inline vanilla JavaScript for click-to-sort
on every hotspot table, and zero external dependencies (no CDN, no
fonts, no template engine). The page renders identically offline.

Write it to a file and open in any browser:

```bash
bca report --paths /path/to/project \
    html --top 10 --output report.html
```

Click any column header to sort that table ascending, click again to
toggle descending. Each table sorts independently. Empty cells (where a
metric was not measured) sort as if they were positive infinity, which
keeps "no data" rows out of the visible top of a hotspot.

Hover (or keyboard-focus, where the browser supports it) any metric
column header — `SLOC`, `MI`, `CC`, `ABC`, `WMC`, `NPA`, `NPM`,
`Exits`, etc. — for a one-sentence plain-English explanation of the
metric. The tooltip is delivered through the native HTML `title`
attribute, so it works offline with no JavaScript.

Because `title` tooltips are hover-only — invisible in print, on
mobile, and to screen readers — the page also ends with a visible,
collapsible **Legend** (`<details>`) listing every metric column's
one-line definition. Both the tooltips and the legend draw from the
same column specs, so a definition cannot say one thing on hover and
another in the legend.

Every interpolated string — function name, file path, language label —
is HTML-escaped on the way out, so a crafted source path or symbol name
cannot inject markup or break out of an attribute value.

Each per-language `<section>` carries a stable `lang-<name>` class
(e.g. `lang-rust`, `lang-python`) styled with a low-alpha background
tint and matching left border so a multi-language report's section
boundaries are obvious at a glance. Languages without an explicit
palette entry fall back to a neutral `lang-other` tint, and a
`prefers-color-scheme: dark` adapter raises the alpha so contrast
holds in both themes.

## Metric values of zero

A metric value of **0** in the report means the metric was not measured
for that item (e.g. Halstead metrics on an empty function). Sections
whose entries are all zero are omitted entirely.
