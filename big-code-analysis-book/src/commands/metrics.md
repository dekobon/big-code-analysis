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
- CSV
- JSON
- TOML
- YAML

Both JSON and TOML can be exported as pretty-printed.

It also supports two aggregated CI/IDE formats that combine findings
from every analyzed file into a single document:

- Checkstyle (Checkstyle 4.3 XML, the lingua franca for Jenkins,
  SonarQube, GitLab, and most "warnings plugin" CI integrations)
- SARIF (SARIF 2.1.0 JSON, the OASIS standard ingested natively by
  GitHub Code Scanning and most modern IDE/security tooling)
- Clang/GCC warning lines (one offender per line, recognized by
  editor quickfix parsers and GitHub Actions problem matchers)
- MSVC warning lines (Visual Studio's `cl.exe` diagnostic format,
  recognized by Visual Studio, VS Code, and Windows CI runners)

### Export command

To export metrics as JSON files:

```bash
big-code-analysis-cli --paths /path/to/your/file/or/directory metrics \
    -O json -o /path/to/output/directory
```

- `-O, --output-format`: per-file output format (`cbor`, `csv`,
  `json`, `toml`, `yaml`) or aggregated CI format (`checkstyle`,
  `sarif`, `clang-warning`, `msvc-warning`).
- `-o, --output`: directory to save output files for per-file formats.
  Filenames mirror the input file plus the format extension. If
  omitted, results are printed to stdout. CBOR is binary and therefore
  requires `-o`. For aggregated formats (`checkstyle`, `sarif`,
  `clang-warning`, `msvc-warning`), `--output` names a single output
  **file** (extension `.checkstyle.xml`, `.sarif.json`, or `.txt`)
  rather than a directory.

### CSV (spreadsheets and Pandas)

```bash
big-code-analysis-cli --paths /path/to/your/code metrics \
    -O csv -o csv-output
```

The CSV writer emits one row per `FuncSpace` (function, class,
struct, unit, etc.) with the entire metric matrix as columns. Header
order is fixed — see `CSV_HEADER` in
[`src/output/csv.rs`](https://github.com/dekobon/big-code-analysis/blob/main/src/output/csv.rs)
for the canonical list. Identity columns come first
(`path`, `space_name`, `space_kind`, `start_line`, `end_line`)
followed by every leaf metric using the same dotted JSON-style names
(`loc.lloc`, `halstead.volume`, `cyclomatic.modified.average`, etc.)
so a single column name addresses the metric in both CSV and JSON.

Empty cells (no value, not `0`) signal "not applicable for this
space" — for example, the OOP-only metrics (`wmc.*`, `npm.*`,
`npa.*`) appear empty for procedural code. RFC 4180 quoting is
delegated to the [`csv`] crate, so paths and names containing commas,
quotes, or newlines round-trip cleanly.

Stream the result to a single file with `-`:

```bash
big-code-analysis-cli --paths /path/to/your/code metrics -O csv \
    > metrics.csv
```

CSV is a per-file format; with `--output <dir>` each input file
produces a `<input>.csv` mirror under the output directory.

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

### SARIF (GitHub Code Scanning)

```bash
big-code-analysis-cli --paths /path/to/your/code metrics \
    -O sarif -o report.sarif.json
```

The SARIF writer emits a single SARIF 2.1.0 JSON document with one
`runs[]` element. Each metric-threshold violation becomes a `result`
under `runs[0].results[]`; the metric names appearing in the run are
deduplicated into `runs[0].tool.driver.rules[]` with short
descriptions. The threshold engine that produces these records is
tracked under
[issue #96](https://github.com/dekobon/big-code-analysis/issues/96).
Until that lands the writer emits a well-formed run with empty
`results` and `rules` arrays, so CI pipelines can already wire up the
consumer without waiting on the producer.

To upload a SARIF file to GitHub Code Scanning from a workflow:

```yaml
name: bca-sarif
on: [push, pull_request]
jobs:
  scan:
    runs-on: ubuntu-latest
    permissions:
      security-events: write
    steps:
      - uses: actions/checkout@v4
      - name: Run big-code-analysis
        run: |
          big-code-analysis-cli --paths . metrics \
              -O sarif -o report.sarif.json
      - name: Upload SARIF
        uses: github/codeql-action/upload-sarif@v3
        with:
          sarif_file: report.sarif.json
```

### Clang/GCC warning lines (editor quickfix and CI annotators)

```bash
big-code-analysis-cli --paths /path/to/your/code metrics \
    -O clang-warning -o report.txt
```

The Clang format emits one offender per line in the conventional
compiler-warning shape:

```text
path/to/file.rs:42:5: warning: cyclomatic 17 exceeds limit 15 [big-code-analysis-cyclomatic]
```

This is the format `clang -fdiagnostics-format=` produces and the
shape every editor quickfix parser (VS Code, IntelliJ, Vim) and most
CI annotators understand without configuration. The threshold engine
that produces these violation records is tracked under
[issue #96](https://github.com/dekobon/big-code-analysis/issues/96);
until it lands the writer emits an empty file (zero bytes), so CI
pipelines can already wire up the consumer.

GitHub Actions surfaces the lines as inline annotations on the PR
diff via the built-in GCC problem matcher (or any community
`compiler-problem-matchers` action):

```yaml
name: bca-clang-warnings
on: [push, pull_request]
jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Enable GCC problem matcher
        run: echo "::add-matcher::$RUNNER_TOOL_CACHE/problem-matchers/gcc.json"
      - name: Run big-code-analysis
        run: |
          big-code-analysis-cli --paths . metrics \
              -O clang-warning -o /dev/stdout
```

If your runner does not ship a GCC matcher, fall back to streaming
the lines and re-emitting them as `::warning file=...,line=...::`
workflow commands.

### MSVC warning lines (Visual Studio and Windows CI)

```bash
big-code-analysis-cli --paths /path/to/your/code metrics \
    -O msvc-warning -o report.txt
```

The MSVC format emits one offender per line in Visual Studio's
`cl.exe` diagnostic shape:

```text
path\to\file.rs(42,5): warning : cyclomatic 17 exceeds limit 15
```

Note the space before the colon after `warning`/`error` — that is
the MSVC convention. On Windows the path is normalized to use `\`
separators (matching cl.exe output); on other platforms the path is
emitted as-is. Visual Studio, VS Code with the C/C++ extension, and
Windows CI runners (Azure Pipelines, GitHub Actions on
`windows-latest`) parse these inline without extra configuration.

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
