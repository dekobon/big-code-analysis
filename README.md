# big-code-analysis

[![Crates.io](https://img.shields.io/crates/v/big-code-analysis.svg)](https://crates.io/crates/big-code-analysis)
[![Task Status](https://community-tc.services.mozilla.com/api/github/v1/repository/dekobon/big-code-analysis/master/badge.svg)](https://community-tc.services.mozilla.com/api/github/v1/repository/dekobon/big-code-analysis/master/latest)
[![codecov](https://codecov.io/gh/dekobon/big-code-analysis/branch/master/graph/badge.svg)](https://codecov.io/gh/dekobon/big-code-analysis)
<a href="https://chat.mozilla.org/#/room/#big-code-analysis:mozilla.org" target="_blank">
   <img src="https://img.shields.io/badge/chat%20on%20[m]-%23rust--code--analysis%3Amozilla.org-blue">
</a>

**big-code-analysis** is a hard fork of the [rust-code-analysis](https://github.com/mozilla/rust-code-analysis) project.
This project is an unapologetic vibe-coded fork that seeks to add as many features and functions as fast as possible.

Nonetheless, it is still a Rust library to analyze and extract information
from source code written in many different programming languages.
It is based on a parser generator tool and an incremental parsing library
called
<a href="https://tree-sitter.github.io/tree-sitter/" target="_blank">Tree Sitter</a>.

A command line tool called **big-code-analysis-cli** is provided to interact with the API of the library in an easy way.

This tool can be used to:

- Call **big-code-analysis** API
- Print nodes and metrics information
- Export metrics in different formats
- Generate a Markdown quality-metrics report (`-O markdown`)

In addition, we provide a **big-code-analysis-web** tool to use the library through a REST API.

# Usage

**big-code-analysis** supports many types of programming languages and
computes a great variety of metrics. You can find up to date documentation at
<a href="https://dekobon.github.io/big-code-analysis/index.html" target="_blank">Documentation</a>.

On the
<a href="https://dekobon.github.io/big-code-analysis/commands/index.html" target="_blank">
    Commands
</a> page, there is a list of commands that can be run to get information
about metrics, nodes, and other general data provided by this software.

## Building

To build the `big-code-analysis` library, you need to run the following
command:

```console
cargo build
```

If you want to build the `cli`:

```console
cargo build -p big-code-analysis-cli
```

If you want to build the `web` server:

```console
cargo build -p big-code-analysis-web
```

If you want to build everything in one fell swoop:

```console
cargo build --workspace
```

## Testing

To verify whether all tests pass, run the `cargo test` command.

```console
cargo test --workspace --all-features --verbose
```

### Updating insta tests

We use [insta](https://insta.rs), to update the snapshot tests you should install [cargo insta](https://crates.io/crates/cargo-insta)

``` console
cargo insta test --review
```

Will run the tests, generate the new snapshot references and let you review them.

### Updating grammars

Have a look at
<a href="https://dekobon.github.io/big-code-analysis/developers/update-grammars.html" target="_blank">Update grammars guide</a>
to learn how to update languages grammars.

# Contributing

If you want to contribute to the development of this software, have a look at the
guidelines contained in our
<a href="https://dekobon.github.io/big-code-analysis/developers/index.html" target="_blank">Developers Guide</a>.

# Licenses

- Mozilla-defined grammars are released under the MIT license.

- **big-code-analysis**, **big-code-analysis-cli** and **big-code-analysis-web**
are released under the
<a href="https://www.mozilla.org/MPL/2.0/" target="_blank">Mozilla Public License v2.0</a>.
