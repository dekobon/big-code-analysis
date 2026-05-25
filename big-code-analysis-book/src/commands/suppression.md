# Suppression markers

In-source suppression markers silence threshold violations without
editing the offending function or excluding the file from the walk.
Drop a marker in any comment in the source file and `bca check`
treats the covered metrics as if they were within limits for that
scope. Metric computation is unaffected — raw `bca metrics` /
`bca report` output still reports every number. Suppression is a
threshold-check concern only.

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
    // dense match on every supported opcode; rewrite tracked in #123
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

`abc`, `cognitive`, `cyclomatic`, `exit`, `halstead`, `loc`, `mi`,
`nargs`, `nom`, `npa`, `npm`, `wmc`.

They mostly match the JSON field names emitted on `CodeMetrics`, with
two deliberate differences:

- `exit` is the suppression spelling for the threshold name `nexits`
  (the JSON field is also `nexits`) — `bca: suppress(exit)` silences a
  `nexits` threshold violation.
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

The marker is dropped — a typo never silently widens scope to other
metrics. Unknown verbs (anything other than `suppress` / `suppress-file`)
and malformed bodies (unbalanced parentheses, trailing garbage)
produce the same shape of warning and are similarly dropped. None of
these are fatal: a typo in one file does not derail a workspace walk.

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
bca --paths src/ check --config bca-thresholds.toml --no-suppress
```

The flag has no effect on metric values themselves: raw
`bca metrics` / `bca report` output already ignores markers, since
suppression is a threshold-check concern only.

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

- `bca: suppress(metric, reason = "...")` — audit-trail prose alongside
  the metric list, mirroring Rust's `reason = "…"` attribute argument.
- `bca: suppress-next` — silence the immediately following declaration
  rather than the enclosing function.

Authors should avoid using either form today: a `reason = "..."`
argument is currently parsed as an unknown metric identifier and
discarded with a stderr warning, and `bca: suppress-next` is rejected
as an unknown verb. Both will be promoted to first-class behavior
in a future release without breaking existing markers.
