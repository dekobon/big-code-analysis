# big-code-analysis

> This documentation is also available in [Japanese](ja/).

**big-code-analysis** is a Rust library and command line tool (`bca`)
that computes maintainability metrics from source code in many
programming languages. It parses with
<a href="https://tree-sitter.github.io/tree-sitter/" target="_blank">tree-sitter</a>,
an incremental parsing library with a grammar per language, so it needs
no compiler or language runtime.

This book is task-oriented: metric definitions, the command reference,
CI recipes, and guides for the Rust crate and the Python bindings. The
source code is on
<a href="https://github.com/dekobon/big-code-analysis/" target="_blank">GitHub</a>,
and issues and feature requests go to the
<a href="https://github.com/dekobon/big-code-analysis/issues/" target="_blank">issue tracker</a>.

## Supported platforms

**big-code-analysis** runs on Linux, macOS, and Windows. The
<a href="https://github.com/dekobon/big-code-analysis/releases" target="_blank">GitHub releases page</a>
carries prebuilt binaries for all three.

## API docs

The complete
<a href="https://doc.rust-lang.org/rustdoc/" target="_blank">Rustdoc</a>
reference for the crate (every public type, trait,
and function, including the feature-gated `vcs` module) lives on
<a href="https://docs.rs/big-code-analysis/*/big_code_analysis/" target="_blank">docs.rs</a>,
generated straight from the source.

For task-oriented guides on embedding the crate (quick start,
in-memory analysis, walking `FuncSpace` results, and error
handling), see [Using as a Library](library/index.html).

For the <a href="https://pyo3.rs/" target="_blank">PyO3</a> bindings
(`pip install big-code-analysis`, batch processing, flat-record
iteration,
<a href="https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html" target="_blank">SARIF</a>
output, and async patterns), see [Python Bindings](python/index.html).

## License

- The vendored grammar crates (`tree-sitter-ccomment`,
  `tree-sitter-mozcpp`, `tree-sitter-mozjs`, `tree-sitter-preproc`,
  `tree-sitter-tcl`) are released under the
  <a href="https://opensource.org/license/mit" target="_blank">MIT license</a>.

- **big-code-analysis**, **big-code-analysis-ast**, **big-code-analysis-cli**,
  **big-code-analysis-web**, and **big-code-analysis-py**
  are released under the
  <a href="https://www.mozilla.org/MPL/2.0/" target="_blank">Mozilla Public License v2.0</a>.
