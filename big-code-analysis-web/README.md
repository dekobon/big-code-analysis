# big-code-analysis-web

`bca-web` is the HTTP daemon of the
[big-code-analysis](https://github.com/dekobon/big-code-analysis)
workspace. It exposes the library's analysis over a REST API so a
long-running client (an editor plugin, a CI worker, a web app) can
analyse source without spawning the `bca` CLI per file.

## Endpoints

- **Comment removal**: strips comments from submitted source.
- **Function spans**: returns the start and end lines of each function.
- **AST dump**: returns the [tree-sitter](https://tree-sitter.github.io/tree-sitter/)
  syntax tree for the submitted source.
- **Metrics**: computes the maintainability metrics for the submitted source.
- **Change-history (VCS) metrics**: ranks files, samples trends, and scores
  commits or diffs over a git repository on the server's filesystem.

The [REST API reference][rest-api] documents every endpoint, its
request and response shapes, and the error contract.

[rest-api]: https://dekobon.github.io/big-code-analysis/commands/rest.html

## Installation

Clone the repository and build the crate from the workspace root:

```sh
cargo build -p big-code-analysis-web
```

Or install the `bca-web` binary from crates.io:

```sh
cargo install big-code-analysis-web
```

## Building with a subset of languages

The shipped `bca-web` binary compiles every supported tree-sitter
grammar in. The `big-code-analysis-web` crate pins the library's
`all-languages` feature set explicitly, so passing
`--no-default-features` or a custom `--features` list to
`cargo build -p big-code-analysis-web` does **not** drop grammars
from the resulting binary. Feature selection on the web crate is
not honoured, because dropping a grammar silently from a
user-facing daemon would surface as "language X stopped working"
rather than as a build error ([#252][issue-252]).

If you need a reduced feature set, embed the `big-code-analysis`
library in your own Rust code and select features in your own
`Cargo.toml`. The library's [per-language Cargo features][cargo-features]
chapter lists every feature with a worked example.

[cargo-features]: https://dekobon.github.io/big-code-analysis/library/cargo-features.html
[issue-252]: https://github.com/dekobon/big-code-analysis/issues/252

## Usage

```sh
bca-web [OPTIONS]
```

### Options

- `-j, --num-jobs <N|auto>`: number of worker threads, a positive
  integer or `auto` (the default). `auto` resolves to the OS-reported
  effective CPU count (cgroup-quota- and cpuset-aware on Linux),
  matching the `bca` CLI's `--jobs`.
- `--host <HOST>`: address to bind (default `127.0.0.1`).
- `-p, --port <PORT>`: TCP port (default `8080`).
- `--parse-timeout-secs <SECS>`: per-parse timeout in seconds (default
  `30`). `0` disables the timeout. A request whose parse exceeds this
  deadline returns `504 Gateway Timeout`.
- `--cors <ORIGINS>`: enable
  [CORS](https://developer.mozilla.org/en-US/docs/Web/HTTP/CORS) for a
  comma-separated origin allow-list
  (off by default).
- `-h, --help`: print help and exit.
- `-V, --version`: print the version and exit.

For the environment variables (`BCA_MAX_ORPHANED_TASKS`, `RUST_LOG`),
the request-body and orphaned-task limits, and the trust boundaries
to respect before exposing the daemon, see
[Operating bca-web][operating]. The server has no authentication of
its own and the VCS endpoints read server-side repositories, so keep
the default loopback bind or put an authorizing proxy in front.

[operating]: https://dekobon.github.io/big-code-analysis/commands/web-server.html

## Example

Start the server on a specific host and port:

```sh
bca-web --host <HOST> --port <PORT> -j <NUM_JOBS>
```
