# big-code-analysis (Python bindings)

Python bindings for the
[`big-code-analysis`](https://github.com/dekobon/big-code-analysis)
Rust library: compute maintainability metrics for source code in
about twenty languages using the same
[tree-sitter](https://tree-sitter.github.io/tree-sitter/) parsers the
Rust crate ships with. This README is the quick reference shown on
[PyPI](https://pypi.org/project/big-code-analysis/); the book's
[Python Bindings chapter](https://dekobon.github.io/big-code-analysis/python/index.html)
covers the install matrix, the batch, async and
[SARIF 2.1.0](https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html)
recipes, and the full error taxonomy.

The bindings provide single-file analysis, a never-raise batch entry
point, the `flatten_spaces` flat-record iterator, explicit metric
selection (`metrics=`), SARIF rendering (`to_sarif`), a `vcs`
submodule for change-history metrics, an `Ast` / `Node` surface for
lazy syntax-tree traversal, and
[manylinux](https://github.com/pypa/manylinux) wheels for Linux
x86_64 and aarch64. See the
[CHANGELOG](https://github.com/dekobon/big-code-analysis/blob/main/CHANGELOG.md)
for what changed between releases.

## Runnable examples

`big-code-analysis-py/examples/` is the canonical collection of
copy-paste recipes. CI executes every file, either through
`tests/test_book_examples.py` (the `.py` examples) or through
[`jupyter nbconvert --execute`](https://nbconvert.readthedocs.io/)
(the [Jupyter](https://jupyter.org/) notebook), so a renamed kwarg
or removed function fails CI before the example can rot in the
docs.

| File | What it shows |
|------|---------------|
| `quick_start.py` | Single-file analysis + headline metric. Embedded by the book's *Quick start*. |
| `batch_processing.py` | `analyze_batch` + the `AnalysisFailure` discriminator. Embedded by *Batch processing*. |
| `flat_records.py` | `flatten_spaces` → [SQLite](https://sqlite.org/) for one file. Embedded by *Flat-record iteration*. |
| `metric_selection.py` | `metrics=` kwarg + dependency-pull behavior. Embedded by *Metric selection*. |
| `sarif_output.py` | Minimal SARIF rendering. Embedded by *SARIF output*. |
| `errors_taxonomy.py` | The full exception map across the entry points. Embedded by *Error handling*. |
| `async_patterns.py` | `asyncio.to_thread` (canonical) vs the in-loop anti-pattern. Embedded by *Async patterns*. |
| `cli_parity.py` | Byte-for-byte parity smoke test vs `bca metrics --format json`. Wired into `make py-test`. |
| `pipeline_db.py` | Directory walk → `analyze_batch` → `flatten_spaces` → sqlite top-N, with a deliberately broken file to exercise the never-raise contract. |
| `sarif_upload.py` | SARIF emission tuned for [GitHub Code Scanning](https://docs.github.com/en/code-security/code-scanning) (`github/codeql-action/upload-sarif@v3`). |
| `jupyter_quickstart.ipynb` | [pandas](https://pandas.pydata.org/) DataFrame + [matplotlib](https://matplotlib.org/) `cyclomatic.sum` per function + top-N. Executed in CI via `python-examples-nbconvert`. |

## Installation

The package is published on PyPI: the distribution is
**`big-code-analysis`** and the import name **`big_code_analysis`**,
names locked by the stability contract (see
[`STABILITY.md`](https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md),
*Python bindings*).

```bash
pip install big-code-analysis
```

For development, build locally via [maturin](https://www.maturin.rs/).
The recommended bootstrap uses [uv](https://docs.astral.sh/uv/) so the
resolved dev set matches the checked-in `uv.lock`: every contributor
on this path gets the same [ruff](https://docs.astral.sh/ruff/),
[mypy](https://mypy.readthedocs.io/),
[pyright](https://github.com/microsoft/pyright), maturin and
[pytest](https://docs.pytest.org/) versions. CI consumes `uv.lock` through the hash-pinned exports under
`requirements/` (`pip install --require-hashes -r …` in the
workflows), so a `uv.lock` change and its regenerated exports must
land in the same commit:

```bash
make py-bootstrap          # runs `uv sync --locked --extra dev` in big-code-analysis-py/
                           # creates .venv with ruff, mypy, pyright, maturin, pytest
cd big-code-analysis-py
source .venv/bin/activate
maturin develop            # builds the compiled extension into .venv
python -c "import big_code_analysis; print(big_code_analysis.__version__)"
```

Install `uv` via
`curl -LsSf https://astral.sh/uv/install.sh | sh`, `brew install uv`,
or `pipx install uv`. Alternative provisioning paths (`mise install`,
direct `pipx install ruff/mypy/pyright/maturin`, plain
`pip install -e ".[dev]"`) still work (see
[CONTRIBUTING.md](https://github.com/dekobon/big-code-analysis/blob/main/CONTRIBUTING.md)),
but only the uv-managed path is pinned to `uv.lock`.

## Usage

```python
import big_code_analysis as bca

# Analyze a file by path. The returned dict matches the JSON
# emitted by `bca metrics --format json` for the same file at the
# `FuncSpace` boundary: same field order, same numeric formatting,
# same shape. Language detection mirrors the CLI (path extension,
# then an Emacs / Vim mode line, then shebang). Generated files
# (`@generated`, `DO NOT EDIT`, `GENERATED CODE` markers) are
# skipped by default, matching the CLI walker: `analyze` returns
# `None` for them. See `bca.analyze.__doc__` for the full parity
# contract.
result = bca.analyze("src/main.rs")
if result is not None:
    print(result["metrics"]["cognitive"]["sum"])

# Prune Rust `#[test]` / `#[cfg(test)]` subtrees before computing
# metrics: the same result as `bca metrics --exclude-tests`.
prod_only = bca.analyze("src/main.rs", exclude_tests=True)

# Non-UTF-8 paths raise `ValueError` by default so the `name`
# field is always a round-trippable identifier. Pass
# `allow_lossy_path=True` to opt into the CLI's U+FFFD
# substitution behavior (see `bca.analyze.__doc__` and #316).
lossy = bca.analyze(weird_path, allow_lossy_path=True)

# Force analysis of files marked `@generated` (default skips them).
forced = bca.analyze("third_party/generated.pb.go", skip_generated=False)

# Analyze an in-memory snippet (str, bytes, or bytearray accepted).
metrics = bca.analyze_source("fn main() {}\n", "rust")

# Language detection helpers. `language_for_file` reads the file
# and runs the same detection pipeline as `analyze` (path extension,
# then mode line, then shebang; #318), so an extension-less script
# with a `#!/usr/bin/env python` first line resolves the same way.
# With the default `read=True` the path must exist, and I/O failures
# raise the same typed `OSError` subclass `analyze` does. Pass
# `read=False` for the filesystem-free extension-only lookup, which
# never raises and answers for paths that do not exist yet;
# `language_for_extension` is the same table lookup for a bare
# extension (`"py"` / `".py"`).
assert bca.language_for_file("path/to/real/foo.py") == "python"
assert bca.language_for_file("not/yet/written.py", read=False) == "python"
assert bca.language_for_extension(".py") == "python"
# Extension-less script with a `#!/usr/bin/env python` first line
# would resolve to "python" too (the asymmetry #318 closed).
assert "python" in bca.supported_languages()
assert "py" in bca.language_extensions("python")

# `supported_languages()` returns `list[Lang]` and
# `language_for_file()` returns `Lang | None`. `Lang` is a
# `StrEnum`, so the members *are* strings: `Lang.PYTHON == "python"`
# is True and `bca.Lang.PYTHON in bca.supported_languages()` holds.
# `language_extensions()` accepts a `Lang` member or a plain slug.
from big_code_analysis import Lang

assert bca.Lang.PYTHON in bca.supported_languages()
assert bca.language_for_file("path/to/real/foo.py") is Lang.PYTHON
assert "py" in bca.language_extensions(Lang.PYTHON)
```

## Selecting metrics

Pass `metrics=[…]` to compute only a subset of the metric suite.
`metrics=None` (the default) computes everything. Unrequested
metrics are **absent** from the result dict, not present with `None`
placeholders.

```python
import big_code_analysis as bca

# Compute only LoC and cyclomatic complexity.
result = bca.analyze("src/main.rs", metrics=["loc", "cyclomatic"])
assert result is not None
assert set(result["metrics"]) == {"loc", "cyclomatic"}

# Selecting a derived metric pulls its dependencies in automatically:
# `metrics=["mi"]` also computes loc, cyclomatic, and halstead.
mi_result = bca.analyze("src/main.rs", metrics=["mi"])
assert mi_result is not None
assert {"loc", "cyclomatic", "halstead", "mi"}.issubset(mi_result["metrics"])

# `bca.METRIC_NAMES` is a `tuple[MetricName, ...]` enumerating every
# canonical name accepted by `metrics=`. `MetricName` is a `StrEnum`,
# so `"halstead" in bca.METRIC_NAMES` works and you can pass either a
# member or a plain string to `metrics=`.
assert "halstead" in bca.METRIC_NAMES
assert bca.MetricName.HALSTEAD == "halstead"
```

`bca.analyze_source` and `bca.analyze_batch` honor the same kwarg;
the batch applies the selection uniformly to every file. Validation
runs *before* any file I/O: an empty list or unknown name raises
`ValueError` immediately and never returns an `AnalysisFailure` slot
for what is really a caller bug.

```python
# Compute only `cyclomatic` and `cognitive` across a batch.
results = bca.analyze_batch(
    ["src/a.py", "src/b.rs"],
    metrics=["cyclomatic", "cognitive"],
)
```

`metrics=` takes every name `bca metrics --metrics` takes: besides
the family names in `METRIC_NAMES`, the `bca list-metrics` row names
(`"sloc"`, `"dloc"`, …) and the dotted `bca check --threshold` ids
(`"cyclomatic.modified"`, `"mi.original"`, …). Each alias selects its
whole family, so `metrics=["dloc"]` returns the full `"loc"` block.

Names are case-sensitive lowercase; passing an unknown name raises
`ValueError` with a did-you-mean hint and the accepted names in the
error message. The number-of-exit-points metric is spelled `"nexits"`
(matching its JSON-key spelling); the legacy `"exit"` alias was
retired at 2.0. Duplicates are silently collapsed.

## SARIF 2.1.0 output

`bca.to_sarif(result, *, thresholds=None)` renders an analysis
result (or an iterable of them) into a SARIF 2.1.0 JSON document
for upload to GitHub Code Scanning or any other SARIF consumer. The output is produced by the same Rust
writer that backs `bca check --report-format sarif`, so the schema
URL, tool driver name and version, and rule descriptions match the
CLI byte-for-byte.

```python
import big_code_analysis as bca

# Single file → SARIF with a finding for every function whose
# cyclomatic complexity strictly exceeds 15.
sarif = bca.to_sarif(
    bca.analyze("src/main.py"),
    thresholds={"cyclomatic": 15, "loc.lloc": 200},
)
with open("metrics.sarif", "w", encoding="utf-8") as fh:
    fh.write(sarif)

# Batch input: AnalysisFailure entries are skipped silently because
# they represent files we could not analyze, not findings.
batch = bca.analyze_batch(["src/a.py", "src/b.rs", "src/c.cpp"])
sarif = bca.to_sarif(batch, thresholds={"cognitive": 20})
```

Accepted threshold names are the library's `metric_catalog::METRICS`
table in `src/metric_catalog.rs`, the same source the CLI reads:
`"cognitive"`,
`"cyclomatic"`, `"cyclomatic.modified"`, `"halstead.volume"`,
`"halstead.difficulty"`, `"halstead.effort"`, `"halstead.time"`,
`"halstead.bugs"`, `"loc.sloc"`,
`"loc.ploc"`, `"loc.lloc"`, `"loc.cloc"`, `"loc.blank"`,
`"loc.dloc"`, `"nom"`,
`"tokens"`, `"nexits"`, `"nargs"`, `"mi.original"`, `"mi.sei"`,
`"mi.visual_studio"`, `"abc"`, `"wmc"`, `"npm"`, `"npa"`. Names
resolve by the rule `bca check --threshold` applies: a bare `loc`
sub-metric name (`"sloc"`) stands for its dotted id, and a family head
with no single scalar (`"mi"`, `"halstead"`, `"loc"`) raises
`ValueError` naming the ids, as does one id given under both
spellings. Any other unknown name raises `ValueError` listing the
accepted set, so a typo fails fast instead of silently producing an
empty SARIF run.

`thresholds=None` (the default) and `thresholds={}` both produce
a well-formed SARIF document with empty `results` and `rules`
arrays. This matches `bca check`, which applies **no implicit
limits**: every run supplies its own, from `--threshold` or a
`bca.toml` (which `bca init` scaffolds with a starting table).

**Unit-level findings.** `to_sarif` emits a finding at every space
whose **own** value breaches its limit, exactly matching
`bca check --report-format sarif`. Emission is scope-gated per metric
(`loc.*` at the file unit, `nom` / `wmc` / `npm` / `npa` at
containers, the rest at function spaces), and the five
subtree-aggregate metrics (`cyclomatic`, `cyclomatic.modified`,
`cognitive`, `abc`, `nargs`) read the per-space `value` field rather
than the rolled-up aggregate (#958, #969, #1236). Unit findings carry
`logicalLocations: [{"fullyQualifiedName":
"<file>"}]`; every other space carries its qualified symbol. Within
that symbol, a closure/lambda (the `<anonymous>` name every grammar
emits) and the `None`-name parse-failure case both collapse to
`<anon@L{start_line}>`, matching the CLI's `space_segment`. See the book's
[SARIF output](https://dekobon.github.io/big-code-analysis/python/sarif.html)
page for the full contract.

### Upload to GitHub Code Scanning

```yaml
# .github/workflows/code-scanning.yml (excerpt)
- name: Compute metric SARIF
  run: |
    python - <<'PY'
    import big_code_analysis as bca
    with open("paths.txt", encoding="utf-8") as paths_fh:
        results = bca.analyze_batch(paths_fh.read().splitlines())
    with open("metrics.sarif", "w", encoding="utf-8") as fh:
        fh.write(bca.to_sarif(results, thresholds={"cyclomatic": 15}))
    PY
- name: Upload to Code Scanning
  uses: github/codeql-action/upload-sarif@v3
  with:
    sarif_file: metrics.sarif
```

## Batch processing

`bca.analyze_batch(paths)` runs the same analysis as `bca.analyze`
over every path in an iterable and **never raises on per-file
errors**: each result element is an analysis `dict`, a
`bca.AnalysisFailure` describing the failure, or `None`. Results
preserve input order, so `zip(inputs, results)` lines up by index
**when no path is skipped**. `analyze_batch` accepts the same
keyword-only options as `analyze` (`exclude_tests`,
`allow_lossy_path`, `skip_generated` with default `True`, `metrics`,
`vcs`, `vcs_per_function`, `mi_charges_docs`), so migrating between
the two preserves behavior.

With the default `skip_generated=True`, a generated file is *skipped*
and produces no result element, so the result list can be shorter than
the input. The shape differs from the comprehension it replaces:
`[bca.analyze(p) for p in paths]` keeps a `None` per skipped file
(staying index-aligned), while the batch drops the slot. The per-file
*filter semantics* match; the list shape does not. This default
flipped at 2.0. The pre-2.0 `analyze_batch` always analyzed generated
files (`skip_generated=False`); pass that explicitly to restore one
element per input.

`skip_generated=False` really is one element per input, including for
files that cannot be parsed at all. The read gate `analyze` shares with
the CLI walker declines a file of three bytes or fewer, one carrying a
UTF-16 BOM, and one whose leading window is not valid UTF-8. Those
hold their slot as `None`, the same value single-file `analyze`
returns for them, rather than dropping out of the list. Before #1238
they dropped regardless of the flag, so one binary file in a corpus
silently mis-attributed every later result to the wrong path.

```python
import big_code_analysis as bca

paths = ["src/a.py", "src/missing.py", "src/b.rs"]
results = bca.analyze_batch(paths, skip_generated=False)
for path, result in zip(paths, results):
    if isinstance(result, bca.AnalysisFailure):
        print(f"skipped {path}: ({result.error_kind}) {result.error}")
    elif result is None:
        print(f"skipped {path}: nothing to parse (empty or binary)")
    else:
        process(result)
```

If you need to pair results with their input paths, materialize the
paths before calling `analyze_batch`. Passing the same generator
(`glob.iglob('**/*.py')`, say) to both `zip` and `analyze_batch`
yields nothing because the batch exhausts it first:

```python
import glob

paths = list(glob.iglob("src/**/*.py", recursive=True))
results = bca.analyze_batch(paths, skip_generated=False)
# now zip(paths, results) works: skip_generated=False keeps one
# element per input, so the two sequences stay index-aligned
```

A `None` slot means "no record emitted": the file exists and was read,
but there was nothing to parse. `bca.to_sarif` skips those entries, so
a batch list can be handed to it verbatim.

`bca.AnalysisFailure` is a frozen value type with `path: str`,
`error: str`, and `error_kind: Literal["UnsupportedLanguage",
"ParseError", "IoError"]`. It implements `__eq__`, `__hash__`,
and `__repr__`, so callers can put errors in a `set` to
deduplicate failures across runs. It is **not** an `Exception`
subclass; `analyze_batch` returns it, never raises it.

`analyze_batch` only raises on **programmer** errors: `TypeError`
for a non-iterable `paths` argument (or a non-path element
inside), and `ValueError` for an empty `metrics=` list or an
unknown metric name. The `metrics=` selection (see
[Selecting metrics](#selecting-metrics) above) applies uniformly
to every file in the batch; validation runs **before** the input
iterable's `__iter__` so a bad selection aborts without invoking
any side effects.

`analyze_batch` analyzes an explicit list of paths verbatim. To
*discover* the files first, `bca.analyze_paths(*seeds, include=…,
exclude=…, respect_gitignore=True, …)` walks file or directory seeds
with the CLI's gitignore-aware walker and returns the same
`list[dict | AnalysisFailure | None]` shape; see the book's
[Batch processing](https://dekobon.github.io/big-code-analysis/python/batch.html)
page.

Generators work, since paths are consumed lazily. There is no
built-in parallelism; the recommended pattern is
`concurrent.futures.ThreadPoolExecutor` around `bca.analyze` for
parallel single-file calls.

## Flatten to records

`bca.flatten_spaces(result)` walks the nested `FuncSpace` tree in
pre-order and yields one flat, scalar-only `dict` per node, ready
for `sqlite3.executemany`, `pandas.DataFrame.from_records`, or any
other tabular consumer. Metric keys use the same dotted convention
as the CLI's CSV writer (`cyclomatic.modified.sum`,
`halstead.volume`, `loc.lloc_average`, and so on).

The schema differs from the CLI's `CSV_HEADER` in three ways. Metric
*columns* are a superset: the per-space `cognitive.value`,
`cyclomatic.value`, `cyclomatic.modified.value`, `abc.value` and
`nargs.value` leaves (the numbers `bca check` thresholds against) are
in the JSON shape and in these records but not in the CSV header.
The identity columns differ: CSV uses `space_name` / `space_kind` and
has no `parent_name` / `depth`, while flat records use `name` / `kind`
and add the parent / depth pair. One metric leaf diverges: `tokens.*`
flattens to the JSON shape (`tokens.tokens`, `tokens.average`,
`tokens.min`, `tokens.max`); only the sum leaf differs from
`CSV_HEADER`, which spells it `tokens.sum`, while the `average` /
`min` / `max` leaves now match (#590). Rename the sum leaf in the
consumer if you need exact CSV alignment.

```python
import sqlite3
import big_code_analysis as bca

result = bca.analyze("src/lib.rs")
if result is None:  # generated/skipped file
    raise SystemExit("nothing to analyze")
records = list(bca.flatten_spaces(result))
columns = sorted({k for r in records for k in r})
# flatten_spaces keys come from a bounded alphabet (`.`, `_`,
# ASCII alnum), so f-string quoting is safe here. Sanitize if you
# ever build records by hand.
with sqlite3.connect("metrics.db") as db:
    cols = ", ".join(f'"{c}"' for c in columns)
    qs = ", ".join("?" for _ in columns)
    db.execute(f"CREATE TABLE m ({cols})")
    db.executemany(
        f"INSERT INTO m ({cols}) VALUES ({qs})",
        [tuple(r.get(c) for c in columns) for r in records],
    )
```

The iterator is lazy and single-use: it walks the input once
without materializing the whole list, and a second iteration is
empty. Records always carry `path` (the analyzed file, or `None`
for `analyze_source`), `name`, `kind`, `start_line`, `end_line`,
`parent_name`, and `depth`. Anonymous spaces (Rust closures, JS
function expressions and arrows) keep their `name == "<anonymous>"`
marker verbatim; `flatten_spaces` does not normalize. Missing
metric subtrees produce no keys (absent, not `None`), matching the
"Halstead disabled" edge case for `metrics=` selection.

`parent_name` alone cannot disambiguate same-named siblings nested
under different parents (two `Inner` classes under two different
outer classes both surface as `parent_name == 'Inner'` for their
own children). Pair it with `depth` and source-order position, or
rebuild the qualified name in your consumer, if you need a
fully-qualified path.

Do not mutate the input `result` while iterating: the walker keeps
references into it, so mutations to not-yet-yielded subtrees show
up in later records.

`flatten_spaces` raises `TypeError` if the input is not a mapping.
Filter out the `None` returns from `bca.analyze` (a generated file
under `skip_generated=True`) before passing, and likewise the `None`
slots `analyze_batch` emits under `skip_generated=False`.

## Errors

`bca.analyze` raises exceptions; `bca.analyze_batch` returns
`bca.AnalysisFailure` values inside the result list and never raises
on per-file failures (see Batch processing above).

Exception types raised by `bca.analyze` / `bca.analyze_source`:

- `bca.UnsupportedLanguageError` (subclass of `ValueError`): neither
  the file extension, an Emacs or Vim mode line, nor the shebang
  identifies a supported language; or `analyze_source` receives an
  unknown language name.
- `bca.ParseError` (subclass of `ValueError`): the underlying
  tree-sitter parser failed on the supplied source.
- `ValueError`: raised by `bca.analyze` when the path is not valid
  UTF-8 and the default strict policy is in effect. Pass
  `allow_lossy_path=True` to mirror the CLI's U+FFFD substitution
  via `Path::to_string_lossy` and accept the resulting
  non-round-trippable `name` field (#316).
- `OSError`: bubbled up from the underlying file-system read.
  Dispatches to the canonical subclass (`FileNotFoundError`,
  `PermissionError`, `IsADirectoryError`, …) based on `errno`,
  with `err.errno` and `err.filename` populated.

Exception types raised by the change-history surface
(`bca.vcs.rank` / `bca.vcs.trend` / `bca.vcs.commit` /
`bca.vcs.score_diff`, and `bca.analyze(..., vcs=True)` for its
option parsing). All subclass `bca.VcsError`, itself a `ValueError`,
so existing `except ValueError` handlers keep working (#624):

- `bca.NotARepositoryError`: the path is not inside a supported VCS
  working tree. The variant to branch on for "not a repo, skip this
  directory". (`analyze(..., vcs=True)` never raises it; a
  non-repository file just yields no `vcs` block.)
- `bca.InvalidRevisionError`: a `reference` / `commit` could not be
  resolved (`vcs.commit`).
- `bca.InvalidDiffError`: the `diff` passed to `vcs.score_diff` is
  malformed.
- `bca.VcsEnvironmentError`: an environment or backend failure
  (opening the repository, walking history, diffing, `.mailmap`,
  blame, or cache I/O). Mirrors the `500` (rather than `400`)
  responses the web crate returns for the same errors.
- `bca.VcsError`: the base; raised directly for client-input option
  failures (a malformed window / timestamp / formula / file-type scope
  / bus-factor threshold / bot pattern / trend point count), where the
  message names the offending value.

Returned by `bca.analyze_batch` inside the result list:

- `bca.AnalysisFailure`: frozen value type with `path: str`,
  `error: str`, and `error_kind: Literal["UnsupportedLanguage",
  "ParseError", "IoError"]`. Not an `Exception` subclass.
  `error_kind` is a closed taxonomy: ``"IoError"`` covers both
  filesystem failures and the non-UTF-8 path case (kept at three
  kinds per the API contract); ``"ParseError"`` similarly covers
  internal JSON-serialization failures of the resulting
  `FuncSpace` (rare; reserved upstream). The OS `errno` is
  preserved in the `error` string via Rust's ``"<msg> (os error
  <N>)"`` default formatting. Parse it with the regex
  ``r"\(os error (\d+)\)$"`` if you need it for retry
  classification, or call `bca.analyze` per file to get a typed
  `OSError` subclass instead.

## Type checking

The package ships [PEP 561](https://peps.python.org/pep-0561/) type
stubs (`py.typed` + `_native.pyi`). `mypy --strict` and `pyright`
both pass against client code.

## License

MPL-2.0 (matches the Rust library).
