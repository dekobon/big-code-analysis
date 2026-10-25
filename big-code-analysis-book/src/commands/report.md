# Report

`bca report [--format <FORMAT>]` produces an aggregated quality-metrics
report across every file walked, for pasting into pull requests, wikis,
or issue trackers. For per-file data meant for tooling, use
[`bca metrics`](metrics.md); for a pass/fail gate, use
[`bca check`](check.md).

Two formats are available: `markdown` (plain text, suited to PR
comments) and `html` (a self-contained dashboard with sortable tables,
suited to a build artifact). Pick one with `--format` / `-O`
(`bca report --format html`); the default is `markdown`. The bare
positional form (`bca report markdown`) still works as a deprecated
alias and is slated for removal in the next major; prefer `--format`.

> **CI integration.** For runnable
> [GitHub Actions](https://docs.github.com/en/actions) and
> [GitLab CI](https://docs.gitlab.com/ci/) recipes that post the
> Markdown report as a PR/MR comment, see the
> [CI integration recipe](../recipes/ci.md).
>
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
markers: the same `// bca: suppress`, `// bca: suppress-file`, and
[Lizard](https://github.com/terryyin/lizard) `#lizard forgives` comments
that [`bca check`](check.md) and the
[SARIF](https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html)
emitter respect (see
[Suppression](suppression.md)). A function is omitted from a metric's
hotspot table when that metric is suppressed for it, so the published
report agrees with the threshold gate instead of re-surfacing every
silenced offender.

Suppression is per-metric: a `// bca: suppress(cyclomatic)` marker drops
the function from the Cyclomatic table only; it still appears in the
Cognitive, Halstead, and other tables. A bare `// bca: suppress` (or
`// bca: suppress-file`) covers every metric.

Pass `--no-suppress` for the raw audit view that lists every offender
regardless of markers. The setting can also be pinned in the
[`bca.toml` manifest](check.md):

```toml
[report]
no_suppress = true
```

The CLI flag wins in either direction: a bare `--no-suppress` forces
the audit view on, and `--no-suppress=false` restores the
marker-honoring default even when the manifest enabled the audit
view. The manifest only fills the value when the CLI left it unset.

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

A typical day-to-day invocation:

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

1. **Project summary**: files analyzed, languages, total SLOC / PLOC /
   comment counts, function and class counts, comment ratio.
2. **Per-language overview table**: one row per language with file
   count, SLOC, function count, the SLOC-weighted average
   [Maintainability Index](../metrics.md#maintainability-index-mi) (MI),
   average [Cyclomatic Complexity](../metrics.md#cyclomatic-complexity-cc)
   (CC), and average
   [Cognitive Complexity](../metrics.md#cognitive-complexity). The MI
   average is size-weighted and uses the *unclamped* Visual Studio
   value, so a language whose files
   are mostly unmaintainable reads negative instead of saturating at the
   0 floor of the per-file `MI` column.
3. **Per-language hotspot sections** (repeated for each language). Every
   hotspot title follows one template, `<Concept> hotspots (top N by
   <column>)`, where the truncation clause states the actual `--top`
   state (`top 20 by CC`, or `all, by CC` for `--top 0`):
   - *Summary*: file count, SLOC, PLOC, comment ratio, and
     `Average MI (SLOC-weighted)` with a GOOD / MODERATE / LOW rating.
     The headline is the SLOC-weighted mean of the *unclamped* Visual
     Studio MI: large files dominate, and a file whose displayed MI
     clamps to 0 still contributes its true (often negative)
     maintainability rather than a misleading 0. A file with no code to
     measure (an `#include`-only header, an empty module) is left out of
     every MI average and of the MI hotspot table, because its MI of 0.0
     is a placeholder, not a score; it still counts toward Files and
     SLOC. A language with no file to measure reads `n/a`.
   - *Actionable Summary*: counts of functions exceeding common
     thresholds (by default CC > 10, cognitive > 15, SLOC > 100,
     args > 3, Halstead bugs > 1; a manifest `[thresholds]` table
     overrides each cutoff). The Actionable Summary appears immediately
     after the Summary and before the hotspot tables. It counts raw
     measurements without applying suppression; the section is
     captioned to say so, naming how many suppressed functions are
     folded in. When
     suppression empties a hotspot table whose metric this summary
     still counts, the table is replaced by a one-line "table omitted:
     all N matching functions suppressed" note so a summary bullet
     never points at a missing table.
   - *Maintainability Index hotspots (lowest N by MI)*: files sorted
     ascending by MI.
   - *Cyclomatic complexity hotspots (top N by CC)*: functions sorted
     descending by CC, with summary statistics (average, max, counts
     above 10 and 20).
   - *Cognitive complexity hotspots (top N by Cognitive)*: functions
     sorted descending by cognitive complexity.
   - *Halstead effort hotspots (top N by Effort)*: functions sorted
     descending by Halstead effort, including volume and estimated
     bugs. Effort and Volume render as rounded integers with thousands
     separators (`8,845`); full precision lives in JSON/CSV.
   - *Function size hotspots (top N by SLOC)*: functions sorted
     descending by source lines of code.
   - *Many parameters hotspots (top N by Args)*: functions with more
     than three parameters, sorted descending.
   - *Type hotspots (top N by WMC)*: types sorted descending by
     Weighted Methods per Class, with NOM, NPA, and NPM. "Type" covers
     all six kinds the report counts: class, struct, trait, impl,
     interface, namespace (the legend's WMC entry lists them).
   - *Exit points hotspots (top N by Exits)*: functions with more than
     two exit points, sorted descending. A single `return` is the
     baseline, not a hotspot, so the table admits only `nexits > 2`;
     when nothing clears the floor the section is omitted.
   - *ABC magnitude hotspots (top N by ABC)*: functions sorted
     descending by ABC metric magnitude.

## Format consistency

The Markdown and HTML renderers share one data model. They use the same
summaries, hotspot membership, and captions, so both formats report
identical numbers.

Both formats carry a **Legend** that defines every metric column
abbreviation (`CC`, `MI`, `ABC`, `WMC`, and so on) plus the
global-header stats (`PLOC`, `Comments`, `Comment ratio`). It is a
`## Legend` section in Markdown (its own outline entry, not nested under
the last language) and an expanded (`<details open>`) block in HTML, so
it survives print, mobile, and screen readers as well as a hover
tooltip. Each entry links to its chapter in the
[Supported Metrics](../metrics.md) reference, so a one-line definition
can hand the reader the full explanation. The definitions come from the
same shared column specs the tooltips use, so the two formats cannot
drift.

Both formats close with a **provenance footer** stating the `bca`
version, generation date, the seed paths scanned, the per-table `--top`
value, and whether suppression markers were honored, so a detached
artifact (a PR comment, a Pages deployment, a file on a ticket) records
what it was generated from. The date honors
[`SOURCE_DATE_EPOCH`](https://reproducible-builds.org/docs/source-date-epoch/)
for reproducible builds. The HTML report additionally carries a
`<meta name="viewport">` tag and wraps every table in a horizontal-scroll
container so wide tables stay reachable on mobile and narrow windows, and
its table of contents nests each language's hotspot subsections under a
collapsible entry.

Source suppression markers apply per metric to both report hotspot
tables and `bca check`'s offender formats (`code-climate`, `sarif`,
`checkstyle`, `clang-warning`, `msvc-warning`). Check baselines
additionally filter check violations; they do not filter report hotspot
tables. The
[Code Climate](https://github.com/codeclimate/platform/blob/master/spec/analyzers/SPEC.md),
SARIF, and [Checkstyle](https://checkstyle.org/) documents are
themselves three renderings of one offender set, so they agree by
construction; the reports honour the same per-metric suppression
decisions.

The single deliberate exception is the **Actionable Summary**, a
whole-codebase health indicator that counts raw measurements regardless
of suppression: silencing a function in one metric's hotspot table does
not erase it from that aggregate concern count. Every other figure,
including each hotspot table's caption, reflects the
suppression-filtered set. To stop a reader mistaking the two populations
for a double-count, each is captioned: the cyclomatic note adds
"(excluding suppressed functions)", and the Actionable Summary names
the raw, suppression-ignoring basis of its counts.

## HTML format {#html-format}

`bca report html` emits a single self-contained HTML page covering the
same sections as the Markdown report. It is built to be served as a
static artifact: inline CSS, inline vanilla JavaScript for click-to-sort
on every hotspot table, and no external dependencies (no CDN, no
fonts, no template engine). The page renders identically offline.

Write it to a file and open it in any browser:

```bash
bca report --paths /path/to/project \
    html --top 10 --output report.html
```

Click any column header to sort that table ascending, click again to
toggle descending. Each table sorts independently. Empty cells (where a
metric was not measured) sort as if they were positive infinity, which
keeps "no data" rows out of the visible top of a hotspot.

Hover (or keyboard-focus, where the browser supports it) any metric
column header (`SLOC`, `MI`, `CC`, `ABC`, `WMC`, `NPA`, `NPM`,
`Exits`, and the rest) for a one-sentence plain-English explanation of
the metric. The tooltip is delivered through the native HTML `title`
attribute, so it works offline with no JavaScript.

Because `title` tooltips are hover-only (invisible in print, on
mobile, and to screen readers), the page also ends with a visible,
collapsible **Legend** (`<details>`) listing every metric column's
one-line definition. Both the tooltips and the legend draw from the
same column specs, so a definition cannot say one thing on hover and
another in the legend.

Every interpolated string (function name, file path, language label)
is HTML-escaped on the way out, so a crafted source path or symbol name
cannot inject markup or break out of an attribute value.

Each per-language `<section>` carries a stable `lang-<name>` class
(`lang-rust`, `lang-python`) styled with a low-alpha background tint
and matching left border so a multi-language report's section
boundaries are obvious at a glance. Languages without an explicit
palette entry fall back to a neutral `lang-other` tint, and a
`prefers-color-scheme: dark` adapter raises the alpha so contrast
holds in both themes.

## Metric values of zero

Zero can be a valid measurement or a placeholder for unavailable data,
depending on the metric. Each hotspot table applies its own eligibility
rule; a measured MI of zero remains eligible. A table with no eligible
entries is omitted.
