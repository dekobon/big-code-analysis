# Report

`bca report <FORMAT>` produces an aggregated quality-metrics report
across every file walked. It is designed for pasting into pull
requests, wikis, or issue trackers.

The only format available today is `markdown`. `html` is reserved for a
future implementation.

> **Migrating?** This command replaces the pre-restructure `--metrics
> -O markdown` invocation. See the [migration guide](../migration.md).

## Quick start

Print to stdout:

```bash
big-code-analysis-cli --paths /path/to/project report markdown
```

Write to a file:

```bash
big-code-analysis-cli --paths /path/to/project report markdown --output report.md
```

> **Note:** `--output` must be a *file* path, not a directory.

## Flags

| Flag | Default | Description |
| --- | --- | --- |
| `--top N` | 20 | Maximum entries per hotspot table. |
| `--strip-prefix PATH` | *(empty)* | Prefix removed from file paths. |
| `-o, --output FILE` | *(stdout)* | Output file. Parent directory must exist. |

## Examples

Show only the five worst hotspots per section:

```bash
big-code-analysis-cli -p src/ report markdown --top 5
```

Strip the workspace root from displayed paths:

```bash
big-code-analysis-cli -p /home/user/project report markdown \
    --strip-prefix /home/user/project/
```

The user's daily-driver invocation:

```bash
big-code-analysis-cli \
    --paths "$PWD" \
    --num-jobs $(nproc) \
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
     Halstead bugs > 1).
   - *Class/Trait/Impl Hotspots (WMC)* — classes sorted descending by
     Weighted Methods per Class, with NOM, NPA, and NPM.
   - *Functions with the most exit points (NEXITS)* — sorted
     descending by exit count.
   - *ABC Magnitude Hotspots* — functions sorted descending by ABC
     metric magnitude.

## Metric values of zero

A metric value of **0** in the report means the metric was not measured
for that item (e.g. Halstead metrics on an empty function). Sections
whose entries are all zero are omitted entirely.
