# big-code-analysis (Python bindings)

Python bindings for the
[`big-code-analysis`](https://github.com/dekobon/big-code-analysis)
Rust library — compute maintainability metrics for source code in
~20 languages using the same tree-sitter parsers the Rust crate
ships with.

This is **phase 1** of the Python bindings work (issue #265,
parent #103): the single-file analysis API. Batch analysis,
`flatten_spaces`, SARIF rendering, and explicit metric selection
land in follow-up phases.

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
# computation). The remaining CLI-only behaviour (the
# `is_generated` filter) is deferred to a phase-1 follow-up;
# see `bca.analyze.__doc__` for the full parity contract.
metrics = bca.analyze("src/main.rs")
print(metrics["metrics"]["cognitive"]["sum"])

# Analyse a Rust file with `#[test]` subtrees pruned out — same
# result as `bca metrics --exclude-tests --output-format json`.
prod_only = bca.analyze("src/main.rs", exclude_tests=True)

# Non-UTF-8 paths raise `ValueError` by default so the `name`
# field is always a round-trippable identifier. Pass
# `allow_lossy_path=True` to opt into the CLI's U+FFFD
# substitution behaviour (see `bca.analyze.__doc__` and #316).
lossy = bca.analyze(weird_path, allow_lossy_path=True)

# Analyse an in-memory snippet (str, bytes, or bytearray accepted).
metrics = bca.analyze_source("fn main() {}\n", "rust")

# Language detection helpers.
assert bca.language_for_file("foo.py") == "python"
assert "python" in bca.supported_languages()
assert "py" in bca.language_extensions("python")
```

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
