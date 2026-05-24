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
immediately and never returns an `AnalysisError` slot for what is
really a caller bug.

## Canonical names

The full set is available as a tuple:

```python
import big_code_analysis as bca

assert "halstead" in bca.METRIC_NAMES
```

Names are case-sensitive lowercase; passing an unknown name
raises `ValueError` with the canonical list in the message. The
`"exit"` Metric-Display spelling is accepted as an alias for the
canonical JSON-key spelling `"nexits"`; both produce a
`"nexits"` key in the output. Duplicates are silently collapsed.

| Metric | JSON key | Dependencies pulled in |
|--------|----------|------------------------|
| LoC | `loc` | — |
| Cyclomatic | `cyclomatic` | — |
| Cognitive | `cognitive` | — |
| Halstead | `halstead` | — |
| ABC | `abc` | — |
| `nargs` | `nargs` | — |
| `nom` | `nom` | — |
| `npa` | `npa` | — |
| `npm` | `npm` | — |
| `nexits` (alias `exit`) | `nexits` | — |
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
