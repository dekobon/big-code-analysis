# Check

`bca check` evaluates per-function metrics against thresholds and exits
non-zero when any function exceeds a limit. It is the CI integration
point: wire it into a build step and a regression in code complexity
fails the pipeline before the change lands.

## Exit codes

| Code | Meaning |
|------|---------|
| `0`  | All functions within thresholds (or `--no-fail` set). |
| `2`  | At least one threshold exceeded. |
| `1`  | Tool error (bad arguments, unreadable config, unknown metric). |

`1` is reserved so CI can distinguish a regression (`2`) from a tool
misconfiguration (`1`).

## Declaring thresholds

Pass `--threshold <metric>=<limit>` once per metric (repeatable). Metric
names match `bca list-metrics`; sub-metrics use a dotted form. `0` is a
valid limit and means "no value permitted".

```bash
bca --paths src/ check \
    --threshold cyclomatic=15 \
    --threshold cognitive=20 \
    --threshold loc.lloc=200
```

Or pull thresholds from a TOML config (one place to keep CI thresholds
versioned alongside the code):

```toml
# bca-thresholds.toml
[thresholds]
cyclomatic = 15
cognitive = 20
"loc.lloc" = 200
"halstead.volume" = 1000
```

```bash
bca --paths src/ check --config bca-thresholds.toml
```

CLI flags override values from `--config` for the same metric name, so
you can keep a project-wide default and tighten a single metric for a
specific run.

### Accepted metric names

Top-level scalar metrics use their `list-metrics` names directly:
`cognitive`, `cyclomatic`, `nargs`, `nexits`, `nom`, `tokens`, `abc`,
`wmc`, `npm`, `npa`. Metric suites with multiple sub-fields use a dotted
form:

| Metric              | Accepted threshold names |
|---------------------|---------------------------|
| Cyclomatic          | `cyclomatic`, `cyclomatic.modified` |
| Halstead            | `halstead.volume`, `halstead.difficulty`, `halstead.effort`, `halstead.time`, `halstead.bugs` |
| Lines of code       | `loc.sloc`, `loc.ploc`, `loc.lloc`, `loc.cloc`, `loc.blank` |
| Maintainability Index | `mi.original`, `mi.sei`, `mi.visual_studio` |

An unknown threshold name is a tool error (exit `1`), not silently
ignored.

## Offender output

Every offending `(function, metric)` pair prints one line to stderr in
this stable format:

```text
<path>:<start_line>-<end_line>: <function_name>: <metric> = <value> (limit <limit>)
```

For example:

```text
src/parser.rs:42-117: parse_expression: cyclomatic = 22 (limit 15)
src/parser.rs:42-117: parse_expression: cognitive = 31 (limit 20)
```

Lines are sorted by path, then start line, then metric name, so output
is deterministic across runs over the same tree.

## Silencing violations with suppression markers

In-source comments can silence threshold violations on individual
functions or whole files without editing the offending code or
excluding it from the walk. The native dialect is `bca: allow` /
`bca: allow-file`; Lizard's `#lizard forgives` is recognized as a
compatibility shim. See [Suppression markers](suppression.md) for
the full reference and the `--no-suppress` CI-audit flag.

## Reporting without failing

`--no-fail` (also written `--report-only` in some CI vocabularies)
prints offenders to stderr but exits `0`. Useful while adopting
baselines without flipping CI red.

```bash
bca --paths src/ check \
    --config bca-thresholds.toml --no-fail
```

## CI example (GitHub Actions)

```yaml
- name: Check code complexity thresholds
  run: |
    bca --paths src/ check --config bca-thresholds.toml
  # The default behavior — non-zero exit fails the step — is exactly
  # what we want here. No extra wiring needed.
```

If you want to keep the job green and surface offenders as a build
annotation while you reduce the count, swap in `--no-fail`:

```yaml
- name: Surface complexity hot spots (non-blocking)
  run: |
    bca --paths src/ check \
        --config bca-thresholds.toml --no-fail
```

## Exporting offender records

`bca check` also emits a single CI/IDE document covering every
offender in the walk. Pass `--output-format <fmt>` to pick the shape
and `--output <file>` to write it to disk (stdout if omitted). The
exit-code contract is unaffected by these flags: 0 clean, 2 on any
violation (unless `--no-fail`), 1 on tool error.

| Format          | Audience                                                |
| --------------- | ------------------------------------------------------- |
| `checkstyle`    | Jenkins, SonarQube, GitLab, "warnings plugin" CI        |
| `sarif`         | GitHub Code Scanning, modern IDEs / security tooling    |
| `clang-warning` | Editor quickfix parsers, GitHub Actions problem matcher |
| `msvc-warning`  | Visual Studio, VS Code, Windows CI runners              |

When no offenders exist the writer emits a well-formed but empty
document — empty `runs[].results` array for SARIF, no `<file>`
children under the `<checkstyle>` root for Checkstyle, and zero
bytes for the two warning-line formats — so CI consumers can ingest
clean runs unchanged.

### Checkstyle (CI integration)

```bash
bca --paths src/ check \
    --threshold cyclomatic=15 \
    --output-format checkstyle \
    --output report.checkstyle.xml
```

The Checkstyle writer emits a single `<checkstyle version="4.3">`
document containing one `<file>` element per source path, each
holding one `<error>` per metric-threshold violation. The schema is
the Checkstyle 4.3 XSD that Jenkins and SonarQube's "Warnings Next
Generation" / "Generic Issue" importers consume directly.

### SARIF (GitHub Code Scanning)

```bash
bca --paths src/ check \
    --threshold cyclomatic=15 \
    --output-format sarif \
    --output report.sarif.json
```

The SARIF writer emits a single SARIF 2.1.0 JSON document with one
`runs[]` element. Each metric-threshold violation becomes a `result`
under `runs[0].results[]`; the metric names appearing in the run are
deduplicated into `runs[0].tool.driver.rules[]` with short
descriptions.

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
          bca --paths . check \
              --config bca-thresholds.toml \
              --output-format sarif \
              --output report.sarif.json \
              --no-fail
      - name: Upload SARIF
        uses: github/codeql-action/upload-sarif@v3
        with:
          sarif_file: report.sarif.json
```

`--no-fail` keeps the job green so the SARIF upload step still runs
when offenders exist; remove it once you want a metric regression to
fail the workflow.

### Clang/GCC warning lines (editor quickfix and CI annotators)

```bash
bca --paths src/ check \
    --threshold cyclomatic=15 \
    --output-format clang-warning \
    --output report.txt
```

The Clang format emits one offender per line in the conventional
compiler-warning shape:

```text
path/to/file.rs:42:5: warning: cyclomatic 17 exceeds limit 15 [big-code-analysis-cyclomatic]
```

This is the format `clang -fdiagnostics-format=` produces and the
shape every editor quickfix parser (VS Code, IntelliJ, Vim) and most
CI annotators understand without configuration.

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
          bca --paths . check \
              --config bca-thresholds.toml \
              --output-format clang-warning \
              --no-fail
```

If your runner does not ship a GCC matcher, fall back to streaming
the lines and re-emitting them as `::warning file=...,line=...::`
workflow commands.

### MSVC warning lines (Visual Studio and Windows CI)

```bash
bca --paths src/ check \
    --threshold cyclomatic=15 \
    --output-format msvc-warning \
    --output report.txt
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
