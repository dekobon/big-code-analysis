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

- `[new]` — no baseline entry for this `(path, function, start_line,
  metric)` tuple. The violation is new since the baseline was written.
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

When `.bca-baseline.toml` contains only `version = 2` and no entries,
drop the `--baseline` flag from CI and delete the file. The thresholds
now stand on their own.

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

- **Line drift.** Entries key on `(path, function, start_line, metric)`.
  Editing code above a function shifts its `start_line` and the
  baseline entry stops matching, surfacing as a "new" offender. Refresh
  with `--write-baseline` and commit the diff.
- **Path identity.** Entries record the path the walker saw. Run
  `--write-baseline` and `--baseline` from the same working directory
  with the same `--paths` argument; a relative `--paths src/` and an
  absolute `--paths /repo/src/` produce non-matching baselines.
- **OS portability.** Paths are normalized to forward slashes on
  write and re-normalized on read, so a baseline generated on Linux
  matches the same tree on Windows. Non-UTF-8 paths fall back to a
  lossy display form and may not round-trip exactly.
- **Tightening a threshold.** Lowering a limit may newly expose
  functions that were previously clean. They will not be in the
  baseline → CI will fail. This is correct — tightening should expose
  new offenders. Refresh the baseline if the team chooses to absorb
  the new entries.
