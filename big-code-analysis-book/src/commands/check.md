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
big-code-analysis-cli --paths src/ check \
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
big-code-analysis-cli --paths src/ check --config bca-thresholds.toml
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
| Maintainability index | `mi.original`, `mi.sei`, `mi.visual_studio` |

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

## Reporting without failing

`--no-fail` (also written `--report-only` in some CI vocabularies)
prints offenders to stderr but exits `0`. Useful while adopting
baselines without flipping CI red.

```bash
big-code-analysis-cli --paths src/ check \
    --config bca-thresholds.toml --no-fail
```

## CI example (GitHub Actions)

```yaml
- name: Check code complexity thresholds
  run: |
    big-code-analysis-cli --paths src/ check --config bca-thresholds.toml
  # The default behavior — non-zero exit fails the step — is exactly
  # what we want here. No extra wiring needed.
```

If you want to keep the job green and surface offenders as a build
annotation while you reduce the count, swap in `--no-fail`:

```yaml
- name: Surface complexity hot spots (non-blocking)
  run: |
    big-code-analysis-cli --paths src/ check \
        --config bca-thresholds.toml --no-fail
```
