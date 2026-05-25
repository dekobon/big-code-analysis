# Python Bindings

`big-code-analysis` ships first-party Python bindings (PyO3 +
[maturin](https://www.maturin.rs/)) that expose the same metric
pipeline as the Rust library and the `bca` CLI — same JSON shape,
same numeric formatting, same language coverage.

```python
import big_code_analysis as bca

result = bca.analyze("src/main.rs")
if result is not None:
    print(result["metrics"]["cyclomatic"]["sum"])
```

The bindings are a peer of the Rust API: anywhere this book points
at a Rust function (`big_code_analysis::analyze`,
[`FuncSpace`](../library/walking-funcspace.md), the metric modules),
Python has a one-to-one equivalent. Pick whichever language fits
your pipeline — the metrics are identical.

## When to reach for Python

* You're already in a data-pipeline stack (pandas, Jupyter,
  Airflow, dbt, Polars) and want metric records as
  `dict`/`DataFrame` rows without shelling out to the CLI.
* You're integrating with a Python-native security tool that
  consumes SARIF — see [SARIF output](sarif.md).
* You're building a code-quality dashboard whose backend is a
  Python web framework (FastAPI, Django).

If you only need a one-shot quality report from the command line,
the `bca` CLI is the simpler tool — see
[Commands → Metrics](../commands/metrics.md).

If you're embedding the analysis into a long-running Rust program,
the [Rust library](../library/index.html) is the lower-overhead
option.

## Chapter contents

* [Installation](installation.md) — `pip install`, wheel matrix,
  building from source.
* [Quick start](quick-start.md) — analyse one file, print one
  metric.
* [Batch processing](batch.md) — `analyze_batch`,
  `AnalysisError`, parallelism with `ThreadPoolExecutor`.
* [Flat-record iteration](flat-records.md) — `flatten_spaces`
  feeding sqlite / pandas.
* [Metric selection](metrics.md) — `metrics=` kwarg,
  `bca.METRIC_NAMES`, dependency-pull semantics.
* [SARIF output](sarif.md) — `to_sarif` + GitHub Code Scanning
  upload.
* [Error handling](errors.md) — the full exception taxonomy and
  the never-raise batch contract.
* [Async patterns](async.md) — `asyncio.to_thread` is the
  canonical recipe.

The headline example on each page is embedded verbatim from an
importable file under `big-code-analysis-py/examples/` and
exercised end-to-end by
`big-code-analysis-py/tests/test_book_examples.py`, so a renamed
kwarg or a removed function on the primary path fails CI before
it can rot the docs. Shorter illustrative snippets that surround
the embedded example (logging recipes, regex parsing of the
errno suffix, the `asyncio` anti-pattern, the pandas
one-liner, …) are inline and intentionally not test-pinned —
treat the embedded blocks as the canonical reference when the
two disagree.
