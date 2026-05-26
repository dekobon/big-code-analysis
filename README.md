# big-code-analysis

[![crates.io](https://img.shields.io/crates/v/big-code-analysis.svg)](https://crates.io/crates/big-code-analysis)
[![MSRV](https://img.shields.io/crates/msrv/big-code-analysis.svg)](Cargo.toml)
[![CI](https://github.com/dekobon/big-code-analysis/actions/workflows/ci.yml/badge.svg?branch=main&event=push)](https://github.com/dekobon/big-code-analysis/actions/workflows/ci.yml?query=branch%3Amain+event%3Apush)
[![CodeQL](https://github.com/dekobon/big-code-analysis/actions/workflows/codeql.yml/badge.svg?branch=main)](https://github.com/dekobon/big-code-analysis/actions/workflows/codeql.yml?query=branch%3Amain)
[![docs.rs](https://docs.rs/big-code-analysis/badge.svg)](https://docs.rs/big-code-analysis)
[![License](https://img.shields.io/crates/l/big-code-analysis.svg)](LICENSE)

**big-code-analysis** is a hard fork of the [rust-code-analysis](https://github.com/mozilla/rust-code-analysis) project.
This project is an unapologetic vibe-coded fork that seeks to add as many features and functions as fast as possible.

Nonetheless, it is still a Rust library to analyze and extract information
from source code written in many different programming languages.
It is based on a parser generator tool and an incremental parsing library
called
<a href="https://tree-sitter.github.io/tree-sitter/" target="_blank">Tree Sitter</a>.

A command line tool called **bca** is provided to interact with the API of the library in an easy way.

This tool can be used to:

- Call **big-code-analysis** API
- Print nodes and metrics information
- Export metrics in different formats
- Generate a Markdown or HTML quality-metrics report (`bca report markdown` / `bca report html`)

In addition, we provide a **bca-web** tool to use the library through a REST API.

## Live example reports

`bca` runs against its own source on every push to `main` and publishes
the result alongside the documentation:

- HTML hotspot report:
  <https://dekobon.github.io/big-code-analysis/reports/index.html>
- Markdown PR/MR comment:
  <https://dekobon.github.io/big-code-analysis/reports/report.md>

The wiring lives in
[`.github/workflows/pages.yml`](.github/workflows/pages.yml); see the
book's [CI integration recipe](https://dekobon.github.io/big-code-analysis/recipes/ci.html)
for adapting it to your own project.

## Usage

**big-code-analysis** supports many types of programming languages and
computes a great variety of metrics. You can find up to date documentation at
<a href="https://dekobon.github.io/big-code-analysis/index.html" target="_blank">Documentation</a>.

On the
<a href="https://dekobon.github.io/big-code-analysis/commands/index.html" target="_blank">
    Commands
</a> page, there is a list of commands that can be run to get information
about metrics, nodes, and other general data provided by this software.

## Using as a library

`big-code-analysis` is published on crates.io and can be embedded
directly. The crate is on the `1.x` line and ships under a written
stability contract: the public API surface is held stable across
patch and minor bumps, and breaking shape changes are reserved for
the next major bump. Metric *values* may still drift across minor
bumps when a grammar pin moves or a metric definition is fixed —
see [STABILITY.md](./STABILITY.md) for the full versioning contract,
MSRV policy, escape hatches, and exactly what we do and do not
promise within `1.x`.

For task-oriented walkthroughs — quick start, in-memory analysis,
walking `FuncSpace` results, and error handling — see the
[**Using as a Library**](https://dekobon.github.io/big-code-analysis/library/index.html)
section of the book.

Python bindings (PyO3) live in
[`big-code-analysis-py/`](./big-code-analysis-py/README.md) and ship
the same metric pipeline as a Python package. See the book's
[**Python Bindings**](https://dekobon.github.io/big-code-analysis/python/index.html)
section for the install matrix, batch / async / SARIF recipes, and
the full error taxonomy.

### Per-language Cargo features

Every tree-sitter grammar is gated behind a per-language Cargo
feature. The default feature set is `all-languages`, so a bare

```toml
big-code-analysis = "1.1.0"
```

pulls every grammar in (matching the library's historical behaviour
and what the `bca` / `bca-web` binaries ship). Library consumers that
only need a subset of languages can opt out of the defaults and
re-enable just the grammars they want:

```toml
big-code-analysis = { version = "1.1.0", default-features = false, features = ["rust", "typescript"] }
```

Supported language features: `bash`, `cpp`, `csharp`, `elixir`,
`go`, `groovy`, `java`, `javascript`, `kotlin`, `lua`, `mozjs`,
`perl`, `php`, `python`, `ruby`, `rust`, `tcl`, `typescript`. The
`cpp` feature covers the `Cpp`, `Ccomment`, and `Preproc` LANG
variants and pulls in `bca-tree-sitter-mozcpp`,
`bca-tree-sitter-ccomment`, and `bca-tree-sitter-preproc` together
(published forks of the matching Mozilla grammars — see the publish
strategy notes in `RELEASING.md`).

The `LANG` enum keeps every variant defined regardless of the active
feature set; selecting a [`LANG`] variant whose feature is off
returns `Err(MetricsError::LanguageDisabled(LANG))` from every
dispatch entry point (`analyze`, `metrics_from_tree`, `action`,
`get_ops`, the deprecated `get_function_spaces*` shims, and
`LANG::get_tree_sitter_language`). The set of compiled-in variants
is queryable via `LANG::is_enabled`.

## Building

The repository ships a `Makefile` that wraps every common build, test,
lint, and docs task. Run `make help` for the full list, and
`make check-tools` to verify the optional tools are installed.

```console
make build           # debug build of the entire workspace
make build-release   # optimised release build
```

If you prefer to run cargo directly, or want to build a single crate:

```console
cargo build                              # library only
cargo build -p big-code-analysis-cli     # CLI only
cargo build -p big-code-analysis-web     # web server only
cargo build --workspace                  # everything in one shot
```

## Testing

```console
make test           # cargo test --workspace --all-features --lib --bins --tests
make test-doc      # cargo test --workspace --all-features --doc
make pre-commit    # full local gate: fmt-check, clippy, tests, udeps, lint families
```

`make pre-commit` is the recommended gate before committing — it is
equivalent to what CI runs. If GNU Make 4 or any of the optional
tools are unavailable, the raw cargo invocation still works:

```console
cargo test --workspace --all-features --verbose
```

### Updating insta tests

We use [insta](https://insta.rs), to update the snapshot tests you should install [cargo insta](https://crates.io/crates/cargo-insta)

```console
make insta-review   # cargo insta test --review
```

Will run the tests, generate the new snapshot references and let you review them.

### Updating grammars

Have a look at
<a href="https://dekobon.github.io/big-code-analysis/developers/update-grammars.html" target="_blank">Update grammars guide</a>
to learn how to update languages grammars.

## Contributing

If you want to contribute to the development of this software, have a look at the
guidelines contained in our
<a href="https://dekobon.github.io/big-code-analysis/developers/index.html" target="_blank">Developers Guide</a>.

## Licenses

- Mozilla-defined grammars are released under the MIT license.

- **big-code-analysis**, **big-code-analysis-cli** and **big-code-analysis-web**
are released under the
<a href="https://www.mozilla.org/MPL/2.0/" target="_blank">Mozilla Public License v2.0</a>.
