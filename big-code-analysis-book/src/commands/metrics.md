# Metrics

Metrics can be displayed or exported in various formats using **big-code-analysis-cli**.

## Display Metrics

To compute and display metrics for a given file or directory, run:

```bash
big-code-analysis-cli -m -p /path/to/your/file/or/directory
```

- `-p`: Path to the file or directory to analyze. If a directory is provided, metrics will be computed for all supported files it contains.

## Exporting Metrics

**big-code-analysis-cli** supports multiple output formats for exporting metrics, including:

- CBOR
- JSON
- TOML
- YAML

Both `JSON` and `TOML` can be exported as pretty-printed.

### Export Command

To export metrics as a JSON file:

```bash
big-code-analysis-cli -m -p /path/to/your/file/or/directory -O json -o /path/to/output/directory
```

- `-O`: Specifies the output format (e.g., json, toml, yaml, cbor).
- `-o`: Path to save the output file. The filename of the output file is the same as the input file plus the extension associated to the format. If not specified, the result will be printed in the shell. 

### Pretty Print

To output pretty-printed JSON metrics:

```bash
big-code-analysis-cli -m -p /path/to/your/file/or/directory --pr -O json
```

This command prints the formatted metrics to the console or the specified output path.

### Markdown Report

For a comprehensive, human-readable quality report use `-O markdown`.
This format aggregates metrics across all analyzed files and produces
per-language hotspot tables.  See the [Markdown Report](markdown.md) chapter
for details.

## Listing Available Metrics

Tooling that drives **big-code-analysis-cli** can discover the metric
catalog at runtime instead of hard-coding it:

```bash
big-code-analysis-cli --list-metrics
```

prints metric names one per line. Pass `descriptions` for a one-line
summary of each metric:

```bash
big-code-analysis-cli --list-metrics descriptions
```
