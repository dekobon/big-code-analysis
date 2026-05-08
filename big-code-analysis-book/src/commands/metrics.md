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

It also supports one aggregated CI/IDE format that combines findings
from every analyzed file into a single document:

- Checkstyle (Checkstyle 4.3 XML, the lingua franca for Jenkins,
  SonarQube, GitLab, and most "warnings plugin" CI integrations)

### Export command

To export metrics as JSON files:

```bash
big-code-analysis-cli --paths /path/to/your/file/or/directory metrics \
    -O json -o /path/to/output/directory
```

- `-O, --output-format`: per-file output format (`cbor`, `json`,
  `toml`, `yaml`) or aggregated CI format (`checkstyle`).
- `-o, --output`: directory to save output files for per-file formats.
  Filenames mirror the input file plus the format extension. If
  omitted, results are printed to stdout. CBOR is binary and therefore
  requires `-o`. For aggregated formats (`checkstyle`), `--output`
  names a single output **file** (extension `.checkstyle.xml`) rather
  than a directory.

### Checkstyle (CI integration)

```bash
big-code-analysis-cli --paths /path/to/your/code metrics \
    -O checkstyle -o report.checkstyle.xml
```

The Checkstyle writer emits a single `<checkstyle version="4.3">`
document containing one `<file>` element per source path, each
holding one `<error>` per metric-threshold violation. The threshold
engine that produces these violation records is tracked under
[issue #96](https://github.com/dekobon/big-code-analysis/issues/96).
Until that lands the writer emits a well-formed but empty
`<checkstyle version="4.3"/>` document, so CI pipelines can already
wire up the consumer without waiting on the producer.

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
