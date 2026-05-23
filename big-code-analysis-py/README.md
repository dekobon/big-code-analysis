# big-code-analysis (Python bindings)

Python bindings for the
[`big-code-analysis`](https://github.com/dekobon/big-code-analysis)
Rust library — compute maintainability metrics for source code in
~20 languages using the same tree-sitter parsers the Rust crate
ships with.

This is **phase 1+2** of the Python bindings work
(issues #265, #266; parent #103): single-file analysis plus the
never-raise batch entry point. `flatten_spaces`, SARIF rendering,
and explicit metric selection land in follow-up phases.

## Installation

The package is not yet published on PyPI. For development, build
locally via [maturin](https://www.maturin.rs/):

```bash
cd big-code-analysis-py
uv venv .venv && source .venv/bin/activate
uv pip install maturin pytest mypy
maturin develop
python -c "import big_code_analysis; print(big_code_analysis.__version__)"
```

## Usage

```python
import big_code_analysis as bca

# Analyse a file by path. The returned dict matches the JSON
# emitted by `bca metrics --output-format json` for the same
# file at the `FuncSpace` boundary — same field order, same
# numeric formatting, same shape. Language detection mirrors the
# CLI (path extension, then shebang, then emacs `-*- mode -*-`).
# Pass `exclude_tests=True` to mirror `bca metrics --exclude-tests`
# (prunes Rust `#[test]` / `#[cfg(test)]` subtrees before metric
# computation). Generated files (`@generated`, `DO NOT EDIT`,
# `GENERATED CODE` markers) are skipped by default, matching the
# CLI walker — `analyze` returns `None` for them; pass
# `skip_generated=False` to opt out. See `bca.analyze.__doc__`
# for the full parity contract.
result = bca.analyze("src/main.rs")
if result is not None:
    print(result["metrics"]["cognitive"]["sum"])

# Analyse a Rust file with `#[test]` subtrees pruned out — same
# result as `bca metrics --exclude-tests --output-format json`.
prod_only = bca.analyze("src/main.rs", exclude_tests=True)

# Non-UTF-8 paths raise `ValueError` by default so the `name`
# field is always a round-trippable identifier. Pass
# `allow_lossy_path=True` to opt into the CLI's U+FFFD
# substitution behaviour (see `bca.analyze.__doc__` and #316).
lossy = bca.analyze(weird_path, allow_lossy_path=True)

# Force analysis of files marked `@generated` (default skips them).
forced = bca.analyze("third_party/generated.pb.go", skip_generated=False)

# Analyse an in-memory snippet (str, bytes, or bytearray accepted).
metrics = bca.analyze_source("fn main() {}\n", "rust")

# Language detection helpers.
assert bca.language_for_file("foo.py") == "python"
assert "python" in bca.supported_languages()
assert "py" in bca.language_extensions("python")
```

## Batch processing

`bca.analyze_batch(paths)` runs the same analysis as `bca.analyze`
over every path in an iterable and **never raises on per-file
errors**: each result slot is either an analysis ``dict`` or a
`bca.AnalysisError` describing the failure. The list has the same
length as the input and preserves order one-to-one, so callers
can `zip(inputs, results)` without losing the pairing.

```python
import big_code_analysis as bca

paths = ["src/a.py", "src/missing.py", "src/b.rs"]
for path, result in zip(paths, bca.analyze_batch(paths)):
    if isinstance(result, bca.AnalysisError):
        print(f"skipped {path}: ({result.error_kind}) {result.error}")
    else:
        process(result)
```

`bca.AnalysisError` is a frozen value type with `path: str`,
`error: str`, and `error_kind: Literal["UnsupportedLanguage",
"ParseError", "IoError"]`. It implements `__eq__`, `__hash__`,
and `__repr__`, so callers can put errors in a `set` to
deduplicate failures across runs. It is **not** an `Exception`
subclass — `analyze_batch` returns it, never raises it.

`analyze_batch` only raises on **programmer** errors: `TypeError`
for a non-iterable `paths` argument (or a non-path element
inside), `ValueError` for an explicitly empty `metrics=` list.
The `metrics=` kwarg is accepted today (and validated) but the
selection itself lands in a later phase; passing `None` (the
default) is the supported choice for now.

Generators work — paths are consumed lazily. There is no
built-in parallelism; the recommended pattern is
`concurrent.futures.ThreadPoolExecutor` around `bca.analyze` for
parallel single-file calls. `analyze_batch` also runs with the
`is_generated` walker filter **off** so every input position
yields either a `dict` or an `AnalysisError` (never `None`).
Call `bca.analyze(path)` per-file with the default
`skip_generated=True` if you need the CLI walker's skip behaviour.

## Errors

- `bca.UnsupportedLanguageError` (subclass of `ValueError`) —
  raised when a file extension is unrecognised, or when
  `analyze_source(..., language="...")` is passed an unknown
  language name.
- `bca.ParseError` (subclass of `ValueError`) — raised when the
  underlying tree-sitter parser fails on the supplied source.
- `ValueError` — raised by `bca.analyze` when the path is not
  valid UTF-8 and the default strict policy is in effect; pass
  `allow_lossy_path=True` to mirror the CLI's U+FFFD substitution
  via `Path::to_string_lossy` and accept the resulting
  non-round-trippable `name` field (#316).
- `OSError` — bubbled up from the underlying file-system read.

## Type checking

The package ships PEP 561 type stubs (`py.typed` + `_native.pyi`).
`mypy --strict` and `pyright` should both pass cleanly against
client code.

## License

MPL-2.0 (matches the Rust library).
