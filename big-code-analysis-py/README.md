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

# Analyse a file by path; result mirrors `bca metrics --output json`.
metrics = bca.analyze("src/main.rs")
print(metrics["metrics"]["cognitive"]["cognitive_sum"])

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
- `OSError` — bubbled up from the underlying file-system read.

## Type checking

The package ships PEP 561 type stubs (`py.typed` + `_native.pyi`).
`mypy --strict` and `pyright` should both pass cleanly against
client code.

## License

MPL-2.0 (matches the Rust library).
