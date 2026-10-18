# Metric selection

Pass `metrics=[…]` to compute only a subset of the metric suite.
`metrics=None` (the default) preserves the "compute everything"
behaviour. Unrequested metrics are **absent** from the result
dict (not present with `None` placeholders).

```python
{{#include ../../../big-code-analysis-py/examples/metric_selection.py:17:47}}
```

The same kwarg is honoured by `bca.analyze_source` and
`bca.analyze_batch` — the latter applies the selection uniformly
to every file in the batch. Validation runs **before** any file
I/O: an empty list or unknown name raises `ValueError`
immediately and never returns an `AnalysisFailure` slot for what is
really a caller bug.

## Canonical names

The full set is available as a tuple of `MetricName` members. Each
member is a `StrEnum`, so it *is* a `str` — `"halstead" in
bca.METRIC_NAMES` works, and `bca.MetricName.HALSTEAD == "halstead"`
is `True`. Pass either a plain string or a member to `metrics=`:

```python
import big_code_analysis as bca
from big_code_analysis import MetricName

assert "halstead" in bca.METRIC_NAMES
assert bca.MetricName.HALSTEAD == "halstead"

# Either spelling works in `metrics=`:
selection = [MetricName.CYCLOMATIC, "cognitive"]
```

The members are generated from the same `Metric` table the CLI and
JSON output use, so the values never drift from the slugs you see in
`bca metrics --format json`.

## Accepted names

`metrics=` takes every name `bca metrics --metrics` takes, so a name
copied from `bca list-metrics` or from a `bca check --threshold`
limit works unchanged:

| Spelling | Example | Selects |
|----------|---------|---------|
| Family name, from `METRIC_NAMES` | `"loc"`, `"cyclomatic"` | that family |
| `bca list-metrics` row name | `"sloc"`, `"dloc"` | the family that owns the row, `loc` |
| Dotted threshold id | `"cyclomatic.modified"`, `"mi.original"` | that id's family |

Selection works per family, because the walk computes whole
families: `metrics=["dloc"]` returns the full `loc` block, `sloc` and
all, exactly as `metrics=["loc"]` does. `METRIC_NAMES` lists only the
family names, which are also the JSON keys, so `name in
bca.METRIC_NAMES` does not tell you whether `metrics=` accepts `name`.

Names are case-sensitive lowercase. An unknown name raises
`ValueError` naming the closest accepted names, then listing every
family and row name, as the CLI does:

```text
ValueError: unknown metric: dlocc; did you mean `dloc`?; known metrics: abc, blank, cloc, …
```

The canonical spelling for the exit-point metric is `"nexits"`
everywhere (the enum `Display`, `METRIC_NAMES`, and the JSON output
key). The legacy `"exit"` alias was retired at 2.0 and now raises
`ValueError` like any other unknown name.
Duplicates are silently collapsed.

## Dependencies

Selecting a metric that reads another metric's result computes that
one too. `cognitive`, `nargs` and `nexits` each report a per-function
average whose divisor is the function count `nom` supplies.

| Metric | JSON key | Dependencies pulled in |
|--------|----------|------------------------|
| LoC | `loc` | — |
| Cyclomatic | `cyclomatic` | — |
| Cognitive | `cognitive` | `nom` |
| Halstead | `halstead` | — |
| ABC | `abc` | — |
| `nargs` | `nargs` | `nom` |
| `nom` | `nom` | — |
| `npa` | `npa` | — |
| `npm` | `npm` | — |
| `nexits` | `nexits` | `nom` |
| `tokens` | `tokens` | — |
| Maintainability Index | `mi` | `loc`, `cyclomatic`, `halstead` |
| Weighted Methods per Class | `wmc` | `cyclomatic`, `nom` |

## Performance trade-off

Computing the full suite is the default because it is what the
CLI does. Selecting a single metric is **strictly faster** —
each `compute` pass is skipped — but the tree-sitter parse and
the AST walk are the dominant cost on most inputs, so the saving
on a single file is small. The benefit scales with batch size:
when `analyze_batch` runs across a large repository, dropping
the most expensive metric you do not need (often Halstead, on
deep call trees) is a measurable win.

Unrequested metrics are absent from the result. Code that
unconditionally indexes into `result["metrics"]["mi"]` will
`KeyError` if you opted out of `mi`; guard with `if "mi" in
result["metrics"]` or use `.get("mi")`.

## See also

* [Batch processing](batch.md) — `metrics=` applies uniformly to
  every file in a batch; validation runs once, before the input
  is iterated.
* [SARIF output](sarif.md) — threshold names are independent of
  the `metrics=` selection; you can request `metrics=["loc"]`
  and still gate on `cyclomatic` thresholds, but the SARIF will
  have no findings for the dropped metrics.
* [Flat-record iteration](flat-records.md) — `flatten_spaces`
  silently emits no keys for metrics that were absent from the
  source dict, so a `metrics=` selection naturally narrows the
  flattened columns.
