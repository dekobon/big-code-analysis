# Check

`bca check` evaluates per-function metrics against thresholds and exits
non-zero when any function exceeds a limit. It is the CI integration
point: wire it into a build step and a regression in code complexity
fails the pipeline before the change lands.

> **Looking for full CI recipes?** The
> [CI integration recipe](../recipes/ci.md) consolidates the
> `--output-format` matrix, runnable GitHub Actions and `.gitlab-ci.yml`
> examples, the baseline / ratchet pattern, and the GitLab Code Quality
> path. This page documents the command itself; the recipe documents
> how to wire it into a pipeline.

## Exit codes

| Code | Meaning |
|------|---------|
| `0`  | All functions within thresholds (or `--no-fail` set). |
| `2`  | At least one threshold exceeded. |
| `1`  | Tool error (bad arguments, unreadable config, unknown metric). |

`1` is reserved so CI can distinguish a regression (`2`) from a tool
misconfiguration (`1`).

### Tiered exit codes (`--strict-exit-codes`)

`--strict-exit-codes` (or `[check] exit_codes = "tiered"` in
`bca.toml`) splits the single violation code `2` by severity so CI can
branch on it without parsing the `[new]` / `[regr +N%]` stderr tags:

| Code | Meaning (tiered mode) |
|------|-----------------------|
| `0`  | All functions within thresholds (or `--no-fail` set). |
| `1`  | Tool error. |
| `2`  | New offenders only (no `--baseline` entry matched). |
| `3`  | Baseline regressions only (a baselined offender worsened). |
| `4`  | Both new offenders and regressions. |
| `5`  | A `--tier=soft` violation that also breaches the hard limit. |

The tiered codes are opt-in; the default contract above stays
`0`/`1`/`2`. Every fail-state remains non-zero, so `exit != 0 → fail`
wrappers keep working — only tooling that tests `$? -eq 2` explicitly
needs to widen to `2`-`5`. `--no-fail` still forces exit `0`. Code `5`
is emitted only at the soft tier; at the hard tier every violation is a
hard breach by definition, so the `2`/`3`/`4` split applies instead.
The manifest key can only enable the tiered mode (`--strict-exit-codes`
ORs on top); an invalid `exit_codes` value is a tool error (`1`).
`--print-effective-config` reports the resolved `exit_codes` style.

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

Or keep thresholds in the `bca.toml` manifest (one place to version CI
thresholds alongside the code). Dropped at the repo root, it is
auto-discovered — a bare `bca check` reads it with no `--config` flag:

```toml
# bca.toml
paths = ["src"]

[thresholds]
cyclomatic = 15
cognitive = 20
"loc.lloc" = 200
"halstead.volume" = 1000
```

```bash
bca check
```

To merge a separate threshold file on top of the manifest for one run,
pass it explicitly with `--config`; CLI flags and `--config` values
override the manifest for the same metric name, so you can keep a
project-wide default and tighten a single metric for a specific run:

```bash
bca --paths src/ check --config bca.toml
```

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

## Two-tier thresholds (`--tier`)

`--tier <hard|soft>` selects which threshold tier the gate compares
against. `hard` (the default) uses the `[thresholds]` table verbatim;
`soft` is an early-warning tier that fires *before* the hard gate.

A `[thresholds.soft]` table sets per-metric soft limits, each either an
absolute number or a `"<ratio>x"` string that scales the metric's hard
limit:

```toml
[thresholds]
cognitive  = 25
cyclomatic = 15
nargs      = 7

[thresholds.soft]
cognitive  = 22       # absolute soft limit
cyclomatic = "0.9x"   # 90% of the hard limit → 13.5
# nargs absent → soft tier inherits the hard limit (no soft band)
```

```bash
bca --paths src/ check --tier soft
```

The soft tier resolves in a fixed order:

1. Start from `[thresholds]` (a `bca.toml` manifest, merged with
   `--config`).
2. If a `[thresholds.soft]` table exists, merge its overrides on top;
   metrics absent from it inherit their hard limit. `--headroom` is
   then ignored with a warning (explicit per-metric limits win).
3. Otherwise scale every limit by `--headroom` (default `0.95` when
   unset; `--headroom 1.0` disables scaling).
4. Repeated `--threshold name=value` flags apply last, absolutely.

`--headroom` is a soft-tier dial: at the default hard tier it is
ignored with a note. The scale factor in a `"<ratio>x"` string (and
`--headroom`) must be in `(0, 1]`. Both tiers ratchet through the same
`--baseline`, and `--print-effective-config` reports the resolved
`tier` alongside the post-merge limits. See the
[Local threshold gates](../recipes/local-gates.md#two-tier-thresholds)
recipe for the migration tip and rationale.

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
excluding it from the walk. The native dialect is `bca: suppress` /
`bca: suppress-file`; Lizard's `#lizard forgives` is recognized as a
compatibility shim. See [Suppression markers](suppression.md) for
the full reference and the `--no-suppress` CI-audit flag.

## Exempting whole file categories (`[check.exclude]`)

Some files should be **analysed and reported but never gated**: test
fixtures that intentionally trip cognitive/cyclomatic, generated
bindings, macro-dispatch modules whose complexity is structural and
will never be "fixed". Putting these in `.bcaignore` is too blunt — it
removes them from the walk entirely, so `bca report` loses them too.
Baselining them is also wrong — they are not debt being paid down, and
they churn the baseline diff forever.

`[check.exclude]` is the glob-level middle ground: matching files are
walked, parsed, metric'd, and shown by `bca report`, but `bca check`
drops their violations before emitting offenders **and before
`--write-baseline` records anything**, so the structural exemptions
stay out of `.bca-baseline.toml`.

In `bca.toml`:

```toml
[check]
exclude = [
    "tests/**",
    "src/languages/language_*.rs",
    "xtask/**",
]
```

Or on the command line (`--check-exclude` is repeatable and unions with
`--check-exclude-from`):

```bash
bca check --check-exclude "tests/**" --check-exclude "xtask/**"
bca check --check-exclude-from .bcacheckignore
```

`--check-exclude-from` reads a `.gitignore`-style file (blank lines and
`#`-comments skipped); the conventional name is `.bcacheckignore`,
mirroring `.bcaignore` for the walker. Globs match the path exactly as
the walker matched it for `--exclude`. An explicit `--check-exclude`
list replaces (does not append to) the manifest `[check] exclude` list,
matching the CLI-wins precedence used for every other manifest key.

### Precedence with the other suppression mechanisms

Most-specific to least, `bca check` resolves exemptions in this order:

1. **In-source markers** (`bca: suppress` / `bca: suppress-file`) —
   always win; applied during the walk so the function never becomes a
   violation.
2. **`[check.exclude]` globs** — exempt *categories* of files (tests,
   generated code).
3. **`.bca-baseline.toml`** — known offenders being paid down.

`--print-effective-config` reports the resolved `check_exclude` globs
alongside the other gate inputs.

## Baselines

When you adopt thresholds on an existing codebase you typically face a
binary choice between "raise the limit until nothing fires" and "fix
every offender before turning the gate on". A baseline file is the
ratchet-down alternative: record today's offenders, fail only on
regressions and new offenders, and shrink the file over time as the
team pays down debt.

Baselines are **complementary to** the suppression markers from
[Suppression markers](suppression.md), not a substitute. Suppressions
express "this function is intentionally exempt forever" and live in
source; baselines express "this is tech debt we're paying down" and
live in a committed TOML file. `bca check` honors suppressions first
and applies the baseline filter to whatever remains.

### Writing a baseline

```bash
bca --paths src/ check \
    --write-baseline .bca-baseline.toml
```

This walks the tree, captures every threshold violation that would
otherwise fail the check, and writes them to the file as sorted TOML.
The run exits `0` regardless of offender count — the point is to
capture them.

```toml
# bca baseline file. Generated by `bca check --write-baseline`.
# Listed offenders are filtered from threshold checks; a function that
# gets worse than its recorded value still fails. Refresh with
# `--write-baseline` when entries become stale.
version = 5

[provenance]
tier = "hard"

[[entry]]
path = "src/parser.rs"
qualified = "Parser::parse_expression"
start_line = 42
metric = "cyclomatic"
value = 22.0
```

The `qualified` field is the function's qualified symbol (the
`::`-joined chain of enclosing named containers plus the function
name); `start_line` is retained only to disambiguate a symbol shared by
several functions. With `--baseline-fuzzy-match`, each entry also
carries a `body_hash` for rename-tolerant matching.

Functions already covered by an in-source suppression marker are
excluded. Pass `--no-suppress` together with `--write-baseline` to
record every violation (CI-auditor flow).

`--write-baseline` cannot be combined with `--baseline`,
`--output-format`, or `--output` — the baseline file *is* the output.

### Reading a baseline

```bash
bca --paths src/ check \
    --baseline .bca-baseline.toml
```

A violation is suppressed when both conditions hold:

- An entry matches by `(path, qualified_symbol, metric)` — independent
  of line number — or, failing that and with `--baseline-fuzzy-match`,
  by body hash. (See the [Baselines recipe](../recipes/baselines.md#how-matching-works)
  for the full resolution order.)
- The current `value` is **less than or equal to** the recorded value.

A function that gets worse than its baseline value still fails. New
offenders not listed in the baseline still fail. Improvements pass
silently (the entry remains at its older, higher value until the next
`--write-baseline` refresh).

A baseline file that does not exist, is empty, has a missing or
unsupported `version`, or fails to parse is a tool error (exit `1`),
not a silent zero-match.

Path keys are canonicalised relative to the baseline file's own
directory (the *anchor*), so `--paths .`, `--paths src/`, and
`--paths "$PWD"` produce byte-identical baselines and a `--baseline`
run matches regardless of which `--paths` form generated the file —
switch between them freely without re-running `--write-baseline`.

### Limitations

- **Ambiguous symbols / anonymous functions.** Entries key on the
  qualified symbol, so inserting code above a *named* function no
  longer re-keys it. The exceptions: functions sharing a qualified
  symbol that drift beyond `--baseline-line-tolerance` apart, and
  anonymous closures/lambdas (whose synthetic symbol embeds the line).
  Both re-key as "new" on movement; refresh with `--write-baseline`.
- **OS portability.** Paths are stored with forward slashes so a
  baseline written on one OS matches the same tree on another. Paths
  that are not valid UTF-8 fall back to a lossy display form
  (U+FFFD substitution) and may not round-trip exactly.

See the [Baselines recipe](../recipes/baselines.md) for the end-to-end
adoption flow and CI integration patterns.

## Reporting without failing

`--no-fail` prints offenders to stderr but exits `0`. Useful while
adopting baselines without flipping CI red. Other CI tools call this
behavior `--report-only` or `--soft-fail`; here the flag is spelled
`--no-fail`.

```bash
bca --paths src/ check --no-fail
```

## Actionable failure output

When `bca check` fails, five flags shape the failure stream so a
developer skimming a CI log can see what tripped, where in their
PR it tripped, and what to do next. Each flag is independent and
all auto-detect from GitHub Actions env vars when present, so the
common CI case needs zero explicit configuration.

| Flag                       | Effect                                                                   | Auto-detect env                                                  |
| -------------------------- | ------------------------------------------------------------------------ | ---------------------------------------------------------------- |
| `--since <ref>`            | Partition per-file footer into "Files in this range" + "Other offenders" | `BCA_DIFF_BASE`, `GITHUB_BASE_REF`, `GITHUB_EVENT_BEFORE`        |
| `--changed-only`           | Drop violations outside the diff scope entirely                          | Requires a resolvable base (`--since` or one of the above)       |
| `--github-annotations`     | Emit `::error file=…::msg` workflow commands for inline file annotations | `GITHUB_ACTIONS == "true"`                                       |
| `--summary-file <path>`    | Append markdown digest (per-file rollup + breakdown + top-10 offenders)  | `GITHUB_STEP_SUMMARY`                                            |
| `--no-remediation`         | Suppress the trailing `--- next steps ---` block                         | Block emitted on failure unless this flag is passed              |

The per-violation stderr lines and the per-file rollup footer
remain unchanged when none of the above are active, so existing
CI tooling that grep-anchors on the legacy output keeps working.

See the [CI integration recipe](../recipes/ci.md#actionable-failure-output)
for worked examples — including a "putting it all together" GHA
snippet that composes all five into one step — and the
[Baselines recipe](../recipes/baselines.md) for the
`--write-baseline` refresh flow the remediation block links to.

### Diff-base auto-detection precedence

When `--since` is omitted, `bca` consults env vars in this order:

1. `BCA_DIFF_BASE` — explicit override hatch for local shells or
   non-GHA CI runners.
2. `GITHUB_BASE_REF` — set by GHA on `pull_request` events.
   Expanded to `origin/<value>`; the runner is responsible for the
   corresponding `git fetch` (`fetch-depth: 0` on `actions/checkout`).
3. `GITHUB_EVENT_BEFORE` — set by GHA on `push` events to the SHA
   at HEAD before the push. The all-zeroes sentinel (force push,
   brand-new branch) is treated as no signal.

Failing to resolve a base is non-fatal **unless `--changed-only`
is passed**, in which case the gate dies — silently suppressing
every violation under a misconfigured base would be the worst
failure mode this feature exists to prevent. `--write-baseline`
also conflicts with `--since` / `--changed-only` (a partial
baseline would silently mask every offender outside the diff scope
on the next full-tree run).

## CI example (GitHub Actions)

```yaml
- name: Check code complexity thresholds
  run: |
    bca check
  # Thresholds and paths come from the auto-discovered `bca.toml`
  # manifest at the repo root. The default behavior — non-zero exit
  # fails the step — is exactly what we want here. No extra wiring.
```

If you want to keep the job green and surface offenders as a build
annotation while you reduce the count, swap in `--no-fail`:

```yaml
- name: Surface complexity hot spots (non-blocking)
  run: |
    bca --paths src/ check --no-fail
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
| `code-climate`  | GitLab MR Code Quality widget                           |
| `clang-warning` | Editor quickfix parsers, GitHub Actions problem matcher |
| `msvc-warning`  | Visual Studio, VS Code, Windows CI runners              |

When no offenders exist the writer emits a well-formed but empty
document — empty `runs[].results` array for SARIF, empty JSON array
(`[]`) for Code Climate, no `<file>` children under the
`<checkstyle>` root for Checkstyle, and zero bytes for the two
warning-line formats — so CI consumers can ingest clean runs
unchanged.

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

### GitLab Code Quality (Code Climate JSON)

```bash
bca --paths src/ check \
    --threshold cyclomatic=15 \
    --output-format code-climate \
    --output gl-code-quality-report.json
```

The Code Climate writer emits a single JSON array of issue objects
matching [GitLab's strict subset](https://docs.gitlab.com/ci/testing/code_quality/)
of the upstream Code Climate engine spec — one entry per
metric-threshold violation, no byte-order-mark, one trailing
newline (empty input renders as `[]\n`). Each issue carries a
namespaced `check_name` (`big-code-analysis/<metric>`), a stable
SHA-256 `fingerprint` over `path \0 function \0 metric` (line- and
value-insensitive so cosmetic edits still dedup in the MR widget),
and a `severity` mapped from the value/threshold ratio onto
GitLab's five-level enum: `≤ 1.5×` → `minor`, `≤ 2×` → `major`,
`≤ 4×` → `critical`, `> 4×` → `blocker` (inverted for the `mi.*`
family where lower is worse). The full enum is
`info`/`minor`/`major`/`critical`/`blocker`; `bca` never emits
`info` — a threshold violation always lands at `minor` or higher.

To wire the artifact into GitLab's MR Code Quality widget:

```yaml
code_quality:
  stage: quality
  script:
    - bca --paths "$CI_PROJECT_DIR" check
          --output-format code-climate
          --output gl-code-quality-report.json
          --no-fail
  artifacts:
    when: always
    reports:
      codequality: gl-code-quality-report.json
    paths:
      - gl-code-quality-report.json
```

See the
[GitLab Code Quality widget recipe](../recipes/ci.md#gitlab-code-quality-widget)
for the full pipeline (combined Code Climate + Checkstyle + Markdown
report) and a local `jq` smoke check.

`--no-fail` keeps the job green so the Code Quality report still
uploads when offenders exist; remove it once you want a metric
regression to fail the pipeline.

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
