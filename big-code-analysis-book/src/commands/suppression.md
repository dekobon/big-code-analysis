# Suppression markers

In-source suppression markers silence threshold violations without
editing the offending function or excluding the file from the walk.
Drop a marker in any comment in the source file and `bca check`
treats the covered metrics as if they were within limits for that
scope. Metric computation is unaffected — raw `bca metrics` output
still reports every number. Suppression is a measurement-display
concern: `bca check` drops the covered violations from the gate, and
`bca report markdown|html` omits the covered functions from the
matching hotspot tables by default (pass `bca report --no-suppress`
for the raw audit view — see [report](report.md)).

Markers exist for the cases editing the code is not an option:
generated-style legacy modules awaiting rewrite, accepted exceptions
documented in the comment, and migration from
[Lizard](https://github.com/terryyin/lizard)'s `#lizard forgives`
convention.

## Native markers (`bca:`)

The native dialect uses the `bca:` namespace and the `suppress` verb,
matching the project's internal "suppression" vocabulary
(`SuppressionPolicy`, `FuncSpace::suppressed`, `--no-suppress`). Four
forms:

| Marker                              | Scope             | Effect                          |
| ----------------------------------- | ----------------- | ------------------------------- |
| `bca: suppress`                     | Enclosing function | Suppress every metric           |
| `bca: suppress(metric, ...)`        | Enclosing function | Suppress only the listed metrics |
| `bca: suppress-file`                | File              | Suppress every metric           |
| `bca: suppress-file(metric, ...)`   | File              | Suppress only the listed metrics |

Any of the four may carry a **rationale** — free text on the same line,
after the marker itself:

```rust
// bca: suppress(nargs) — threaded context, not a god-function
```

After a metric list no separator is required and none is privileged:
`— why`, `- why`, `: why`, `// why`, and bare prose all read the same.
After a *bare* verb, with no metric list, the rationale must open with
one of `-`, `:`, `//`, `#`, or an em/en dash — see
[Rationale text](#rationale-text) for why.

A function-scope marker attaches to the innermost `FuncSpace`
(see the [`FuncSpace` rustdoc](https://docs.rs/big-code-analysis/*/big_code_analysis/spaces/struct.FuncSpace.html))
whose source range contains the comment.
A function-scope marker outside every function body is silently
ignored; for file-wide silencing use the explicit `suppress-file` verb.
A file-scope marker may appear anywhere in the source — there is no
"must be in first N lines" rule.

### `bca: suppress` — function-scoped, all metrics (Rust)

```rust
// bca: suppress
fn legacy_dispatch(opcode: u8) -> Action {
    // dense match on every supported opcode
    match opcode { /* ... */ }
}
```

### `bca: suppress(metric, ...)` — function-scoped, listed metrics (Python)

```python
def parse_token_stream(tokens):
    # bca: suppress(cognitive)
    # cognitive complexity is intrinsic to this state machine;
    # cyclomatic is still bounded.
    ...
```

Other thresholds (cyclomatic, halstead, loc, ...) still apply.

### `bca: suppress-file` — file-scoped, all metrics (JavaScript)

```javascript
// bca: suppress-file
// Hand-tuned hot path; do not rewrite to satisfy thresholds.
function transform(input) { /* ... */ }
function validate(input) { /* ... */ }
```

### `bca: suppress-file(metric, ...)` — file-scoped, listed metrics (C++)

```cpp
/* bca: suppress-file(halstead) */
// Halstead volume is inflated by the generated tables below; every
// other metric is still enforced file-wide.
```

> **Prefer a narrower tool first.** Since [threshold scope](./check.md#threshold-scope)
> (#969), a metric's file-wide or `impl`-wide *aggregate* no longer fires
> as a per-function limit, so the most common reason `suppress-file` was
> reached for — muting a file-level `halstead` / `nargs` / `nexits` /
> `nom` total — is gone. Reach for `suppress-file` only when you genuinely
> want to silence a metric for *every* function in the file. To excuse one
> irreducibly-complex function, use a function-scoped `bca: suppress(...)`
> inside it; to grandfather existing offenders without blinding the gate
> to future regressions, prefer a [baseline](./check.md) entry, which keeps
> firing once a function gets *worse* than its recorded value.

## Lizard compatibility markers

Two Lizard-style markers are recognized verbatim so existing
Lizard-instrumented codebases need no rewrites:

| Lizard marker             | Scope             | Equivalent native marker |
| ------------------------- | ----------------- | ------------------------ |
| `#lizard forgives`        | Enclosing function | `bca: suppress`          |
| `#lizard forgive global`  | File              | `bca: suppress-file`     |

The compatibility layer is intentionally narrow: only these two
shapes are accepted. Other Lizard directives parse as ordinary
comments. Lizard offers no per-metric scoping, so the native form's
`bca: suppress(metric, ...)` list has no Lizard analogue — every
Lizard-style marker silences every metric.

Lizard's `GENERATED CODE` marker is **not** handled here; it is part
of the generated-code auto-skip mechanism (see
[Skipping generated code](index.html#skipping-generated-code) and the
`--no-skip-generated` flag).

### Native vs Lizard side by side

| Effect                                  | Native form                       | Lizard form              |
| --------------------------------------- | --------------------------------- | ------------------------ |
| Silence every metric for one function   | `// bca: suppress`                | `// #lizard forgives`    |
| Silence one metric for one function     | `// bca: suppress(cyclomatic)`    | (no equivalent)          |
| Silence every metric for the whole file | `// bca: suppress-file`           | `// #lizard forgive global` |
| Silence one metric for the whole file   | `// bca: suppress-file(halstead)` | (no equivalent)          |

## Metric identifiers

The identifiers accepted inside `bca: suppress(...)` and
`bca: suppress-file(...)` are:

`abc`, `cognitive`, `cyclomatic`, `halstead`, `loc`, `mi`, `nargs`,
`nexits`, `nom`, `npa`, `npm`, `wmc`.

These match the threshold names and the JSON field names emitted on
`CodeMetrics`, with one deliberate exclusion:

- `nexits` is the canonical spelling — `bca: suppress(nexits)` silences
  a `nexits` threshold violation. The legacy `exit` alias was retired
  in #555 and is no longer accepted; spelling it `exit` is an unknown
  identifier, which warns and is skipped — the recognized names beside
  it still suppress (see below).
- `tokens` is a threshold-checkable metric (and a `CodeMetrics` JSON
  field) but is deliberately absent from the suppression list: a
  marker cannot turn it off. Treat `tokens` as a hard resource cap,
  not a maintainability heuristic.

Silencing a family (for example `halstead`) covers every sub-metric
threshold under it (`halstead.volume`, `halstead.effort`, ...);
suppression vocabulary has no dotted form.

Unknown identifiers in a `bca: suppress(...)` list emit a stderr warning
of the form

```text
warning: path/to/file.rs:42: unknown metric 'no_such_metric' in bca suppression marker; known metrics: abc, cognitive, ...
```

Only the offending identifier is dropped: `bca: suppress(cognitive,
exit)` still silences `cognitive`. Skipping can only ever shrink what a
marker covers, so a typo never widens scope to a metric the author did
not name — while voiding the whole marker, which is what earlier
releases did, produced the opposite hazard: a suppression its author
believed was active and the gate did not.

Unknown verbs (anything other than `suppress` / `suppress-file`) and
bodies that parse to no directive at all (an unbalanced parenthesis, a
bare verb followed by words that open no rationale) produce the same
shape of warning, and those *do* void the marker — there is nothing left
of it to honour. None of these are fatal: a typo in one file does not
derail a workspace walk, and a doc comment that merely mentions the
syntax does not fail your gate.

### Rationale text {#rationale-text}

Text after the marker is yours; `bca` parses past it and does not
interpret it. The asymmetry between the two forms is deliberate:

- **After a metric list** — `bca: suppress(nargs) threaded context` —
  anything goes. The list has already stated the intent unambiguously,
  so whatever follows is prose.
- **After a bare verb** — `bca: suppress — irreducible dispatch` — a
  separator (`-`, `:`, `//`, `#`, an em/en dash) is required. Without a
  list there is nothing to distinguish a rationale from a sentence
  *about* the feature, and reading `// bca: suppress markers are
  honoured here` as a marker would silence every metric in the enclosing
  function on the strength of a comment. That shape warns instead.

## Where markers may appear

A marker is recognized inside any source comment, regardless of
comment style. The scanner strips the following leading delimiter
characters before matching: `/`, `*`, `!`, `#`, `;`, `-`, and ASCII
whitespace. That covers every comment shape `bca` parses today:

- C-family line comments: `// bca: suppress`
- C-family block comments: `/* bca: suppress */`
- Rust inner doc comments: `//! bca: suppress` and `/*! bca: suppress */`
- Python / shell / Ruby / Perl `#` comments: `# bca: suppress`
- Lisp / Lua / SQL line comments: `;; bca: suppress`, `-- bca: suppress`

Function-scope markers attach to the innermost `Function`-kind
`FuncSpace` whose `(start_line..=end_line)` range contains the
comment's line. Markers buried in a class or struct body but outside
every method are silently ignored — for class-wide silencing use
`bca: suppress-file` or repeat the marker on each method.

File-scope markers are merged into the top-level `Unit` space and
apply to every function in the file regardless of nesting.

Position the marker near the start of the comment. The scanner trims
delimiter characters from both ends and then expects `bca:` (or
`#lizard`) at the very front; markers buried deep in a multi-line
block comment will not be recognized.

## `--no-suppress` (CI auditing)

`bca check --no-suppress` ignores every suppression marker — native
and Lizard alike — and reports every threshold violation in the
walk. Use it in audit pipelines that need the raw, un-silenced
offender list:

```bash
bca check --paths src/ --no-suppress
```

The flag has no effect on metric values themselves: raw `bca metrics`
output always reports every number. `bca report markdown|html` honours
markers in its hotspot tables by default and accepts its own
[`--no-suppress`](report.md) flag for the same raw audit view.

## Surfacing suppressed debt (`--report-suppressed`)

Suppression keeps an offender out of the gate, which also keeps it out of
the `--format` document — so a suppressed module disappears from the
code-scan report entirely. `bca check --report-suppressed` puts it back, as
*suppressed* rather than active:

```bash
bca check --report-format sarif --no-fail --report-suppressed \
    --tier=soft=0.95 --output bca.sarif
```

Offenders silenced by an in-source marker or covered by the baseline are
emitted into the SARIF document with a SARIF `suppressions` entry —
`kind: "inSource"` for markers, `kind: "external"` for the baseline. The
suppression never fails the gate (exit code and the human offender rows
are unaffected); the `suppressions` entry lets downstream tooling tell
suppressed debt apart from active offenders.

> **GitHub Code Scanning caveat.** GitHub does **not** honor the SARIF
> `suppressions` property natively — it ingests suppressed results as
> *open* alerts, not closed ones. To dismiss them on the Security tab you
> need a follow-up step such as the
> [`advanced-security/dismiss-alerts`](https://github.com/advanced-security/dismiss-alerts)
> action, which reads `suppressions[]` and dismisses the matching alerts.
> If you only want active offenders to appear, omit `--report-suppressed`
> from the upload (this repo's own Pages workflow does exactly that).

Notes:

- Only the SARIF format represents suppression; other `--format`
  values ignore the flag and emit the active offenders alone.
- Pair it with `--tier=soft=0.95` (matching your baseline's
  provenance) so baseline-covered offenders that sit below the hard limit
  still appear.
- Mutually exclusive with `--no-suppress` (which un-silences markers to
  show the raw offender list) and `--write-baseline`.

## Auditing exemptions (`bca exemptions`) {#auditing-exemptions-bca-exemptions}

`--no-suppress` shows you the offenders a marker *silences*, but not
the markers themselves — to find every silencer you previously had to
diff a `--no-suppress` run against a normal one. `bca exemptions`
replaces that workaround with a direct listing of everything the
`bca check` gate skips, across all three exemption tiers, in one
report:

| Tier | Granularity | Source |
| --- | --- | --- |
| In-source markers | per-function / per-file | `bca: suppress`, `#lizard forgives`, … |
| `[check.exclude]` globs | per-glob (categories of files) | `bca.toml` `[check] exclude` / `--check-exclude` |
| Baseline entries | per-`(path, symbol, metric)` | `.bca-baseline.toml` |

```bash
# List every exemption in the tree (in-source markers honour
# [walker.exclude] just like every other walking command).
bca exemptions --paths src/
```

```text
# In-source markers (2)
  src/parser.rs:120  bca: suppress       metrics=all  parse_long
  src/lib.rs:1       bca: suppress-file  metrics=halstead  (whole file)

# [check.exclude] globs (1)
  tests/**

# Baseline (.bca-baseline.toml, 1 entry)
  src/markdown_report.rs:88 write_language_section cognitive 29
```

The surrounding function (for function-scoped markers) gives scope
context; file-scoped markers read `(whole file)`, and a function-scoped
marker written outside any function — which silences nothing — reads
`(no enclosing fn)` so dead markers are visible.

### Formats and section filters

`--format markdown` emits tables for PR comments; `--format json`
nests all three tiers under a single `suppressions` envelope for
dashboards and `jq` filtering:

```bash
bca exemptions --paths src/ --format json | jq '.suppressions.markers[] | select(.dialect == "lizard")'
```

In the JSON form an omitted section is `null` (not requested via a
`--*-only` flag) while a requested-but-empty section is `[]`, so
filters can tell the two apart.

The mutually-exclusive `--markers-only` / `--excludes-only` /
`--baseline-only` flags narrow the report to a single tier for PR-bot
specialisation (e.g. a bot that only comments on newly-added in-source
markers). The baseline (`bca.toml` top-level `baseline`) and
`[check.exclude]` (`[check] exclude`) inputs default to the same
sources `bca check` reads, so the audit reflects exactly what the gate
would skip; override the baseline with `--baseline <path>`.

The earlier `--only-markers` / `--only-excludes` / `--only-baseline`
spellings remain as hidden aliases for one release cycle to keep
existing PR-bot invocations working; prefer the `--<section>-only`
forms, which match the `diff-baseline` section filters.

Unlike `bca check`, `bca exemptions` is informational and always exits
0 on success — it is a review surface, not a gate.

See also the [Baselines recipe](../recipes/baselines.md) for using
`bca exemptions` alongside `bca diff-baseline` during PR review.

## JSON output

`FuncSpace` exposes the merged suppression scope as the optional
`suppressed` field in its JSON output. When no marker applies to a
space the field is elided so existing snapshot consumers see no
change. When a marker fires the field carries one of two shapes:

```json
{ "suppressed": { "kind": "all" } }
```

```json
{ "suppressed": { "kind": "some", "metrics": ["cognitive", "loc"] } }
```

`kind: all` corresponds to a bare marker (`bca: suppress`,
`bca: suppress-file`, or any Lizard-style marker). `kind: some` carries
the explicit metric list from `bca: suppress(...)` /
`bca: suppress-file(...)`. Both shapes are stable serialization output
suitable for dashboards and audit logs.

## Migrating from Lizard

The compatibility layer means migration is incremental:

1. Existing `#lizard forgives` and `#lizard forgive global` markers
   continue to work with no change. `bca check` honors them out of
   the box.
2. Rewrite to the native form opportunistically. `bca: suppress(...)`
   gives per-metric scoping (the Lizard form silences everything) and
   is the form future audit-trail features will extend.

The project will keep the Lizard compatibility layer indefinitely;
there is no removal date.

## Reserved syntax

These shapes are reserved for future use and are **not** parsed
today:

- `bca: suppress-next` — silence the immediately following declaration
  rather than the enclosing function. Rejected today as an unknown verb;
  it will be promoted to first-class behavior in a future release
  without breaking existing markers.

The `reason = "..."` argument this section used to reserve is no longer
planned. Write the rationale after the marker instead — see
[Rationale text](#rationale-text) — which is the spelling authors
already reach for. Inside the parentheses, `reason = "..."` is still an
unknown metric identifier and is skipped with a stderr warning.
