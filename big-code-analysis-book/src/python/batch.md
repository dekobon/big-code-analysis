# Batch processing

`bca.analyze_batch(paths)` runs the same analysis as `bca.analyze`
over every path in an iterable and **never raises on per-file
errors**: each result element is either an analysis `dict` or a
`bca.AnalysisFailure` describing the failure. Results preserve input
order, so `zip(inputs, results)` lines up by index **when no path
is skipped**. `analyze_batch` shares `analyze`'s keyword-only
options — `exclude_tests`, `allow_lossy_path`, `skip_generated`
(default `True`), and `metrics` — so the two entry points are
behaviour-preserving.

```python
{{#include ../../../big-code-analysis-py/examples/batch_processing.py:18:38}}
```

A few key contracts:

* `AnalysisFailure` is **returned**, not raised. It is not an
  `Exception` subclass — `isinstance(slot, bca.AnalysisFailure)` is
  the discriminator.
* `paths` is consumed lazily, so generators work — but if you
  want to keep the input around for `zip`, materialise it into a
  list first.
* With the default `skip_generated=True`, a generated file is
  *skipped* and produces **no** element, so the result list can
  be shorter than the input — exactly matching single-file
  `analyze`, which returns `None` for a generated file. Pass
  `skip_generated=False` to guarantee one element per input
  (the pre-2.0 default). This default flipped at 2.0 so that
  switching between `analyze` and `analyze_batch` no longer
  silently changes generated-file handling.

## Walking a directory: `analyze_paths`

`analyze_batch` analyses an **explicit list** of paths verbatim.
When you instead want to *find* the source files first — "analyze my
repo" — reach for `analyze_paths` (#658), which reuses the CLI's
gitignore-aware walker:

```python
import big_code_analysis as bca

results = bca.analyze_paths("path/to/repo", include="*.py")
```

Each positional seed may be a file or a directory; directories are
walked honouring `.gitignore`, the `include` / `exclude` globs (a
single glob string or a sequence; a leading `./` is optional, so
`dir/**` ≡ `./dir/**`), and the generated-file filter. A seed naming
a file directly is always analysed regardless of `exclude` — an
explicit request overrides ignore-style rules — while `include`
still narrows it by basename. `respect_gitignore=False` opts into
walking ignored files. The
result is the same `list[FuncSpaceDict | AnalysisFailure]` shape and
never-raise contract as `analyze_batch`, and it forwards the same
`exclude_tests` / `allow_lossy_path` / `skip_generated` / `metrics` /
`vcs` / `vcs_per_function` kwargs.

## Attaching change-history metrics

`analyze_batch` and `analyze_paths` accept the same `vcs=True` /
`vcs_per_function=True` kwargs as single-file `analyze` (#670). The
batch builds **one** history index / blame engine per containing
repository and reuses it across that repo's files — amortising the
walk that a comprehension over `analyze(p, vcs=True)` would repeat
per file. A VCS failure on one file leaves its AST metrics intact (it
never becomes an `AnalysisFailure`); a file outside any repository
simply gets no `vcs` block. For ranking a whole repository (rather
than per-file attachment), use the dedicated `big_code_analysis.vcs`
surface instead.

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

## `AnalysisFailure` taxonomy

`error_kind` is a closed `Literal`:

| `error_kind` | Triggered by |
|--------------|--------------|
| `"UnsupportedLanguage"` | Unknown extension + no shebang / emacs-mode hit |
| `"ParseError"` | tree-sitter rejected the source, or a rare internal serialisation failure (`internal: serialization error: …`) |
| `"IoError"` | `std::fs::read` failed **or** the path was not valid UTF-8 |

`AnalysisFailure` is frozen and implements `__eq__` / `__hash__` /
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
