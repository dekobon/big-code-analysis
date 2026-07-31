# Choosing thresholds {#choosing-thresholds}

`bca check` compares each metric against a limit you configure. This page explains where the
shipped defaults come from, how to adjust them for the language you are gating, and how to pick a
different set for a different job. It is for anyone editing a `[thresholds]` table, whether by hand
or through a coding agent.

If you have not set up a gate yet, start with [Local threshold gates](./local-gates.md) for the
mechanics and [Baselines](./baselines.md) for absorbing the offenders you already have. This page is
about the numbers.

## The shipped defaults {#shipped-defaults}

`bca init` scaffolds this table. It is defined once in
`big-code-analysis-cli/src/default_thresholds.rs`.

| Metric | Limit | Scope | Anchor |
| --- | --- | --- | --- |
| `cognitive` | 15 | function | [SonarSource](https://www.sonarsource.com/docs/CognitiveComplexity.pdf) default |
| `cyclomatic` | 15 | function | [lizard](https://github.com/terryyin/lizard) default; MISRA and NASA safety-critical ceiling |
| `abc` | 40 | function | between [RuboCop](https://docs.rubocop.org/rubocop/cops_metrics.html) `AbcSize` 17 and Flog's "60 and above is dangerous" |
| `nargs` | 5 | function | RuboCop `ParameterLists` 5; [Code Climate](https://docs.codeclimate.com/docs/default-analysis-configuration) is stricter at 4 |
| `nexits` | 5 | function | Code Climate `return-statements` 4 |
| `halstead.effort` | 50000 | function | none published; percentile-derived |
| `loc.ploc` | 600 | file | none published; percentile-derived |
| `loc.sloc` | 1200 | file | bloat backstop, not the working limit |
| `nom` | 30 | container | Code Climate `method-count` 20; [PMD](https://docs.pmd-code.org/latest/pmd_rules_java_design.html) `TooManyMethods` 10 |
| `wmc` | 60 | container | none published; percentile-derived |

Scope matters when you read these. A `cognitive` limit applies to each function; `nom` and `wmc`
apply to each class, struct, trait, impl, namespace, or interface; `loc.*` applies to the whole-file
root. `bca check` will not compare a container's method count against a per-function limit. See
[Check](../commands/check.md) for the full scope rules.

Two of these limits deserve their reasoning spelled out.

`loc.ploc` and `loc.sloc` are a pair. PLOC counts physical lines of code with blanks and comments
excluded, so it is the working file-size limit: growing a file by documenting a decision costs
nothing against it. SLOC counts everything, so it sits far looser and does one job only, which is
stopping a file from growing without bound on comment volume while still clearing `loc.ploc`. Gating
file size on SLOC alone charges the same price for a paragraph of rationale as for a new branch, and
the observable result is that people delete the rationale.

`cyclomatic` at 15 rather than McCabe's 10 is a deliberate loosening.
[NIST 500-235](https://nvlpubs.nist.gov/nistpubs/Legacy/SP/nistspecialpublication500-235.pdf), the
document that made 10 the canonical number, allows raising it for teams with the process to justify
it, and observes that limits as high as 15 have been used successfully. Measured against real code,
10 flags roughly twice as many functions as 15 without a corresponding change in what a reviewer
would call a problem.

## How the defaults were derived {#derivation}

Published thresholds disagree with each other by wide margins. For cyclomatic complexity alone the
defaults in common tools span 7 (RuboCop) to 30 (`gocyclo`), with Checkstyle and PMD at 10, lizard
and MISRA at 15, and ESLint at 20. Picking one by authority means picking an authority.

So each limit here is checked against measurement as well. The reference corpus is 43 real-world
repositories across 20 languages, cloned at their default branch on 2026-07-31, with test trees,
vendored code, and generated files excluded. Measured with `bca metrics`, that is roughly a quarter
of a million function spaces, forty thousand container spaces, and twenty-seven thousand files.
Values are binned by the same File, Function, and Container scope `bca check` applies, so a
container's `nom` never lands in a per-function distribution.

The design rule is that a default should flag roughly the worst 1% to 3% of spaces in the median
language. Below that a limit is inert and gives false comfort. Above it the gate stops being a gate
and becomes a style rule that people route around. This is a stricter reading of the benchmark
approach in
[Alves, Ypma, and Visser](https://webarchive.di.uminho.pt/wiki.di.uminho.pt/twiki/pub/Personal/Joost/PublicationList/AlvesYpmaVisserICSM2010.pdf),
who derive risk bands at the 70th, 80th, and 90th percentiles. Their 90th percentile is a useful
"worth looking at" line; it is far too noisy to fail a build on.

Two of the previous defaults failed that rule and changed as a result. `nargs` at 7 fired on under
1% of functions in the median language and on nothing at all in this project's own source, which
makes it a limit that cannot catch anything. `cognitive` at 25 was inherited from
[clippy](https://rust-lang.github.io/rust-clippy/master/#cognitive_complexity), which is
deliberately conservative because it lints rather than gates; 15 is what the metric's designers
chose.

## Per-language overrides {#per-language}

Metric distributions vary by language more than by project. The 90th-percentile per-function
`cognitive` value in the reference corpus runs from 0 in C# to 21 in Tcl. Median file `loc.ploc`
runs from 16 in TypeScript to 283 in C, an eighteen-fold spread. A single table cannot fit both
ends.

The table below gives the measured 97.5th percentile per language, which is the value a limit would
need to flag the worst 2.5% of that language's code. Read it as calibration data, not as a
prescription: a low number means the language's code is mostly simple, which is a reason you *can*
tighten, not evidence that you should.

| Language | `cognitive` | `cyclomatic` | `abc` | `halstead.effort` | `loc.ploc` |
| --- | --- | --- | --- | --- | --- |
| Bash | 35 | 25 | 60 | 95000 | (thin sample) |
| C | 50 | 30 | 60 | 200000 | 3500 |
| C++ | 14 | 10 | 25 | 50000 | (thin sample) |
| C# | 4 | 4 | 13 | 9500 | 700 |
| Elixir | 5 | 7 | 20 | 15000 | 1500 |
| Go | 25 | 17 | 45 | 120000 | 1500 |
| Groovy | 16 | 13 | 25 | 30000 | 450 |
| Java | 7 | 5 | 16 | 15000 | 800 |
| JavaScript | 20 | 15 | 35 | 55000 | (thin sample) |
| Kotlin | 10 | 8 | 20 | 20000 | 300 |
| Lua | 35 | 20 | 40 | 85000 | 800 |
| Objective-C | 18 | 12 | 35 | 60000 | (thin sample) |
| Perl | 35 | 25 | 45 | 140000 | 1500 |
| PHP | 10 | 9 | 25 | 50000 | 900 |
| Python | 20 | 13 | 25 | 25000 | 700 |
| Ruby | 7 | 6 | 19 | 10000 | 400 |
| Rust | 8 | 8 | 20 | 35000 | 900 |
| Tcl | 45 | 25 | 85 | 90000 | (thin sample) |
| TypeScript | 20 | 13 | 25 | 55000 | 450 |
| TSX | 14 | 10 | 65 | 95000 | 400 |

"Thin sample" marks a language whose file count in the corpus is too small to derive a file-size
figure from. The per-function figures for Bash, Tcl, JavaScript, and C++ rest on the smallest
samples in the table and are the ones most worth re-deriving against your own code.

### What to change, and why {#per-language-changes}

Most languages need no override. The cases that do fall into three groups.

**Procedural languages with large dispatch functions.** C, Tcl, Bash, Lua, Perl, and Go all run two
to three times the defaults. These languages lack exceptions or discourage them, so error handling
is inline branching, and they favour long `switch`-style dispatch over polymorphism. The default
`cognitive` limit flags 5% to 15% of their functions, against 3% in the median language. Raise
`cognitive` and `halstead.effort` first; those two carry most of the excess.

```toml
# Gating a C, Tcl, Bash, Lua, Perl, or Go codebase.
[thresholds]
cognitive = 30
cyclomatic = 20
abc = 50
"halstead.effort" = 120000
"loc.ploc" = 1200
"loc.sloc" = 2000
```

**Languages whose module construct is not a class.** `nom` and `wmc` are Container-scoped, and a
container is whatever the grammar calls a class, struct, trait, impl, namespace, or interface. An
Elixir `defmodule` holds dozens of functions by design, so roughly a third of Elixir modules breach
`nom = 30`. That is a scope mismatch, not a code smell. Rust sits at the other end: an `impl` block
is usually a handful of methods, so the default never fires.

```toml
# Elixir.
[thresholds]
nom = 150
wmc = 300

# Rust.
[thresholds]
nom = 20
wmc = 40
```

**Compact, heavily-abstracted languages.** Java, Ruby, Kotlin, C#, and Elixir sit far under the
defaults on every per-function metric. Their 97.5th percentile `cyclomatic` is 4 to 8. Gating them
at 15 means the gate will effectively never fire, so tighten if you want it to do work.

Read the C# row with care. Its distribution is dominated by property accessors, which `bca` counts
as functions and which are almost always trivial, so its percentiles sit lower than the code a
reviewer would actually be looking at. The same effect applies more weakly to Java, Kotlin, and
Ruby. Prefer re-deriving from your own repository over adopting any of those four rows directly; see
[Re-deriving for your own codebase](#re-deriving).

### Metrics that do not apply to every language {#language-gaps}

`nargs` reports 0 for every Bash, Perl, and Elixir function. Bash and Perl have no formal parameter
lists, so 0 is correct and the limit is simply inert. Elixir does have them and the zero is a gap,
tracked in [issue #1142](https://github.com/dekobon/big-code-analysis/issues/1142). In all three
cases a `nargs` limit passes unconditionally, which reads as "no offenders" rather than "not
measured".

`wmc`, `npm`, and `npa` are only produced for languages with a class-like container. They are absent
from Bash, C, Go, Lua, Perl, and Tcl output.

### Applying overrides today {#applying-overrides}

A single `bca.toml` carries one `[thresholds]` table for the whole project. Per-language tables in
the manifest are proposed in
[issue #1141](https://github.com/dekobon/big-code-analysis/issues/1141) and are not implemented yet.
Until they are, a polyglot repository has two options.

Run `bca check` once per language, selecting files by glob and passing that language's limits on the
command line:

```bash
bca check --no-config -I '*.rs' \
  --threshold cognitive=15 --threshold cyclomatic=15 --threshold nargs=5
bca check --no-config -I '*.c' \
  --threshold cognitive=30 --threshold cyclomatic=20 --threshold nargs=5
```

Note that `.h` is analyzed with the C++ grammar, not the C one, so a C project's headers land in
whichever glob covers `*.h`. See [Supported Languages](../languages.md) for the extension map.

Or keep one table sized for the loosest language and let per-file baselines carry the rest. That is
simpler to maintain and strictly weaker: the stricter languages stop being gated.

## Per-use-case profiles {#profiles}

The same codebase wants different limits depending on what the gate is for.

### Blocking CI gate {#profile-ci}

This is what the shipped defaults are for. A limit that fails a pull request has to be one the team
agrees is a real problem, because the cost of a false positive is a blocked merge and an argument.
Pair it with a baseline so the gate fails only on new offenders and regressions, and tighten from
there.

Use the shipped defaults unchanged, plus:

```toml
[check]
baseline = ".bca-baseline.toml"
```

### Agent feedback loop {#profile-agent}

When the consumer is a coding agent rather than a human reviewer, a false positive costs one wasted
refactor rather than a blocked merge, and the signal arrives while the code is still being written.
Tighten toward the published per-function values, and drop the file-size and container limits, which
an agent editing one function cannot act on.

```toml
[thresholds]
cognitive = 10
cyclomatic = 10
abc = 25
nargs = 4
nexits = 4
"halstead.effort" = 25000
```

See [Feeding metrics to an agent](./agent-feedback.md) for wiring this into an edit loop, and
[Suppression markers](../commands/suppression.md) for telling the agent when complexity is
essential rather than accidental.

### Legacy audit and triage {#profile-legacy}

When the goal is to rank an unfamiliar codebase rather than to gate it, you want the handful of
functions that are genuinely worst, not a list of thousands. Set limits at roughly twice the
defaults so only extreme outliers surface, run without a baseline, and use
`bca report markdown` rather than `bca check`.

```toml
[thresholds]
cognitive = 40
cyclomatic = 30
abc = 80
"halstead.effort" = 250000
"loc.ploc" = 1500
nom = 60
wmc = 120
```

[Quality reports](./quality-reports.md) covers the report output. Adding `--vcs` ranks by change
history as well, which is usually a better triage order than complexity alone: a complex function
nobody has touched in three years is not where the bugs are.

### Safety-critical and regulated {#profile-safety}

Standards in this space specify limits directly, and the standard wins over any measurement. MISRA
and NASA both cap cyclomatic complexity at 15; the
[JSF C++ standard](https://www.abxsoft.com/jsf/JSF_AV_C++_Coding_Standards_Rev_C.htm) allows 20 with
a documented exception for large `switch` statements. McCabe's original 10 applies where the
testing budget supports it, since the number is a bound on the basis-path test count, not a style
opinion.

```toml
[thresholds]
cognitive = 15
cyclomatic = 10
abc = 30
nargs = 5
nexits = 1
"halstead.effort" = 25000
```

`nexits = 1` encodes the single-exit rule from MISRA C:2023 Rule 15.5. It is contentious outside
regulated work, and in most codebases early returns make code simpler rather than harder to follow,
so treat it as a compliance setting rather than a general recommendation.

## Metrics not gated by default {#not-gated}

These are computed and visible in `bca report markdown|html`. They are left out of the default
table on purpose.

`halstead.volume` has the most widely-cited threshold of any Halstead measure, the guideline that a
function's volume should stay under 1000. Measured against the corpus it flags about 7% of functions
in the median language and 20% in the worst, which makes it useful for ranking and unusable as a
gate.

`mi.original`, `mi.sei`, and `mi.visual_studio` are the Maintainability Index family, and they are
lower-is-worse: the violation is a value below the limit. Visual Studio's bands (below 10 poor, 10
to 19 moderate, 20 and above good) apply to its own rescaled 0 to 100 output, which `bca` reports as
`mi.visual_studio`. `mi.original` is unbounded above and reaches 167 on the corpus, so the original
[SEI](https://insights.sei.cmu.edu/) bands of 65 and 85 do not transfer. The index is also a
function of the metrics you are already gating, so gating it too double-counts. See
[Supported Code Metrics](../metrics.md) for the formulas.

`npm`, `npa`, `tokens`, and `cyclomatic.modified` are omitted because each duplicates something
already in the table, or because their distributions are dominated by a language idiom rather than
by design quality.

## Re-deriving for your own codebase {#re-deriving}

Corpus percentiles are a starting point. Your own distribution is better evidence, and producing it
takes one command plus a short script.

```bash
bca metrics --no-config -O json -I '*.rs' -X '**/tests/**' . > metrics.jsonl
```

Each line is one file's `FuncSpace` tree. Walk it, keep each space's own value for the metric you
care about, filter by `kind` to match the metric's scope (`"function"`, `"unit"` for `loc.*`, or a
container kind for `nom` and `wmc`), and take the 97.5th percentile. Setting a limit there flags
your worst 2.5%.

Two checks are worth running afterwards. Count the offenders the limit produces: if it is more than
a few percent of your spaces, the gate will be ignored rather than obeyed. Then look at where the
values pile up. A natural distribution tails off smoothly, so a cluster of files sitting just under
the limit means the limit is shaping the code rather than measuring it, and people are trimming to
fit. That happened to this project's own `loc.sloc` gate, which is documented in
[issue #1138](https://github.com/dekobon/big-code-analysis/issues/1138).

`bca check --write-baseline` records today's offenders so you can adopt a stricter limit without
failing on day one. [Baselines](./baselines.md) covers the bootstrap, refresh, and retire flow.
