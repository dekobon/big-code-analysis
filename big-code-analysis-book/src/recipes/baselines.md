# Baselines: ratcheting thresholds on existing code

Introducing thresholds on an existing codebase can flag violations
unrelated to the current change. A baseline records accepted
violations so the gate fails on new offenders and regressions, and
shrinks as the team pays the recorded debt down. This recipe walks
through adopting one, reading the gate
output it produces, and keeping it fresh; the flag reference is on the
[check](../commands/check.md#baselines) page.

Baselines complement in-source suppression markers; they are not a
substitute. Use [suppression markers](../commands/suppression.md) when
a function is intentionally complex forever (a parser, a state machine,
generated code). Use a baseline when the team intends to pay the debt
down. Both can live in the same repo; suppression is checked first.

## End-to-end adoption flow

> One-shot shortcut: `bca init` scaffolds a consolidated `bca.toml`
> manifest (with `paths`, `exclude_from`, `baseline`, and a
> `[thresholds]` table), the `.bcaignore` it references, and an initial
> `.bca-baseline.toml` derived from the current tree in a single
> command. With the manifest in place, a bare `bca check` discovers it
> and gates zero-config; pass `--force` to overwrite existing files or
> `--no-baseline` to skip the walk. The longer
> recipe below is useful when you want to tune thresholds before
> bootstrapping the baseline.

### 1. Pick initial thresholds

Either gut-feel numbers (`cyclomatic=15`, `cognitive=20`) or pull them
from a `bca check --no-fail` run over the repo to see the current
distribution.

```toml
# bca.toml at the repo root, discovered automatically by `bca check`.
paths = ["src"]

[check]
baseline = ".bca-baseline.toml"

[thresholds]
cyclomatic = 15
cognitive = 20
"loc.lloc" = 200
```

### 2. Bootstrap the baseline

```bash
bca check --write-baseline
```

A bare `--write-baseline` (no path) writes to the `baseline` key from
the `bca.toml` you just created, so the filename lives in exactly one
place. Pass an explicit path (`--write-baseline <file>`) only when you
have no manifest `baseline` to default to; without one, the bare form
errors rather than guessing a filename.

Commit both files in the same change:

```bash
git add bca.toml .bca-baseline.toml
git commit -m "ci: introduce metric thresholds with baseline"
```

Path keys in the baseline are stored relative to the baseline file's
own directory (the *anchor*). `--paths .`, `--paths src/`, and
`--paths "$PWD"` produce byte-identical baselines, and `--baseline`
runs match whichever `--paths` form CI uses. Switch between them
freely without re-running `--write-baseline`.

### 3. Wire the CI gate

[GitHub Actions](https://docs.github.com/en/actions):

```yaml
- name: Check code complexity thresholds
  run: |
    bca check
  # `paths`, thresholds, and `baseline` all come from the
  # auto-discovered `bca.toml` manifest at the repo root.
```

[GitLab CI/CD](https://docs.gitlab.com/ci/) (job excerpt):

```yaml
threshold-check:
  image: rust:1
  before_script:
    - cargo install --locked big-code-analysis-cli@<VERSION>
  script:
    - bca check
```

Exit codes: `0` clean, `2` regression or new offender, `1` tool error.
See [CI integration](ci.md) for the broader matrix of CI surfaces.

### 4. Refresh the baseline as the team pays debt down {#4-refresh-the-baseline-as-the-team-pays-debt-down}

Every few weeks, or after a focused refactor:

```bash
cp .bca-baseline.toml .bca-baseline.old.toml
bca check --write-baseline
bca diff-baseline .bca-baseline.old.toml .bca-baseline.toml
```

A shrinking diff is the goal. Two `--write-baseline` runs over an
unchanged tree produce byte-identical output, so spurious diffs only
appear when actual offenders changed.

You do not have to guess when a refresh is due. A gated run whose
offenders have measured *better* than their recorded values says so on
stderr (see [the stale-entry warning](#the-stale-entry-warning)),
because the difference between recorded and live is suppression nobody
chose.

#### Before tightening a limit, price it at both tiers {#price-a-candidate-limit}

Paying debt down invites tightening the limit that produced it, and the
obvious measurement, `bca check --threshold <metric>=<candidate>`,
answers only half the question. A `--threshold` value is applied last
and absolutely, never scaled, so it has **no soft tier**. A candidate
that costs nothing at the hard gate can still put a whole population
permanently inside the `--tier=soft` band, and you will not see it
until you have edited `bca.toml` and run the other gate.

```bash
bca check --explain-threshold cognitive=15
```

Weigh the `new` column, not the offender count: it is how many baseline
entries the change would add, and it is what a reviewer is being asked
to approve. A `cluster:` line means the candidate landed on top of an
existing population, and those entries can never be retired; see
[Tightening a limit onto a cluster](thresholds.md#converging-onto-a-cluster).

### 5. PR-review heuristics

Run `bca diff-baseline <old> <new>` and read the summary instead of
parsing a raw `git diff .bca-baseline.toml` in your head. It pairs
entries on their `(path, qualified, metric)` identity, so a function
that merely drifted up or down the file is *not* reported as a
remove + add, and buckets every real change:

```text
1 added, 1 removed, 2 worsened, 0 improved

## Added
  src/new.rs::shiny        cognitive  = 30

## Removed
  src/gone.rs::old_fn      nargs      = 9

## Worsened
  src/bar.rs::act_on_file  cognitive  60 → 63
  src/foo.rs::do_thing     cognitive  25 → 27
```

Map the buckets back to the old heuristics:

- **`removed` (baseline shrank).** Debt paid down. No further action.
- **`added` (baseline grew).** Someone added a new offender to the
  file intentionally. Review the values: was this a deliberate
  stopgap, or did the author bypass the gate? Either is fine if
  conscious; the point of committing the file is to make the choice
  reviewable.
- **`worsened` (an entry got a higher `value`).** The author re-ran
  `--write-baseline` after the function got worse. Treat it the same
  as `added` and surface the change in review.
- **`improved`.** A recorded offender got better without dropping out
  of the baseline. That is a good sign the refactor is working, and
  the reason to land the refreshed file rather than leave it. Until it
  is refreshed, the old entry keeps suppressing the offender up to a
  value the tree no longer produces.

For a PR bot, `bca diff-baseline <old> <new> --format markdown` emits
a fenced block ready to drop into a sticky comment, and the
`--worsened-only` / `--added-only` filters narrow it to just the
regressions reviewers must look at. `--format json` feeds the same
diff to other tooling. The command exits 0 by default, because it
informs review rather than gating (the gate is `bca check` itself);
the opt-in `--exit-code` flag exits 2 on a non-empty filtered diff.

### Reading the gate output {#reading-the-gate-output}

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

- `[new]`: no baseline entry matched this violation by qualified
  symbol (within the line tolerance) or, when `--baseline-fuzzy-match`
  is set, by body hash. The violation is new since the baseline was
  written. See [Matching](#how-matching-works) for the resolution
  order.
- `[regr …]`: the baseline contains a recorded value and the current
  value is worse than it. "Worse" is measured in the metric's own
  direction, so for the lower-is-worse `mi.*` family a *drop* below the
  recorded value reads `+` like any other regression. The amount takes
  one of two shapes, both starting `[regr `, so `rg '\[regr '` matches
  every regressed row:
  - `[regr +N%]` for a count (every metric outside `mi.*`): `N` is the
    rise as a percentage of the recorded value, rounded to an integer.
  - `[regr +N.N pts]` for `mi.original`, `mi.sei`, and
    `mi.visual_studio`: `N.N` is the drop in index points, to one
    decimal. The Maintainability Index has an arbitrary zero and
    `mi.original` and `mi.sei` go negative on large functions, so a
    percentage of the record means nothing: a drop from −10 to −20
    reads `[regr +10.0 pts]`, and a 0.01-point drop from a record of
    0.001 reads `[regr +0.0 pts]` rather than `+1069%`.

  Special forms:
  - `[regr from 0]` when a count's recorded value is `0.0` and a
    percentage would divide by zero. An `mi.*` record of zero is an
    ordinary point and reads in points.
  - `[regr +>9999%]` caps once a count's regression exceeds 100× the
    baseline value. Points are not capped.
  - `[regr NaN]` when the current metric value is NaN (degenerate
    Halstead inputs on trivial functions).

Tags only appear when `--baseline` is passed; without it the line
format is byte-identical to the no-baseline default. CI tooling that
reads the merged streams can suppress the trailing summary with
`--no-summary`.

The summary footer groups violations by file, cites the single worst
metric per file (by severity: `value / limit` for a count, one step
per maintainability band below the floor for `mi.*`), and sorts rows by
violation count descending then path ascending. Use the footer to
identify files with the most violations before reading individual
offender rows.

### Merging two branches

A baseline is generated wholesale, so a *textual* merge of two branches
that both touched it is never right: each side's values were measured
against its own tree, and the merged tree matches neither. Tell git not
to try, with one line in `.gitattributes`:

```gitattributes
.bca-baseline.toml -merge
```

Git then leaves the file conflicted as a whole instead of splicing the
two sides together, and the resolution is always the same: regenerate.

```bash
bca check --write-baseline
git add .bca-baseline.toml
```

Unlike a `merge=ours` driver, `-merge` needs no per-clone `git config`,
so it works for everyone who clones the repository rather than only for
those who already knew to set it up.

### 6. Retire the baseline

When `.bca-baseline.toml` contains only `version = 6` and no entries,
drop the `--baseline` flag from CI and delete the file. The thresholds
now stand on their own.

## Tier/headroom provenance {#tierheadroom-provenance}

A baseline written with `--write-baseline` (v5+) records *which gate it
was written against* in a `[provenance]` table:

```toml
version = 6

[provenance]
tier = "soft"
headroom = 0.95
mi_charges_docs = false
mi_formula = 2
```

- `tier = "hard"`: written by the hard gate (`bca check
  --write-baseline …`); no `headroom` key.
- `tier = "soft"`, `headroom = <ratio>`: written by the soft gate
  scaled by the soft ratio (`bca check --tier=soft=0.95
  --write-baseline …`).
- `tier = "soft"` with no `headroom`: written by the soft gate driven
  by a `[thresholds.soft]` table (per-metric limits, no single ratio).
- `mi_charges_docs`: whether the maintainability index charged
  documentation rows when the file was written (the `--mi-charges-docs`
  flag or manifest key; see
  [Charging documentation](../metrics.md#mi-charges-docs)). Absent from
  a file written before the key existed, which reads as `true`: every
  earlier `bca` charged documentation.
- `mi_formula`: the revision of the maintainability-index formula the
  file's `mi.*` values were computed under (2 since the size term moved
  to `PLOC` and `mi.sei` to a 0–1 comment ratio). Absent from a file
  written before the key existed, which reads as revision 1.

The provenance is a real TOML table, not a comment, so `bca
diff-baseline` and external tooling can read it. Baselines written by an
older bca (v2–v4) carry no provenance and are read without error.

### The stricter-than-baseline warning

`bca check` reduces the baseline's provenance and the current run's
effective limits to a single *strictness* scalar (hard → `1.0`; soft
scaled by `h` → `h`; smaller means stricter) and warns when the current
run is **stricter** than the baseline was written against:

```text
warning: this check's effective limits (strictness 0.9) are stricter
than the baseline was written against (strictness 0.95); the baseline
may under-cover and the gate can fire on untouched files. Refresh it at
the matching tier, …
```

This is the silent-desync the [baseline-refresh
discipline](#4-refresh-the-baseline-as-the-team-pays-debt-down) guards
against: a baseline written *looser* than the current gate may not list
every offender the tighter gate produces, so the gate can suddenly fire
on files nobody touched.

The warning is **directional**. It fires only when the current run is
stricter. It stays silent in the safe direction: a hard check
(strictness `1.0`) reading a soft-`0.95` baseline sees a *superset* of
its offenders, which is the intended single-baseline setup where
`make self-scan` (hard) and `make self-scan-headroom` (soft) ratchet
through the same `.bca-baseline.toml`. It also stays silent for equal
provenance, for pre-v5 baselines (provenance unknown), and when either
side is a `[thresholds.soft]`-table baseline (no single ratio to
compare). To clear a real warning, refresh the baseline at the current
tier with the matching `--write-baseline` recipe.

### The MI formula-revision warning {#the-mi-formula-revision-warning}

A baseline's `mi.*` values are only comparable with a run that computes
MI the same way. When the formula itself changes, every recorded value
moves for unchanged code, so `bca check` warns whenever a baseline
holding any `mi.*` entry records a different `mi_formula` revision from
the one the run computes, in either direction:

```text
warning: this baseline records mi.* values under MI formula revision 1;
this run computes revision 2. The recorded values are not comparable
and can misclassify a regression. Review any mi.* threshold and refresh
with `--write-baseline <file>`.
```

It does not wait for an `mi.*` offender: it names the stale file on
every read, so the record is refreshed before an `mi.*` gate comes to
depend on it. A file written before the key existed reads as revision
1, so the first check after upgrading warns. While it fires, the
documentation-mode warning below is not shown: a baseline from another
formula is not worth comparing mode by mode. Refresh it with
`--write-baseline`. `bca diff-baseline` warns the same way when its two
files record different revisions and either holds an `mi.*` entry,
since their `mi.*` deltas then reflect the formula, not the code.

### The MI documentation-mode warning

The `mi.sei` values a baseline records depend on whether documentation
rows were charged; `mi.original` and `mi.visual_studio` do not.
Including documentation rows raises `mi.sei`. A baseline recorded with
documentation included can therefore reject unchanged code when the
current run excludes documentation. Reversing the modes can hide a
regression. `bca check` warns in either direction:

```text
warning: this check computes mi.* leaving documentation out
(mi_charges_docs = false) but the baseline was written charging
documentation (mi_charges_docs = true); its mi.sei entries describe a
different comment share (mi.original and mi.visual_studio read the
same in both modes), …
```

A baseline that records no mode was written by a `bca` that charged
documentation, so it reads as `mi_charges_docs = true`. Every such file
a release wrote also predates the formula stamp, so after upgrading the
formula-revision warning above fires instead. The documentation-mode
warning stays silent when the modes agree and when no `mi.sei` value is
read against the baseline: a baseline holding no `mi.sei` entry, or a
run with no `mi.sei` offender, is unaffected by the mode. Clear it by
refreshing under the current mode, or by passing the
`--mi-charges-docs=<bool>` the message names to match the baseline.

## The stale-entry warning {#the-stale-entry-warning}

The ratchet suppresses a violation for as long as it has not *worsened*
past its recorded value, so everything on the improving side classifies
alike: a function measuring 5 against a recorded 7 is treated exactly
like one still measuring 7. The two points between them are gate
headroom nobody chose; the function can grow back to 7 and the gate
will not notice.

A gated run reports that drift in one stderr line:

```text
warning: 3 baseline entries improved past the recorded value (worst:
src/spaces/compute.rs::metrics_inner halstead.effort 119147.75 →
116715.61); that gap is gate headroom nobody chose, so refresh with
`--write-baseline`. …
```

It names one example rather than every entry, because the response to
any number of them is the same wholesale refresh. Direction follows the
metric: for the lower-is-worse `mi.*` family the stale direction is a
*rise* above the record. An entry sitting exactly on its recorded value
is silent, so a freshly written baseline never warns about itself.

**It finds only half of the staleness.** A function that stopped
breaching its limit altogether produces no violation at all, so it never
reaches the baseline matcher and cannot be counted here. Its entry stays
in the file, inert and invisible, until someone regenerates.

This repository closes the other half with a scheduled job rather than a
gate, and the shape is worth copying. `baseline-freshness.yml` runs
quarterly. It regenerates the baseline with the same `--write-baseline`
recipe that wrote the committed one (the job pins that recipe, so a
tier change has to land in both places), diffs the two with `bca
diff-baseline --exit-code`, and files a labelled issue naming what
moved. Two details of the job carry its design:

- It uses `diff-baseline` rather than `git diff`, because `git diff`
  reports a moved `start_line` as staleness.
- It looks up an existing issue by label *and* title before filing,
  because a stale baseline persists until someone commits a refresh, so
  an unconditional file would open one issue per quarter for the same
  entry. The label alone is shared with other findings; the fixed title
  is what keeps the comment on the right issue.

Nothing is gated on the result: the tree is fine, the file describing
it has aged.

## How matching works {#how-matching-works}

Each entry is keyed on `(path, qualified_symbol, metric)`. The
qualified symbol is the `::`-joined chain of enclosing named containers
plus the function name (`MyStruct::do_thing`,
`my_namespace::MyClass::method`). The top-level file space collapses to
`<file>`. A violation is resolved against the baseline in this order:

1. **Qualified symbol.** If exactly one entry shares the violation's
   `(path, qualified_symbol, metric)`, it matches **regardless of line
   number**, so editing code above a function does not re-key it as
   `[new]`.
2. **Start-line tolerance.** If several entries share that key (two
   methods named `is_valid` on different `impl` blocks the analyzer
   could not tell apart, overloads, …), the entry whose recorded
   `start_line` is closest to the violation, and within
   `--baseline-line-tolerance` lines (default 50), wins. Beyond the
   tolerance the violation is `[new]`.

   This is the only rule that reads a line number, so from schema v6 a
   `start_line` is written **only** for entries in such a group.
   Everywhere else the field is absent, and the file therefore does not
   change when an edit above a baselined function moves it. That is
   what keeps a real baseline change (a `value` moving) visible in a
   diff instead of buried in line-number noise.
3. **Body hash (opt-in).** With `--baseline-fuzzy-match`, a violation
   whose qualified symbol no longer matches is matched against entries
   with an identical normalised body hash within the same
   `(path, metric)`. This absorbs a rename that kept the function's
   shape (the digest elides the function's own name and is insensitive
   to indentation, blank lines, and CRLF). The hash is written into the
   baseline only when `--baseline-fuzzy-match` is set, so seed it with
   one fuzzy `--write-baseline` to enable fuzzy reads. Configure both
   keys under `[check]` in `bca.toml` as `baseline_line_tolerance` and
   `baseline_fuzzy_match` (the bare top-level spelling is deprecated and
   warns; see #599).

**Anonymous functions** (closures, lambdas) have no stable name, so
their qualified symbol bakes in the line (`outer::<anon@L42>`). They
therefore re-key as `[new]` when they move. Symbol matching survives
line drift only for *named* top-level and method-bound functions,
which produce the bulk of baseline churn.

### Remediation footer

When the gate finds violations, `bca check` emits a trailing
`--- next steps ---` block on stderr (the offender rows themselves go
to stdout) and inside the `$GITHUB_STEP_SUMMARY` digest. The block
names the artifact, prints a copy-paste-safe `--write-baseline`
refresh invocation, and links back to this recipe. The refresh
invocation mirrors the gate's resolved `--paths` / `--exclude` /
`--exclude-from` / `--config` / `--baseline` arguments, so a
first-time reader of a failing CI log can refresh the baseline without
leaving the page.

Suppress the block with `--no-remediation` if downstream tooling
reads the merged streams and the trailing block confuses it.

## Composition with suppression markers

`--write-baseline` already excludes any function silenced by a
`bca: suppress` or `#lizard forgives` marker, so the same function
does not end up in two places. If a function is intentionally exempt
forever, prefer the in-source marker: it lives next to the code,
survives refactors, and needs no extra file to commit. Use the
baseline only for violations the team intends to fix.

To audit the un-filtered offender set (every violation regardless of
suppression or baseline), pass `--no-suppress` and omit `--baseline`:

```bash
bca check --paths src/ \
    --no-suppress \
    --no-fail
```

Combined with `--write-baseline`, `--no-suppress` records every
violation including the ones that suppression markers normally hide.

## Auditing every exemption at once

A baseline is one of three ways code escapes the gate; the other two
are in-source `bca: suppress` markers and `[check.exclude]` globs.
`bca exemptions` lists all three tiers in a single report so a reviewer
can see everything `bca check` is skipping without running three
commands:

```bash
bca exemptions --paths src/
```

```text
# In-source markers (2)
  src/parser.rs:120  bca: suppress       metrics=all  parse_long
  ...

# [check.exclude] globs (1)
  tests/**

# Baseline (.bca-baseline.toml, 417 entries)
  src/markdown_report.rs write_language_section cognitive 29
  ...
```

The baseline section reads the same `--baseline` / `bca.toml`
`[check] baseline` source `bca check` does (or `.bca-baseline.toml`
by default).
Use `--baseline-only` to list just the baselined offenders, `--format
markdown` for a PR comment, or `--format json` for dashboards. During
PR review, pair it with `bca diff-baseline <old> <new>` (above): the
diff shows what *changed* in the baseline, `bca exemptions` shows the
full current exemption surface. See the
[Suppression markers](../commands/suppression.md#auditing-exemptions-bca-exemptions)
page for the complete flag reference.

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
  matches the same tree on Windows. On Unix and Windows the path key
  is percent-encoded, so a non-UTF-8 path keeps its identity rather
  than collapsing onto a lossy display form; the backslash-to-slash
  normalization is the one deliberate exception.
- **Tightening a threshold.** Lowering a limit may newly expose
  functions that were previously clean. They will not be in the
  baseline, so CI will fail. This is correct: tightening should expose
  new offenders. Refresh the baseline if the team chooses to absorb
  the new entries.
