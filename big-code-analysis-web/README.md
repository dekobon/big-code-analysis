# big-code-analysis-web

`bca-web` is a web server that provides source code analysis
capabilities via a RESTful API. It enables developers to interact
with the code analysis functionality from the big-code-analysis suite
through HTTP requests.

## Features

- **Comment Removal**: Removes comments from source code to provide a cleaner version of the code.
- **Function Spans**: Retrieves the start and end lines of functions in the given source code.
- **Metrics Calculation**: Computes static analysis metrics for the source code.

Refer to the REST API documentation for detailed information about the available endpoints and parameters.

## Installation

Clone the repository and build the project:

```sh
cd big-code-analysis-web/
cargo build
```

## Building with a subset of languages

The shipped `bca-web` binary compiles every supported tree-sitter
grammar in. The `big-code-analysis-web` crate pins the library's
`all-languages` feature set explicitly, so passing
`--no-default-features` or a custom `--features` list to
`cargo build -p big-code-analysis-web` does **not** drop grammars
from the resulting binary — feature selection on the web crate is
not honoured (see [#252][issue-252] for the rationale: dropping a
grammar silently from a user-facing daemon would surface as
"language X stopped working" rather than a build error).

Consumers who need a reduced feature set should embed the
`big-code-analysis` library in their own Rust code and control
feature selection in their own `Cargo.toml`. See the library's
[per-language Cargo features][cargo-features] chapter for the full
list of features and a worked example.

[cargo-features]: https://dekobon.github.io/big-code-analysis/library/cargo-features.html
[issue-252]: https://github.com/dekobon/big-code-analysis/issues/252

## Usage

Run the server by specifying the host and port:

```sh
bca-web [OPTIONS]
```

### Available Options

- `-j, --num-jobs <NUM_JOBS>`: Number of parallel jobs to run (optional).
- `--host <HOST>`: IP address where the server should run (default is 127.0.0.1).
- `--port <PORT>`: Port to be used by the server (default is 8080).
- `-h, --help`: Show help information.
- `-v, --version`: Show version information.

## Examples

To start the server on a specific host and port:

```sh
bca-web --host <HOST> --port <PORT> -j <NUM_JOBS>
```
