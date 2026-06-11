# Metrics

`bca metrics` computes per-file metrics and emits them either to stdout
or to a directory of structured files.

> **Migrating?** This command replaces the pre-restructure `--metrics`
> flag. The aggregated report previously selected with `-O markdown`
> now lives under [`bca report`](report.md), and the CI/IDE offender
> formats (Checkstyle, SARIF, code-climate, clang-warning,
> msvc-warning) moved to
> [`bca check --format <fmt>`](check.md). See the
> [migration guide](../migration.md).

## Display metrics

To compute and display metrics for a given file or directory, run:

```bash
bca --paths /path/to/your/file/or/directory metrics
```

- `--paths` (or `-p`): file or directory to analyze. If a directory is
  provided, metrics are computed for every supported file it contains.

## Exporting metrics

`bca metrics` supports five per-file output formats:

- CBOR
- CSV
- JSON
- TOML
- YAML

Both JSON and TOML can be exported as pretty-printed.

The three top-level output kinds map to three separate commands so
each one stays consistent with its data model:

| Command                            | Output                          | Audience          |
| ---------------------------------- | ------------------------------- | ----------------- |
| `bca metrics`                      | Per-file metric trees           | Downstream tooling |
| [`bca report`](report.md)          | Aggregated quality dashboards   | Humans / PRs      |
| [`bca check`](check.md)            | Threshold-violation reports     | CI / IDE          |

The CI/IDE offender formats (Checkstyle, SARIF, code-climate,
clang-warning, msvc-warning) used to live on `bca metrics -O <fmt>`.
They moved to
`bca check --format <fmt>` because their input is a
list of threshold violations, not the per-file metric tree that the
other formats above carry. See the
[`bca check` chapter](check.md#exporting-offender-records) for the
new invocation.

### Export command

To export metrics as JSON files:

```bash
bca --paths /path/to/your/file/or/directory metrics \
    -O json -o /path/to/output/directory
```

- `-O, --format`: output format. Defaults to `text` — a human-readable
  colored metric tree printed to stdout; pass `--format text` to request
  that default explicitly (for example to override a `bca.toml` that set
  a structured format). The structured per-file serializers are `cbor`,
  `csv`, `json`, `toml`, and `yaml`. `--output-format` is accepted as a
  deprecated alias and will be removed in 2.0.
- `-o, --output`: directory to save output files. Filenames mirror
  the input file plus the format extension. If omitted, results are
  printed to stdout. CBOR is binary and therefore requires `-o`.
  Passing `--output` without a structured `--format` is an error (the
  default `text` format streams to stdout and writes no files), so an
  explicit `--output` never silently no-ops (#661).
- `--metrics <name,…>`: restrict computation to a subset of metrics
  (comma-separated and/or repeated, e.g. `--metrics cyclomatic,cognitive
  --metrics loc`). Names are the canonical ids `bca list-metrics` prints
  — the same vocabulary `bca check --threshold` and `bca diff --metric`
  use; dotted (`cyclomatic.modified`) and bare `loc` sub-metric (`sloc`)
  spellings are accepted. Derived metrics pull in their dependencies
  automatically. An unknown name errors with a "did you mean" hint.
  Omit it to compute every metric (#691).

### CSV (spreadsheets and Pandas)

```bash
bca --paths /path/to/your/code metrics \
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
bca --paths /path/to/your/code metrics -O csv \
    > metrics.csv
```

CSV is a per-file format; with `--output <dir>` each input file
produces a `<input>.csv` mirror under the output directory.

> An aggregated HTML *report* covering the whole walk is available
> via [`bca report html`](report.md#html-format). The previous
> per-file `bca metrics -O html` writer was removed because it
> degraded to an unopenable single-file table on real-world repos —
> CSV is the right shape for flat per-`FuncSpace` rows.

### Pretty print

```bash
bca --paths /path/to/your/file/or/directory metrics \
    --pretty -O json
```

## Excluding inline test code

```bash
bca --paths /path/to/your/code --exclude-tests metrics
```

By default, every node in the AST is counted, including inline test
items. Rust files following the idiomatic
`#[cfg(test)] mod tests { ... }` layout therefore have headline
metrics that mix production and test code together.

Pass `--exclude-tests` to elide test-only subtrees before any metric
is computed. The flag is recognised by every subcommand that walks
the AST (`metrics`, `report`, `check`), and currently understands the
following Rust attribute shapes:

- `#[test]` and `#[rstest]` / `#[test_case]` / `#[wasm_bindgen_test]`
- `#[cfg(test)]`, `#[cfg(all(test, ...))]`, `#[cfg(any(test, ...))]`
- `#[tokio::test]`, `#[async_std::test]`, `#[test_log::test]`, …
  (any path ending in `::test`)
- `#![cfg(test)]` on `mod` items (inner attribute form)

Languages without a `Checker::should_skip_subtree` override simply
ignore the flag — only Rust applies the pruning today. The default
remains off so existing metric numbers stay byte-identical for users
who do not opt in.

## Aggregated report

For a comprehensive, human-readable quality report, use
[`bca report markdown`](report.md). That command aggregates metrics
across all analyzed files and produces per-language hotspot tables.

## Listing available metrics

Tooling that drives the CLI can discover the metric catalog at runtime
instead of hard-coding it:

```bash
bca list-metrics
```

prints metric names one per line. Pass `descriptions` for a one-line
summary of each metric:

```bash
bca list-metrics descriptions
```
