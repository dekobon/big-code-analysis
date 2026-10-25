# Batch processing

`bca.analyze_batch(paths)` runs the same analysis as `bca.analyze`
over every path in an iterable and **never raises on per-file
errors**: each result element is an analysis `dict`, a
`bca.AnalysisFailure` describing the failure, or `None`. Results
preserve input order, so `zip(inputs, results)` lines up by index
**when no path is skipped**. This page is for a caller analysing
many files who wants one list back instead of a try/except per file.

`analyze_batch` shares `analyze`'s keyword-only options
(`exclude_tests`, `allow_lossy_path`, `skip_generated` with default
`True`, `metrics`, `vcs`, `vcs_per_function`, and `mi_charges_docs`),
so both entry points treat a file the same way. The list *shape*
matches the `[bca.analyze(p) for p in paths]` comprehension only
under `skip_generated=False`; with the default, the comprehension
keeps a `None` per skipped file while the batch drops the slot.

```python
{{#include ../../../big-code-analysis-py/examples/batch_processing.py:18:55}}
```

The contracts to rely on:

* `AnalysisFailure` is **returned**, not raised. It is not an
  `Exception` subclass; `isinstance(slot, bca.AnalysisFailure)` is
  the discriminator.
* `paths` is consumed lazily, so generators work. To keep the
  input around for `zip`, materialise it into a list first.
* With the default `skip_generated=True`, a generated file is
  *skipped* and produces **no** element, so the result list can
  be shorter than the input. This matches single-file `analyze`,
  which returns `None` for a generated file. Pass
  `skip_generated=False` to guarantee one element per input
  (the pre-2.0 default). The default flipped at 2.0 so that
  switching between `analyze` and `analyze_batch` no longer
  silently changes generated-file handling.
* A file that cannot be parsed at all still holds its slot under
  `skip_generated=False`, as `None`. The read gate shared with the
  CLI walker declines a file of three bytes or fewer, one carrying
  a UTF-16 BOM, and one whose leading window is not valid UTF-8.
  That gate is unconditional, so before #1238 those files shrank
  the list even with the flag off and the `zip` above mis-paired
  every later entry. `None` is the same value single-file
  `analyze` returns for them, and `bca.to_sarif` skips it, so a
  batch list can be passed straight through.

## Walking a directory: `analyze_paths`

`analyze_batch` analyses an **explicit list** of paths verbatim.
To *find* the source files first ("analyze my repo"), use
`analyze_paths` (#658), which reuses the CLI's gitignore-aware
walker:

```python
import big_code_analysis as bca

results = bca.analyze_paths("path/to/repo", include="*.py")
```

Each positional seed may be a file or a directory. Directories are
walked honouring `.gitignore`, the `include` / `exclude` globs (a
single glob string or a sequence; a leading `./` is optional, so
`dir/**` ≡ `./dir/**`), and the generated-file filter. A seed naming
a file directly is always analysed regardless of `exclude`, because
an explicit request overrides ignore-style rules; `include` still
narrows it by basename. `respect_gitignore=False` opts into walking
ignored files.

The result is the same `list[FuncSpaceDict | AnalysisFailure | None]`
shape and never-raise contract as `analyze_batch`, and it forwards
the same `exclude_tests` / `allow_lossy_path` / `skip_generated` /
`metrics` / `vcs` / `vcs_per_function` / `mi_charges_docs` kwargs. A
seed that does not exist leads the result list as an
`AnalysisFailure` (`error_kind="IoError"`) rather than being dropped.

The `None` slots reach this entry point too, under
`skip_generated=False`, one per discovered file the read gate
declined. Results follow the walk, so there is no caller-supplied
ordering to `zip` against; the slots exist so that a file the walk
found but could not analyse stays visible in the output. On a tree
with many binary assets that is a lot of `None`s; filter them with
`[r for r in results if r is not None]` if you only want records.

## Attaching change-history metrics

`analyze_batch` and `analyze_paths` accept the same `vcs=True` /
`vcs_per_function=True` kwargs as single-file `analyze` (#670). The
batch builds **one** history index and blame engine per containing
repository and reuses it across that repository's files, amortising
the walk that a comprehension over `analyze(p, vcs=True)` would
repeat per file. A VCS failure on one file leaves its AST metrics
intact (it never becomes an `AnalysisFailure`); a file outside any
repository gets no `vcs` block. To rank a whole repository rather
than attach per-file numbers, use the dedicated
[`big_code_analysis.vcs`](vcs.md) surface instead.

## Parallel execution

`analyze_batch` has no built-in concurrency; it is a sequential
sweep. For parallelism, fan the per-file `analyze` call out across
a thread pool:

```python
{{#include ../../../big-code-analysis-py/examples/batch_processing.py:58:70}}
```

[PyO3](https://pyo3.rs/)'s `Python::detach` releases the
[GIL](https://docs.python.org/3/glossary.html#term-global-interpreter-lock)
across each file's read and
[tree-sitter](https://tree-sitter.github.io/tree-sitter/) parse, so the threads
do not serialise on the interpreter lock.

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
`PermissionError`, and so on), call `bca.analyze(path)` per file
instead of `analyze_batch`; single-file `analyze` raises the
canonical `OSError` subclass. See [Error handling](errors.md).
