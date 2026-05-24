# Quick start

This page walks through the minimum amount of code needed to
compute metrics from a single source file.

## 1. Install the package

```bash
pip install big-code-analysis
```

See [Installation](installation.md) for the wheel matrix and
build-from-source instructions.

## 2. Analyse a file

`bca.analyze(path)` returns a `dict` matching the JSON `bca
metrics --output-format json` emits for the same file — same field
order, same numeric formatting, same shape.

```python
{{#include ../../../big-code-analysis-py/examples/quick_start.py}}
```

A few details worth noting:

* `analyze` returns `None` when the file matches the CLI walker's
  `is_generated` predicate (a leading `@generated`, `DO NOT EDIT`,
  or `GENERATED CODE` marker). Always handle the optional return
  before reaching into `result["metrics"]`.
* The returned object is a plain `dict[str, Any]`. It is safe to
  serialise with `json.dumps`, ship to a downstream service, or
  feed into [`flatten_spaces`](flat-records.md) for tabular
  consumers.
* Language detection mirrors the CLI exactly: path extension
  first, then shebang / emacs-mode fallback. Pass
  `bca.analyze_source(code, language)` if you have the source
  in-memory.

## 3. Analyse an in-memory snippet

```python
import big_code_analysis as bca

metrics = bca.analyze_source("fn main() {}\n", "rust")
print(metrics["metrics"]["loc"]["sloc"])
```

`analyze_source` accepts `str`, `bytes`, or `bytearray`. The
returned `dict` has the same shape as `analyze`'s output, with
`name` set to `None` (no path is associated with an in-memory
buffer).

## Where to go next

* [Batch processing](batch.md) — `analyze_batch` for many files
  without per-file try/except clutter.
* [Metric selection](metrics.md) — compute only the metrics you
  need.
* [Error handling](errors.md) — the full exception taxonomy.
* The CLI's [Metrics](../commands/metrics.md) command is the
  equivalent shell-level workflow.
