# Batch processing

`bca.analyze_batch(paths)` runs the same analysis as `bca.analyze`
over every path in an iterable and **never raises on per-file
errors**: each result slot is either an analysis `dict` or a
`bca.AnalysisError` describing the failure. The list has the same
length as the input and preserves order one-to-one, so callers
can `zip(inputs, results)` without losing the pairing.

```python
{{#include ../../../big-code-analysis-py/examples/batch_processing.py:18:38}}
```

A few key contracts:

* `AnalysisError` is **returned**, not raised. It is not an
  `Exception` subclass — `isinstance(slot, bca.AnalysisError)` is
  the discriminator.
* The result list is the same length as the input. `paths` is
  consumed lazily, so generators work — but if you want to keep
  the input around for `zip`, materialise it into a list first.
* `analyze_batch` runs with the `is_generated` walker filter
  **off**: every input position yields either a `dict` or an
  `AnalysisError`, never `None`. Call `bca.analyze(path)`
  per-file with the default `skip_generated=True` if you need
  the CLI walker's skip behaviour.

## Parallel execution

There is no built-in concurrency inside `analyze_batch` — it is a
sequential sweep. For parallelism, fan the per-file `analyze`
call out across a thread pool:

```python
{{#include ../../../big-code-analysis-py/examples/batch_processing.py:41:53}}
```

PyO3's `Python::detach` releases the GIL across each file's read +
tree-sitter parse, so the threads do not serialise on the
interpreter lock — this is real parallelism, not contended
co-operation.

## `AnalysisError` taxonomy

`error_kind` is a closed `Literal`:

| `error_kind` | Triggered by |
|--------------|--------------|
| `"UnsupportedLanguage"` | Unknown extension + no shebang / emacs-mode hit |
| `"ParseError"` | tree-sitter rejected the source, or a rare internal serialisation failure (`internal: serialization error: …`) |
| `"IoError"` | `std::fs::read` failed **or** the path was not valid UTF-8 |

`AnalysisError` is frozen and implements `__eq__` / `__hash__` /
`__repr__` over all three fields, so callers can put errors in a
`set` to deduplicate failures across runs. For retry
classification, the `errno` is preserved in the `error` string via
Rust's default formatting:

```python
import re

match = re.search(r"\(os error (\d+)\)$", slot.error)
errno = int(match.group(1)) if match else None
```

If you need typed dispatch (`FileNotFoundError`,
`PermissionError`, …) call `bca.analyze(path)` per-file instead
of `analyze_batch` — single-file `analyze` raises the
canonical `OSError` subclass. See [Error handling](errors.md).
