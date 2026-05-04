# Metrics

`bca metrics` computes per-file metrics and emits them either to stdout
or to a directory of structured files.

> **Migrating?** This command replaces the pre-restructure `--metrics`
> flag. The aggregated report previously selected with `-O markdown`
> now lives under [`bca report`](report.md). See the
> [migration guide](../migration.md).

## Display metrics

To compute and display metrics for a given file or directory, run:

```bash
big-code-analysis-cli --paths /path/to/your/file/or/directory metrics
```

- `--paths` (or `-p`): file or directory to analyze. If a directory is
  provided, metrics are computed for every supported file it contains.

## Exporting metrics

`bca metrics` supports four per-file output formats:

- CBOR
- JSON
- TOML
- YAML

Both JSON and TOML can be exported as pretty-printed.

### Export command

To export metrics as JSON files:

```bash
big-code-analysis-cli --paths /path/to/your/file/or/directory metrics \
    -O json -o /path/to/output/directory
```

- `-O, --output-format`: per-file output format (`cbor`, `json`,
  `toml`, `yaml`).
- `-o, --output`: directory to save output files. The output filename
  mirrors the input file plus the format extension. If omitted,
  results are printed to stdout. CBOR is binary and therefore requires
  `-o`.

### Pretty print

```bash
big-code-analysis-cli --paths /path/to/your/file/or/directory metrics \
    --pretty -O json
```

## Aggregated report

For a comprehensive, human-readable quality report, use
[`bca report markdown`](report.md). That command aggregates metrics
across all analyzed files and produces per-language hotspot tables.

## Listing available metrics

Tooling that drives the CLI can discover the metric catalog at runtime
instead of hard-coding it:

```bash
big-code-analysis-cli list-metrics
```

prints metric names one per line. Pass `descriptions` for a one-line
summary of each metric:

```bash
big-code-analysis-cli list-metrics descriptions
```
