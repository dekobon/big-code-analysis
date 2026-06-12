# Error handling

The bindings split errors into two domains:

* **Caller errors** are raised — `ValueError` for bad arguments,
  `TypeError` for the wrong type, `OSError` and its subclasses
  for filesystem failures.
* **Per-file analysis errors** in a batch are *returned* as
  `bca.AnalysisFailure` values inside the result list. They are
  not exceptions and never raise.

The single-file `bca.analyze` walks the first path; the batch
`bca.analyze_batch` walks the second.

```python
{{#include ../../../big-code-analysis-py/examples/errors_taxonomy.py:30:74}}
```

## Single-file exceptions

`bca.analyze` and `bca.analyze_source` raise:

| Exception | Subclass of | Triggered by |
|-----------|-------------|--------------|
| `bca.UnsupportedLanguageError` | `ValueError` | Unknown extension + no shebang / emacs-mode hit |
| `bca.ParseError` | `ValueError` | tree-sitter rejected the source |
| `ValueError` (raw) | — | Non-UTF-8 path with `allow_lossy_path=False` (the default) |
| `OSError` and subclasses | — | `std::fs::read` failed |

The `OSError` raised by `analyze` dispatches to the canonical
subclass based on `errno`:

```python
import big_code_analysis as bca

path = "src/example.rs"

try:
    bca.analyze(path)
except FileNotFoundError as err:
    print("missing:", err.errno, err.filename)
except PermissionError as err:
    print("denied:", err.errno, err.filename)
except IsADirectoryError as err:
    print("directory:", err.errno, err.filename)
```

Each branch dispatches on the underlying `errno`:

| Exception | Typical `err.errno` (Linux) | When it fires |
|-----------|-----------------------------|---------------|
| `FileNotFoundError` | 2 (`ENOENT`) | Path does not exist. |
| `PermissionError` | 13 (`EACCES`) | Read bit denied for the calling user. |
| `IsADirectoryError` | 21 (`EISDIR`) | Path resolves to a directory. |

Use `except OSError` if you want to catch the whole family and
inspect `err.errno` / `err.filename` yourself.

`UnsupportedLanguageError` and `ParseError` are both `ValueError`
subclasses, so a single `except ValueError` catches both. Prefer
the typed catches when you want to differentiate.

## Batch errors

`bca.analyze_batch` returns `bca.AnalysisFailure` values instead of
raising, so a single bad file does not break the whole batch.

```python
for slot in bca.analyze_batch(paths):
    if isinstance(slot, bca.AnalysisFailure):
        log.warning("%s (%s): %s", slot.path, slot.error_kind, slot.error)
    else:
        process(slot)
```

`error_kind` is a closed `Literal`:

* `"UnsupportedLanguage"` — extension and shebang / emacs-mode
  resolution both came up empty.
* `"ParseError"` — tree-sitter rejected the input, or (rare) a
  Rust-side JSON serialisation of the result failed. The
  serialisation case is prefixed with `internal: serialization error:`
  in the `error` string; check for the prefix when the
  distinction matters (serialisation failures are not recoverable
  by re-reading the file).
* `"IoError"` — the most common kind: `std::fs::read` failed. The
  closed taxonomy also folds in non-UTF-8 path failures, so a
  path-encoding error surfaces as `"IoError"` rather than as a
  distinct fourth value.

For `"IoError"` instances the underlying OS `errno` is preserved
in the `error` string via Rust's default formatting (`"<msg> (os
error <N>)"` on Unix). Parse with regex if you need it for retry
classification:

```python
import re

match = re.search(r"\(os error (\d+)\)$", slot.error)
errno = int(match.group(1)) if match else None
```

If you need typed `OSError` subclasses, call `bca.analyze` per
file instead of `analyze_batch` — single-file `analyze` raises
`FileNotFoundError` / `PermissionError` / `IsADirectoryError`
directly.

## Programmer errors in batches

`analyze_batch` does still raise on caller bugs:

* `TypeError` if `paths` is not iterable, or an element is not
  `str` / `os.PathLike[str]`. This aborts the whole call; any
  results computed before the bad element are discarded.
* `ValueError` if `metrics=` is an explicitly empty sequence or
  contains an unknown name. Validation runs *before* the input
  iterable's `__iter__`, so a generator's side effects (and any
  partial yields) are preserved on this raise path.

## Change-history (VCS) exceptions

The [`big_code_analysis.vcs`](vcs.md) functions raise a typed
hierarchy rooted at `bca.VcsError`, itself a `ValueError`, so an
existing `except ValueError` (or `except bca.VcsError`) catches every
VCS failure (#624). The `analyze(..., vcs=True)` kwarg shares the same
option-parsing errors.

| Exception | Subclass of | Triggered by |
|-----------|-------------|--------------|
| `bca.NotARepositoryError` | `bca.VcsError` | `repo_path` is not inside a git working tree |
| `bca.InvalidRevisionError` | `bca.VcsError` | A `reference` / `commit` could not be resolved |
| `bca.InvalidDiffError` | `bca.VcsError` | The `diff` passed to `vcs.score_diff` is malformed |
| `bca.VcsEnvironmentError` | `bca.VcsError` | History walk, diffing, `.mailmap`, blame, or cache I/O failed |
| `bca.VcsError` (raw) | `ValueError` | A malformed option value (window / timestamp / formula / file-type scope / bus-factor threshold / bot pattern / trend point count); the message names the offending value |

`NotARepositoryError` is the variant to branch on for "not a repo,
skip this directory". The base `VcsError` is raised directly for a bad
**option**, where the message names the offending value, while the
named subclasses cover the **input** failures (a missing revision, a
malformed diff). `VcsEnvironmentError` is the environment / backend
bucket, mirroring the `500` (not `400`) responses the web crate returns
for the same failures.

```python
import big_code_analysis as bca
from big_code_analysis import vcs

try:
    report = vcs.rank("path/to/repo", top=20)
except bca.NotARepositoryError:
    print("not a git repository, skipping")
except bca.VcsError as err:
    # Malformed window, formula, file-type scope, and so on.
    print("bad VCS option:", err)
```

`analyze(..., vcs=True)` is the exception to the
`NotARepositoryError` rule: a file outside any repository simply yields
no `vcs` block rather than raising, so only the option-parsing
`VcsError` reaches the caller from that path.

## Logging recipe

A small logging helper for batch output keeps successes /
failures aligned without bespoke formatting:

```python
import logging
import big_code_analysis as bca

log = logging.getLogger(__name__)

def report(paths: list[str]) -> None:
    for path, slot in zip(paths, bca.analyze_batch(paths)):
        if isinstance(slot, bca.AnalysisFailure):
            log.warning(
                "skip %s (%s): %s", path, slot.error_kind, slot.error
            )
        else:
            log.info(
                "ok %s sloc=%s", path,
                slot["metrics"]["loc"]["sloc"],
            )
```

## See also

* [Batch processing](batch.md) — the never-raise contract that
  routes per-file failures into `AnalysisFailure` slots.
* [Async patterns](async.md) — `asyncio.gather(...,
  return_exceptions=True)` is the async-side equivalent of the
  batch contract: per-task exceptions land in the result list
  instead of cancelling the whole gather.
* [Quick start](quick-start.md) — the single-file `analyze`
  path that raises typed `OSError` subclasses.
