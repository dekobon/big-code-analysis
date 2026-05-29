# Baselines: ratcheting thresholds on existing code

When you introduce metric thresholds on an existing codebase, you
usually hit the same wall: every reasonable threshold flags hundreds of
existing functions, and CI goes red on every push. The realistic
adoption path is "ratchet from current state, fail only on new
offenders". The baseline file (issue #99) is how `bca check` supports
that workflow.

Baselines are the complement to in-source suppression markers, not a
substitute. Use suppression markers
([Suppression markers](../commands/suppression.md)) when a function is
intentionally complex forever (a parser, a state machine, generated
code). Use a baseline when the team intends to pay the debt down. Both
can live in the same repo; suppression is checked first.

## End-to-end adoption flow

> One-shot shortcut: `bca init` scaffolds `bca-thresholds.toml`,
> `.bcaignore`, and an initial `.bca-baseline.toml` derived from the
> current tree in a single command. It writes the same files the
> step-by-step recipe below produces; pass `--force` to overwrite
> existing files or `--no-baseline` to skip the walk. The longer
> recipe below is useful when you want to tune thresholds before
> bootstrapping the baseline.

### 1. Pick initial thresholds

Either gut-feel numbers (`cyclomatic=15`, `cognitive=20`) or pull them
from a `bca check --no-fail` run over the repo to see the current
distribution.

```toml
# bca-thresholds.toml
[thresholds]
cyclomatic = 15
cognitive = 20
"loc.lloc" = 200
```

### 2. Bootstrap the baseline

```bash
bca --paths src/ check \
    --config bca-thresholds.toml \
    --write-baseline .bca-baseline.toml
```

Commit both files in the same change:

```bash
git add bca-thresholds.toml .bca-baseline.toml
git commit -m "ci: introduce metric thresholds with baseline"
```

Path keys in the baseline are stored relative to the baseline file's
own directory (the *anchor*). `--paths .`, `--paths src/`, and
`--paths "$PWD"` produce byte-identical baselines, and `--baseline`
runs match regardless of which `--paths` form CI uses — switch
between them freely without re-running `--write-baseline`.

### 3. Wire the CI gate

GitHub Actions:

```yaml
- name: Check code complexity thresholds
  run: |
    bca --paths src/ check \
        --config bca-thresholds.toml \
        --baseline .bca-baseline.toml
```

GitLab CI (snippet for the relevant job):

```yaml
threshold-check:
  image: rust:1
  before_script:
    - cargo install --locked big-code-analysis-cli@<VERSION>
  script:
    - bca --paths src/ check
        --config bca-thresholds.toml
        --baseline .bca-baseline.toml
```

Exit codes: `0` clean, `2` regression or new offender, `1` tool error.
See [CI integration](ci.md) for the broader matrix of CI surfaces.

### 4. Refresh the baseline as the team pays debt down

Every few weeks, or after a focused refactor:

```bash
bca --paths src/ check \
    --config bca-thresholds.toml \
    --write-baseline .bca-baseline.toml
git diff .bca-baseline.toml
```

A shrinking diff is the goal. Two `--write-baseline` runs over an
unchanged tree produce byte-identical output, so spurious diffs only
appear when actual offenders changed.

### 5. PR-review heuristics

- **Baseline shrank.** Debt paid down. No further action.
- **Baseline grew.** Someone added a new offender to the file
  intentionally. Review the values — was this a deliberate stopgap, or
  did the author bypass the gate? Either is fine if conscious; the
  point of the file being committed is to make the choice reviewable.
- **A single entry got a higher `value`.** The author re-ran
  `--write-baseline` after the function got worse. Treat the same as
  "baseline grew" — surface the change in review.

### Reading the gate output

A failing `bca check --baseline` run prefixes each surviving violation
with a tag and follows the list with a per-file rollup:

```text
bca: filtered 422 violations via baseline
[regr +60%] src/foo.rs:1-865: <file>: halstead.effort = 1557107.72 (limit 50000)
[new] src/bar.rs:506-747: act_on_file: cognitive = 63 (limit 25)
...

--- summary ---
src/foo.rs: 5 violations (worst: halstead.effort = 1557107.72 vs limit 50000 at L1)
src/bar.rs: 4 violations (worst: cognitive = 63 vs limit 25 at L506)
```

Tag prefixes:

- `[new]` — no baseline entry matched this violation by qualified
  symbol (within the line tolerance) or, when `--baseline-fuzzy-match`
  is set, by body hash. The violation is new since the baseline was
  written. See [Matching](#how-matching-works) for the resolution
  order.
- `[regr +N%]` — the baseline contains a recorded value and the
  current value is `N%` higher. Cases:
  - `[regr from 0]` when the recorded value is `0.0` and a non-zero
    percentage would divide by zero.
  - `[regr +>9999%]` caps once the regression exceeds 100× the
    baseline value.
  - `[regr NaN]` when the current metric value is NaN (degenerate
    Halstead inputs on trivial functions).

Tags only appear when `--baseline` is passed; without it the line
format is byte-identical to the no-baseline default. CI tooling that
grep-pipes the stderr stream can suppress the trailing summary with
`--no-summary`.

The summary footer groups violations by file, cites the single worst
metric per file (max `value / limit` ratio), and sorts rows by
violation count descending then path ascending. It is the fastest way
to read a long offender list and spot which file to start with.

### 6. Retire the baseline

When `.bca-baseline.toml` contains only `version = 4` and no entries,
drop the `--baseline` flag from CI and delete the file. The thresholds
now stand on their own.

## How matching works

Each entry is keyed on `(path, qualified_symbol, metric)` — the
qualified symbol being the `::`-joined chain of enclosing named
containers plus the function name (`MyStruct::do_thing`,
`my_namespace::MyClass::method`). The top-level file space collapses to
`<file>`. A violation is resolved against the baseline in this order:

1. **Qualified symbol.** If exactly one entry shares the violation's
   `(path, qualified_symbol, metric)`, it matches **regardless of line
   number** — so editing code above a function no longer re-keys it as
   `[new]`.
2. **Start-line tolerance.** If several entries share that key (two
   methods named `is_valid` on different `impl` blocks the analyzer
   could not tell apart, overloads, …), the entry whose recorded
   `start_line` is closest to the violation — and within
   `--baseline-line-tolerance` lines (default 50) — wins. Beyond the
   tolerance the violation is `[new]`.
3. **Body hash (opt-in).** With `--baseline-fuzzy-match`, a violation
   whose qualified symbol no longer matches is matched against entries
   with an identical normalised body hash within the same
   `(path, metric)`. This absorbs a rename that kept the function's
   shape (the digest elides the function's own name and is insensitive
   to indentation, blank lines, and CRLF). The hash is written into the
   baseline only when `--baseline-fuzzy-match` is set, so seed it with
   one fuzzy `--write-baseline` to enable fuzzy reads. Configure both
   keys in `bca.toml` as `baseline_line_tolerance` and
   `baseline_fuzzy_match`.

**Anonymous functions** (closures, lambdas) have no stable name, so
their qualified symbol bakes in the line (`outer::<anon@L42>`). They
therefore re-key as `[new]` when they move — the symbol fix only
survives line drift for *named* top-level and method-bound functions,
which produce the bulk of baseline churn.

### Remediation footer

When the gate finds violations, `bca check` emits a trailing
`--- next steps ---` block on stderr (and inside the
`$GITHUB_STEP_SUMMARY` digest) that names the artifact, prints a
copy-paste-safe `--write-baseline` refresh invocation, and links
back to this recipe. The refresh invocation mirrors the gate's
resolved `--paths` / `--exclude` / `--exclude-from` / `--config` /
`--baseline` arguments, so a first-time reader of a failing CI log
can refresh the baseline without leaving the page.

Suppress the block with `--no-remediation` if downstream tooling
grep-pipes the stderr stream and the trailing block confuses it.

## Composition with suppression markers

`--write-baseline` already excludes any function silenced by a
`bca: suppress` or `#lizard forgives` marker, so the same function
doesn't end up in two places. If a function is intentionally exempt
forever, prefer the in-source marker (lives next to the code, survives
refactors, no extra file to commit). Use the baseline only for
violations the team genuinely intends to fix.

To audit the un-filtered offender set — every violation regardless of
suppression or baseline — pass `--no-suppress` and omit `--baseline`:

```bash
bca --paths src/ check \
    --config bca-thresholds.toml \
    --no-suppress \
    --no-fail
```

Combined with `--write-baseline`, `--no-suppress` records every
violation including the ones that suppression markers normally hide.

## Limitations

- **Ambiguous symbols.** When two functions share a qualified symbol
  (the analyzer could not resolve distinct containers, or a language
  permits overloads) and both have drifted beyond
  `--baseline-line-tolerance` from their recorded lines, neither
  disambiguates and the violations surface as `[new]`. Refresh with
  `--write-baseline`, or raise the tolerance.
- **Anonymous functions.** Closures and lambdas re-key on movement
  because their synthetic symbol embeds the line (see
  [How matching works](#how-matching-works)).
- **OS portability.** Paths are normalized to forward slashes on
  write and re-normalized on read, so a baseline generated on Linux
  matches the same tree on Windows. Non-UTF-8 paths fall back to a
  lossy display form and may not round-trip exactly.
- **Tightening a threshold.** Lowering a limit may newly expose
  functions that were previously clean. They will not be in the
  baseline → CI will fail. This is correct — tightening should expose
  new offenders. Refresh the baseline if the team chooses to absorb
  the new entries.
