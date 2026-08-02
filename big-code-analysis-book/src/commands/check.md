# Check

`bca check` evaluates per-function metrics against thresholds and exits
non-zero when any function exceeds a limit. It is the CI integration
point: wire it into a build step and a regression in code complexity
fails the pipeline before the change lands.

> **Looking for full CI recipes?** The
> [CI integration recipe](../recipes/ci.md) consolidates the
> `--report-format` matrix, runnable GitHub Actions and `.gitlab-ci.yml`
> examples, the baseline / ratchet pattern, and the GitLab Code Quality
> path. This page documents the command itself; the recipe documents
> how to wire it into a pipeline.

## Exit codes

| Code | Meaning |
|------|---------|
| `0`  | All functions within thresholds (or `--no-fail` set). |
| `2`  | At least one threshold exceeded. |
| `1`  | Tool error (bad arguments, unreadable config or input, unknown metric). |

`1` is reserved so CI can distinguish a regression (`2`) from a tool
misconfiguration (`1`).

A gate that could not read all of its input has no verdict to report,
so three input problems exit `1` rather than `0`: nothing matched
`--paths` / `--include` / `--exclude`, any input file that failed to
read, and any directory the walk could not
[list](README.md#unlistable-directories). The last two are the
workspace-wide [unreadable-input rule](README.md#unreadable-input) —
`check` is not special here, it is just where the rule matters most,
since a gate reporting clean on a tree it could not read is
indistinguishable from a gate that passed. All three run before the gate
is evaluated and are not suppressed by `--no-fail`, which suppresses
threshold failures, not broken input, so none of them lets
`--write-baseline` record a partial run.

### Tiered exit codes (`--exit-codes=tiered`) {#tiered-exit-codes---exit-codestiered}

`--exit-codes=tiered` (or `[check] exit_codes = "tiered"` in
`bca.toml`) splits the single violation code `2` by severity so CI can
branch on it without parsing the `[new]` / `[regr +N%]` row tags:

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
`--exit-codes <default|tiered>` is value-taking; the CLI value overrides
the `[check] exit_codes` manifest key in either direction. An invalid
`exit_codes` value is a tool error (`1`). `--print-effective-config`
reports the resolved `exit_codes` style. The deprecated
`--strict-exit-codes` flag is a one-cycle alias for `--exit-codes
tiered` (warns; removed at the next major).

## Declaring thresholds

Pass `--threshold <metric>=<limit>` once per metric (repeatable). Metric
names match `bca list-metrics`; sub-metrics use a dotted form. `0` is a
valid limit and means "no value permitted".

```bash
bca check --paths src/ \
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
bca check --paths src/ --config bca.toml
```

`bca init` scaffolds a starting table for you. For where those numbers
come from, which ones to override for the language you are gating, and
how to pick a different set for agent feedback, legacy triage, or
safety-critical work, see
[Choosing thresholds](../recipes/thresholds.md).

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

### Threshold scope {#threshold-scope}

A threshold is checked only against the space kind its metric actually
measures, so a metric's whole-file or whole-`impl` aggregate is never
mistaken for a per-function limit. Each metric has a fixed scope; there
is nothing to configure.

| Scope | Gated spaces | Metrics |
|-------|--------------|---------|
| File | the whole-file root only | `loc.sloc`, `loc.ploc`, `loc.lloc`, `loc.cloc`, `loc.blank` |
| Function | individual functions, methods, and closures | `cognitive`, `cyclomatic`, `cyclomatic.modified`, `halstead.*`, `mi.*`, `abc`, `nargs`, `nexits`, `tokens` |
| Container | classes, structs, traits, impls, namespaces, interfaces | `nom`, `wmc`, `npm`, `npa` |

The Function-scoped metrics include the subtree sums (`nargs`, `nexits`,
`tokens`, `halstead.*`): these still roll a function's own nested closures
into its figure, but they are no longer summed across an entire file or
`impl`. The Container-scoped metrics describe a type's method set (methods
per class, weighted methods, public members), so they gate the container
rather than every leaf function. This means a clean file whose functions
are individually fine no longer trips an additive limit purely from the
file-wide total — the false positive that `bca: suppress-file` markers
used to mask.

The bare `bca diff --metric` spelling of a `loc` sub-metric is accepted
as an alias for its dotted form (`sloc` is equivalent to `loc.sloc`, and
so on for `ploc`/`lloc`/`cloc`/`blank`), so a name copied from a `diff`
run gates correctly. A bare family head with no single threshold scalar
(`halstead`, `mi`) is ambiguous and rejected with a "did you mean" hint
listing the concrete sub-metrics — pick one (e.g. `halstead.volume`).

The two spellings name one metric, so they override each other wherever
limits merge: a `[thresholds.lang.c] ploc = 100` replaces the global
`"loc.ploc"`, and `--threshold ploc=100` replaces either. Within a single
table, writing both is an error rather than a silent winner — set the
metric once, under whichever spelling you prefer.

### Per-language limits (`[thresholds.lang.<slug>]`) {#per-language-limits}

Metric distributions vary by language more than by project — the
measured 97.5th-percentile per-function `cognitive` value runs from 4 in
C# to 50 in C. A `[thresholds.lang.<slug>]` table gives one language its
own limits, layered over the project-wide table:

```toml
[thresholds]
cognitive = 15
cyclomatic = 15
"loc.ploc" = 600

[thresholds.lang.c]
cognitive = 30
"loc.ploc" = 1200

[thresholds.lang.elixir]
nom = 150
wmc = 300
```

C is now gated at `cognitive = 30` and `loc.ploc = 1200` while keeping
the project's `cyclomatic = 15`; every other language keeps all three.
The override is **per metric**, not a replacement table — a language
inherits every limit it does not restate. Which numbers to change, and
the two cases where an override is a correction rather than tuning, are
in [Choosing thresholds](../recipes/thresholds.md#per-language).

The key is the canonical language slug, the same vocabulary `--language`
accepts: `rust`, `python`, `cpp`, `csharp`, `objc`, `tsx`, `mozcpp`,
`mozjs`, and so on — `bca check --language nonsense` prints the full
list. Two rules point in opposite directions here:

- An **unknown slug in the manifest** is a tool error (exit `1`) with a
  did-you-mean hint. A typo'd `[thresholds.lang.rust-lang]` must not
  silently leave a gate at the project limit while the author believes
  it was loosened.
- An **unrecognised file language** falls through to the global table,
  as does any language with no override of its own. Nothing is silently
  ungated. (A file whose extension maps to no grammar at all is skipped
  by the walk before the gate ever sees it — with a warning if you named
  it explicitly.)

`--threshold` on the command line stays global and still applies last
and absolutely: it overrides the project table *and* every per-language
one, so a limit you type is the limit that runs.

`--print-effective-config` prints one fully resolved table per
overridden language — inherited limits included, not a diff — so the
number that will actually fire is the number you read:

```toml
[thresholds]
cognitive = 15.0
cyclomatic = 15.0

[thresholds.lang.c]
cognitive = 30.0
cyclomatic = 15.0
```

## Two-tier thresholds (`--tier`)

`--tier <hard|soft|soft=RATIO>` selects which threshold tier the gate
compares against. `hard` (the default) uses the `[thresholds]` table
verbatim; `soft` is an early-warning tier that fires *before* the hard
gate, tightening every limit by `RATIO`. A bare `--tier`
means `soft`; `soft` alone uses the default ratio `0.95`; `soft=0.90`
pins the ratio to `0.90`; `soft=1.0` disables the blanket scale.

`RATIO` scales the *band*, not the number. For most metrics a limit is
a ceiling, so tightening it means multiplying: `cognitive = 15` with
`soft=0.9` warns at `13.5`. For the lower-is-worse `mi.*` family a
limit is a **floor** — a value below it is the violation — so the same
`0.9` *divides*: `"mi.original" = 20` warns at `20 / 0.9 = 22.2223`,
rounded up so the band never resolves below the exact quotient.
Multiplying a floor would move the warning to `18`, *under* the hard
gate, where nothing could reach it first.

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
bca check --paths src/ --tier=soft
```

The soft tier resolves in a fixed order:

1. Start from `[thresholds]` (a `bca.toml` manifest, merged with
   `--config`).
2. If a `[thresholds.soft]` table exists, merge its overrides on top;
   metrics absent from it inherit their hard limit. The blanket `RATIO`
   does not apply (explicit per-metric limits win).
3. Otherwise tighten every limit by the soft `RATIO` (default `0.95`
   for a bare `soft`; `soft=1.0` disables scaling).
4. Repeated `--threshold name=value` flags apply last, absolutely.

Steps 1 to 3 run **once per language**, against that language's own
resolved hard limits. There is no `[thresholds.lang.<slug>].soft` table
and none is needed: with `[thresholds] cognitive = 15`,
`[thresholds.lang.c] cognitive = 30`, and `--tier=soft=0.9`, C's soft
band is `27` — nine tenths of *its* limit, and the ceiling a C offender
is measured against for the exit-`5` escalation is `30`, not `15`.
Derive either from the project's `15` and every C function between 15
and 30 reports "also breaches the hard limit" while sitting inside the
limit the project configured for it.

The one combination that *can* invert the two tiers is an absolute
`[thresholds.soft]` value looser than the hard limit it shadows —
`[thresholds.soft] cognitive = 12` alongside
`[thresholds.lang.csharp] cognitive = 4`, or an `mi.*` soft floor
*below* its hard floor. That is a tool error (exit
`1`) naming the offending table, for the same reason a `"<ratio>x"`
factor above `1` is rejected at parse time: a soft tier that fires
*after* the hard gate is never the intent.

The soft `RATIO` (and the scale factor in a `"<ratio>x"` string) must
be in `(0, 1]`. The `[check] headroom` manifest key supplies the ratio
for a bare `--tier=soft`. The deprecated `--headroom <R>` flag is a
one-cycle alias for `--tier=soft=<R>` (warns; removed at the next
major) — it now promotes a hard run to the soft tier. Both tiers
ratchet through the same `--baseline`, and `--print-effective-config`
reports the resolved `tier` alongside the post-merge limits. See the
[Local threshold gates](../recipes/local-gates.md#two-tier-thresholds)
recipe for the migration tip and rationale.

## Previewing a candidate limit (`--explain-threshold`) {#explain-threshold}

`--explain-threshold <metric>=<limit>` reports what a candidate limit
would cost — at **both** tiers — instead of gating. It is repeatable,
takes one candidate per metric, and writes nothing.

```console
$ bca check --explain-threshold nargs=6
nargs: candidate limit 6
  hard tier (limit 6): 60 offenders, 60 already baselined, 0 new
  soft tier (limit 5.7, 0.95x): 135 offenders, 61 already baselined, 74 new
  cluster: 75 of 75 soft-band offenders sit at exactly 6 — the candidate
  limit itself. The soft tier measures distance to the limit, so a limit
  of 6 places them inside the 5.7 band by construction and none of them
  can clear it without real work.
```

The `new` column is the figure to weigh: it is how many baseline
entries adopting the limit would add. Here the hard tier reads as free
and the soft tier costs 74 entries, none of which any amount of tidying
can retire — which is the whole reason the flag exists.

`--threshold nargs=6` cannot tell you this. Its limits are applied last
and **absolutely, never scaled**, so a candidate trialled that way has
no soft tier at all. Passing both flags for the same metric is
rejected rather than silently resolved.

The soft limit is derived from the candidate exactly as a real run
would derive it: a `[thresholds.soft]` entry for the metric wins, then
the `--tier=soft=RATIO` ratio if one was given, then `0.95`. A
`[thresholds.lang.<slug>]` table that overrides the metric keeps its own
limit — a candidate *global* limit does not reach it — and the report
says so on its own line.

Everything else matches the run being predicted: `exclude_tests`,
`[check] exclude`, in-source suppression markers, `--changed-only`, and
the baseline all apply as usual. The one difference is that
baseline-covered offenders are counted rather than dropped, which is
what makes the `already baselined` / `new` split possible.

The preview replaces the gate, so it never fails: the exit code is `0`
unless a tool error (exit `1`) stops the run, and a one-line reminder of
that goes to stderr. It conflicts with `--write-baseline`,
`--print-effective-config`, `--report-format`, and `--output`, each of
which would produce a second, different artifact.

See [Choosing thresholds](../recipes/thresholds.md#converging-onto-a-cluster)
for the rule this flag exists to make visible.

## Offender output

Every offending `(function, metric)` pair prints one line to **stdout**
in this stable format:

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

### Which stream {#which-stream}

The offender rows are the command's product, so they go to **stdout**:
`bca check | wc -l`, `| head`, `| rg -c` and `2>/dev/null` all reach
them.

Everything the run says *about itself* goes to **stderr**:

- the per-file `--- summary ---` footer,
- the `--- next steps ---` remediation block,
- the GitHub Actions `::error` annotations,
- the `bca: skipped N violations via [check.exclude]` and
  `bca: filtered N violations via baseline` counts,
- every `warning:` and `error:` diagnostic.

One combination inverts this. `--report-format <dialect>` without
`--output` puts the aggregated SARIF / Checkstyle / Code Climate
document on stdout, so the human rows fall back to stderr rather than
corrupting it:

```bash
bca check --report-format sarif | jq '.runs[0].results | length'   # document on stdout
bca check --report-format sarif --output report.sarif | wc -l      # rows back on stdout
```

The `--summary-file` digest is a file, not a stream, and appears on
neither.

Earlier releases sent the rows to stderr along with everything else,
which made `| wc -l` and `2>/dev/null` report an empty offender list —
indistinguishable from a clean tree. A pipeline that reads the rows
through `2>&1` needs no change; one that captured them with `2>file`
should now use `>file`.

## Silencing violations with suppression markers

In-source comments can silence threshold violations on individual
functions or whole files without editing the offending code or
excluding it from the walk. The native dialect is `bca: suppress` /
`bca: suppress-file`; Lizard's `#lizard forgives` is recognized as a
compatibility shim. See [Suppression markers](suppression.md) for
the full reference and the `--no-suppress` CI-audit flag.

## Exempting whole file categories (`[check.exclude]`) {#exempting-whole-file-categories-checkexclude}

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
the walker matched it for `--exclude`. As a *negative filter* key, an
explicit `--check-exclude` list **unions with** (does not replace) the
manifest `[check] exclude` list — a CLI exemption is added to the
project's, never a replacement, so you cannot accidentally re-gate a
path the manifest deliberately exempted.
Duplicates collapse; CLI patterns sort first. Pass `--no-config` to drop
the manifest's exemptions entirely. (Positive scope keys like `paths` /
`include` still *replace* on the CLI — only the exclude filters merge.)

### What a relative glob is relative to {#what-a-relative-glob-is-relative-to}

The two sources anchor to different roots, and this is the part nobody
guesses right:

| Source | A relative glob is resolved against |
| --- | --- |
| `bca.toml` `[check] exclude` / `exclude_from` | the directory holding the `bca.toml` |
| `--check-exclude` / `--check-exclude-from` | the directory you ran `bca` from |

So `exclude = ["./vendor/**"]` in a manifest at the repo root means
`<repo>/vendor/`, whichever directory you invoke `bca check` from —
including from inside `vendor/` itself. The same glob passed as
`--check-exclude './vendor/**'` means `vendor/` *under your shell's
current directory*, because that is where you typed it.

This matters most for per-file callers — editor integrations, pre-commit
hooks, the agent hooks in [Agent feedback](../recipes/agent-feedback.md)
— which run `bca check <one file>` from whatever directory they happen
to be in. Their exemptions belong in the manifest, where the anchor is
the project's and not the caller's.

The same split applies to the walker's `exclude` / `--exclude` pair.

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
bca check --paths src/ \
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
version = 6

[provenance]
tier = "hard"

[[entry]]
path = "src/parser.rs"
qualified = "Parser::parse_expression"
metric = "cyclomatic"
value = 22.0
```

The `qualified` field is the function's qualified symbol (the
`::`-joined chain of enclosing named containers plus the function
name). An entry carries a `start_line` only when its
`(path, qualified, metric)` identity is shared with another entry, the
one case matching consults a line number; recording it elsewhere would
only rewrite the file every time an edit above the function shifted it.
With `--baseline-fuzzy-match`, each entry also carries a `body_hash`
for rename-tolerant matching.

Functions already covered by an in-source suppression marker are
excluded. Pass `--no-suppress` together with `--write-baseline` to
record every violation (CI-auditor flow).

`--write-baseline` cannot be combined with `--baseline`,
`--report-format`, `--output`, `--since`, or `--changed-only` — the
baseline file *is* the output.

### Reading a baseline

```bash
bca check --paths src/ \
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

`--no-fail` reports offenders as usual but exits `0`. Useful while
adopting baselines without flipping CI red. Other CI tools call this
behavior `--report-only` or `--soft-fail`; here the flag is spelled
`--no-fail`.

```bash
bca check --paths src/ --no-fail
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
| `--github-annotations <auto\|always\|never>` | Emit `::error file=…::msg` workflow commands for inline file annotations (bare flag = `always`) | `auto` detects `GITHUB_ACTIONS == "true"` |
| `--summary-file <path\|auto\|never>` | Append markdown digest (per-file rollup + breakdown + top-10 offenders); `never` suppresses it | `auto` detects `GITHUB_STEP_SUMMARY` |
| `--no-remediation`         | Suppress the trailing `--- next steps ---` block                         | Block emitted on failure unless this flag is passed              |

The per-violation rows and the per-file rollup footer remain
unchanged in *content* when none of the above are active, so CI
tooling that grep-anchors on the legacy text keeps working — but
see [Which stream](#which-stream) for where each now lands.

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

## CI example (GitHub Actions) {#ci-example-github-actions}

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
    bca check --paths src/ --no-fail
```

## Exporting offender records {#exporting-offender-records}

`bca check` also emits a single CI/IDE document covering every
offender in the walk. Pass `--report-format <fmt>` to pick the
shape and `--output <file>` to write it to disk (stdout if omitted).
The `--format`, `-O`, and `--output-format` spellings are accepted as
deprecated aliases and will be removed in a future release. The
exit-code contract is unaffected by these flags: 0 clean, 2 on any
violation (unless `--no-fail`), 1 on tool error.

When `--output` is given without `--report-format`, the format is
**inferred from the output extension**: `.sarif` selects `sarif` and
`.xml` selects `checkstyle`. An extension with no unique format
(notably `.json`, which both `sarif` and `code-climate` produce) or no
extension at all is a usage error (exit `1`) naming `--report-format` —
an explicit `--output` is never silently ignored. An explicit
`--report-format` always wins over the extension.

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

### Checkstyle (CI integration) {#checkstyle-ci-integration}

```bash
bca check --paths src/ \
    --threshold cyclomatic=15 \
    --report-format checkstyle \
    --output report.checkstyle.xml
```

The Checkstyle writer emits a single `<checkstyle version="4.3">`
document containing one `<file>` element per source path, each
holding one `<error>` per metric-threshold violation. The schema is
the Checkstyle 4.3 XSD that Jenkins and SonarQube's "Warnings Next
Generation" / "Generic Issue" importers consume directly.

### SARIF (GitHub Code Scanning)

```bash
bca check --paths src/ \
    --threshold cyclomatic=15 \
    --report-format sarif \
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
          bca check --paths . \
              --report-format sarif \
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
bca check --paths src/ \
    --threshold cyclomatic=15 \
    --report-format code-climate \
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
    - bca check --paths "$CI_PROJECT_DIR"
          --report-format code-climate
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
bca check --paths src/ \
    --threshold cyclomatic=15 \
    --report-format clang-warning \
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
          bca check --paths . \
              --report-format clang-warning \
              --no-fail
```

If your runner does not ship a GCC matcher, fall back to streaming
the lines and re-emitting them as `::warning file=...,line=...::`
workflow commands.

### MSVC warning lines (Visual Studio and Windows CI)

```bash
bca check --paths src/ \
    --threshold cyclomatic=15 \
    --report-format msvc-warning \
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
