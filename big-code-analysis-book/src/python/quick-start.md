# Quick start

This page shows the least code needed to compute metrics for one
source file: install the package, call `analyze`, read a value. It
assumes Python 3.12 or newer and [pip](https://pip.pypa.io/en/stable/).

## 1. Install the package

```bash
pip install big-code-analysis
```

See [Installation](installation.md) for the wheel matrix and
build-from-source instructions.

## 2. Analyse a file

`bca.analyze(path)` returns a `dict` matching the JSON `bca
metrics --format json` emits for the same file: same field
order, same numeric formatting, same shape.

```python
{{#include ../../../big-code-analysis-py/examples/quick_start.py}}
```

Three details matter here:

* `analyze` returns `None` for any file the CLI walker would skip:
  one that is three bytes or fewer (treated as empty), one whose
  leading window is not valid UTF-8 (treated as binary), or, with
  the default `skip_generated=True`, one matching the walker's
  `is_generated` predicate (a leading `@generated`, `DO NOT EDIT`,
  or `GENERATED CODE` marker). Handle the `None` before reaching
  into `result["metrics"]`.
* The returned object is a plain dict at runtime, so you can
  serialise it with `json.dumps`, ship it to a downstream service,
  or feed it to [`flatten_spaces`](flat-records.md) for tabular
  consumers. Type checkers see it as the `FuncSpaceDict` TypedDict
  (generated from the Rust wire shapes), so nested metric access
  checks statically under [mypy](https://mypy.readthedocs.io/) and
  [pyright](https://github.com/microsoft/pyright) without casts.
* Language detection mirrors the CLI: path extension first, then
  an Emacs or Vim mode line, then the shebang. Pass
  `bca.analyze_source(code, language)` when the source is already
  in memory.

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

* [Batch processing](batch.md): `analyze_batch` for many files
  without per-file try/except clutter.
* [Metric selection](metrics.md): compute only the metrics you
  need.
* [Error handling](errors.md): the full exception taxonomy.
* The CLI's [Metrics](../commands/metrics.md) command: the
  equivalent shell-level workflow.
