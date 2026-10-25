# Python Bindings

`big-code-analysis` ships first-party Python bindings, built with
[PyO3](https://pyo3.rs/) and [maturin](https://www.maturin.rs/), that
expose the same metric pipeline as the Rust library and the `bca`
CLI: same JSON shape, same numeric formatting, same language coverage.
This chapter is for a Python developer deciding whether to use the
bindings and then finding the page for each workflow.

```python
import big_code_analysis as bca

result = bca.analyze("src/main.rs")
if result is not None:
    print(result["metrics"]["cyclomatic"]["sum"])
```

The bindings cover the analysis workflows: file and in-memory
analysis (`big_code_analysis::analyze`), the
[`FuncSpace`](../library/walking-funcspace.md) result as a typed
dict, metric selection, AST traversal,
[SARIF](https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html)
output, and the VCS metrics. The metrics are computed by the same
Rust code, so the numbers are identical. Python also exposes the
language-detection helpers. Some lower-level Rust facilities,
including the C preprocessor pass and the output writers, have no
Python counterpart. Pick whichever language fits your pipeline.

## When to reach for Python

* You work in a data-pipeline stack ([pandas](https://pandas.pydata.org/),
  [Jupyter](https://jupyter.org/), [Airflow](https://airflow.apache.org/),
  [dbt](https://www.getdbt.com/), [Polars](https://pola.rs/)) and want
  metric records as `dict` or `DataFrame` rows without shelling out to
  the CLI.
* You integrate with a Python-native security tool that consumes
  SARIF; see [SARIF output](sarif.md).
* You build a code-quality dashboard on a Python web framework
  ([FastAPI](https://fastapi.tiangolo.com/),
  [Django](https://www.djangoproject.com/)).

If you only need a one-shot quality report from the command line,
the `bca` CLI is the simpler tool; see
[Commands → Metrics](../commands/metrics.md).

If you embed the analysis in a long-running Rust program, the
[Rust library](../library/index.html) has lower overhead.

## Chapter contents

* [Installation](installation.md): `pip install`, the wheel matrix,
  building from source.
* [Quick start](quick-start.md): analyse one file, print one
  metric.
* [Batch processing](batch.md): `analyze_batch`,
  `AnalysisFailure`, parallelism with `ThreadPoolExecutor`.
* [Flat-record iteration](flat-records.md): `flatten_spaces`
  feeding [SQLite](https://sqlite.org/) or pandas.
* [Metric selection](metrics.md): the `metrics=` kwarg,
  `bca.METRIC_NAMES` and the CLI aliases, dependency-pull semantics.
* [AST traversal](traversal.md): `Ast`, `Node`, `walk()`, and
  `find()` over the held parse.
* [SARIF output](sarif.md): `to_sarif` and the
  [GitHub Code Scanning](https://docs.github.com/en/code-security/code-scanning)
  upload.
* [Change-history (VCS) metrics](vcs.md): `vcs.rank`, `vcs.trend`,
  `vcs.commit`, and `vcs.score_diff` over a git working tree.
* [Error handling](errors.md): the full exception taxonomy and
  the never-raise batch contract.
* [Async patterns](async.md): `asyncio.to_thread` as the
  canonical recipe.

The headline example on each page is embedded verbatim from an
importable file under `big-code-analysis-py/examples/` and run
end-to-end by `big-code-analysis-py/tests/test_book_examples.py`,
so a renamed kwarg or a removed function on the primary path fails
CI before it can rot the docs. The shorter snippets around each
embedded example (logging recipes, regex parsing of the errno
suffix, the `asyncio` anti-pattern, the pandas one-liner) are
inline and deliberately not test-pinned. Treat the embedded blocks
as the canonical reference when the two disagree.
