# Report

`bca report [--format <FORMAT>]` produces an aggregated quality-metrics
report across every file walked. It is designed for pasting into pull
requests, wikis, or issue trackers.

Pick the format with `--format` / `-O` (`bca report --format html`).
When omitted, the report defaults to `markdown`. The bare positional
form (`bca report markdown`) still works as a deprecated alias and will
be removed in 2.0; prefer `--format`.

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
bca --paths /path/to/project report markdown
```

Write to a file:

```bash
bca --paths /path/to/project report markdown --output report.md
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
bca -p src/ report markdown --top 5
```

Strip the workspace root from displayed paths:

```bash
bca -p /home/user/project report markdown \
    --strip-prefix /home/user/project/
```

The user's daily-driver invocation:

```bash
bca \
    --paths "$PWD" \
    report markdown \
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
   count, SLOC, function count, average Maintainability Index (MI),
   average Cyclomatic Complexity (CC), and average Cognitive
   Complexity.
3. **Per-language hotspot sections** (repeated for each language):
   - *Summary* — file count, SLOC, PLOC, comment ratio, average MI
     with a GOOD / MODERATE / LOW rating.
   - *Maintainability Index (lowest files)* — files sorted ascending
     by MI.
   - *Cyclomatic Complexity Hotspots* — functions sorted descending
     by CC, with summary statistics (average, max, counts above 10 and
     20).
   - *Cognitive Complexity Hotspots* — functions sorted descending by
     cognitive complexity.
   - *Halstead Effort Hotspots* — functions sorted descending by
     Halstead effort, including volume and estimated bugs.
   - *Largest Functions by SLOC* — functions sorted descending by
     source lines of code.
   - *Functions With Many Parameters (>3)* — functions with more than
     three parameters, sorted descending.
   - *Actionable Summary* — counts of functions exceeding common
     thresholds (CC > 10, cognitive > 15, SLOC > 100, args > 3,
     Halstead bugs > 1). These are **raw** counts that ignore
     suppression; the section is captioned to say so, naming how many
     suppressed functions are folded in. When suppression empties a
     hotspot table whose metric this summary still counts, the table is
     replaced by a one-line "table omitted: all N matching functions
     suppressed" note so a summary bullet never points at a missing
     table.
   - *Class/Trait/Impl Hotspots (WMC)* — classes sorted descending by
     Weighted Methods per Class, with NOM, NPA, and NPM.
   - *Functions with the most exit points (NEXITS)* — sorted
     descending by exit count.
   - *ABC Magnitude Hotspots* — functions sorted descending by ABC
     metric magnitude.

## Format consistency

The Markdown and HTML reports are two renderings of one underlying data
model — they always present the **same data**. Every shared figure
(project and per-language summaries, hotspot table membership, and each
hotspot caption such as the cyclomatic Average / Max / CC > 10 note) is
computed once and rendered by both, so a single run produces identical
numbers whether you emit `--format markdown` or `--format html`.

Suppression is applied uniformly across **every** output, not just the
reports. A function silenced for a metric — via an in-source marker or
the baseline — is dropped from `bca check`'s offender formats
(`code-climate`, `sarif`, `checkstyle`, `tty`, `json`) and from the
matching report hotspot table alike. The CodeClimate, SARIF, and
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

## HTML format

`bca report html` emits a single self-contained HTML page covering the
same sections as the Markdown report. It is designed to be served as a
static artifact: inline CSS, inline vanilla JavaScript for click-to-sort
on every hotspot table, and zero external dependencies (no CDN, no
fonts, no template engine). The page renders identically offline.

Write it to a file and open in any browser:

```bash
bca --paths /path/to/project \
    report html --top 10 --output report.html
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
