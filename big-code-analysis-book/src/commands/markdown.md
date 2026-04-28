# Markdown Report

The `-O markdown` output format produces a human-readable quality-metrics
report in Markdown.  It is designed for pasting into pull requests, wikis, or
issue trackers.

## Quick start

Print to stdout:

```bash
big-code-analysis-cli --metrics --paths /path/to/project -O markdown
```

Write to a file:

```bash
big-code-analysis-cli --metrics --paths /path/to/project -O markdown --output report.md
```

> **Note:** `--output` must be a *file* path, not a directory.

## Flags

| Flag | Default | Description |
| --- | --- | --- |
| `--top N` | 20 | Maximum number of entries in each hotspot table. |
| `--strip-prefix PATH` | *(empty)* | Prefix removed from file paths in the report (useful for shortening absolute paths). |

### Examples

Show only the five worst hotspots per section:

```bash
big-code-analysis-cli -m -p src/ -O markdown --top 5
```

Strip the workspace root from displayed paths:

```bash
big-code-analysis-cli -m -p /home/user/project -O markdown \
    --strip-prefix /home/user/project/
```

## Report structure

A generated report contains the following sections (each section is omitted
when no data exists for it):

1. **Project summary** -- files analyzed, languages, total SLOC / PLOC /
   comment counts, function and class counts, comment ratio.
2. **Per-language overview table** -- one row per language with file count,
   SLOC, function count, average Maintainability Index (MI), average
   Cyclomatic Complexity (CC), and average Cognitive Complexity.
3. **Per-language hotspot sections** (repeated for each language):
   - *Summary* -- file count, SLOC, PLOC, comment ratio, average MI with
     a GOOD / MODERATE / LOW rating.
   - *Maintainability Index (lowest files)* -- files sorted ascending by MI.
   - *Cyclomatic Complexity Hotspots* -- functions sorted descending by CC,
     with summary statistics (average, max, counts above 10 and 20).
   - *Cognitive Complexity Hotspots* -- functions sorted descending by
     cognitive complexity.
   - *Halstead Effort Hotspots* -- functions sorted descending by Halstead
     effort, including volume and estimated bugs.
   - *Largest Functions by SLOC* -- functions sorted descending by source
     lines of code.
   - *Functions With Many Parameters (>3)* -- functions with more than three
     parameters, sorted descending.
   - *Actionable Summary* -- counts of functions exceeding common thresholds
     (CC > 10, cognitive > 15, SLOC > 100, args > 3, Halstead bugs > 1).
   - *Class/Trait/Impl Hotspots (WMC)* -- classes sorted descending by
     Weighted Methods per Class, with NOM, NPA, and NPM.
   - *Functions with the most exit points (NEXITS)* -- sorted descending by
     exit count.
   - *ABC Magnitude Hotspots* -- functions sorted descending by ABC metric
     magnitude.

## Requirements and restrictions

`-O markdown` **requires** `--metrics` (`-m`).

It is **incompatible** with the following flags (the CLI will exit with an
error if any are combined):

- `--ops`
- `--dump`
- `--comments`
- `--function`
- `--find`
- `--count`

## Metric values of zero

A metric value of **0** in the report means the metric was not measured for
that item (e.g. Halstead metrics on an empty function).  Sections whose
entries are all zero are omitted entirely.
