# big-code-analysis

[![crates.io](https://img.shields.io/crates/v/big-code-analysis.svg)](https://crates.io/crates/big-code-analysis)
[![MSRV](https://img.shields.io/crates/msrv/big-code-analysis.svg)](Cargo.toml)
[![CI](https://github.com/dekobon/big-code-analysis/actions/workflows/ci.yml/badge.svg?branch=main&event=push)](https://github.com/dekobon/big-code-analysis/actions/workflows/ci.yml?query=branch%3Amain+event%3Apush)
[![codecov](https://codecov.io/gh/dekobon/big-code-analysis/graph/badge.svg)](https://codecov.io/gh/dekobon/big-code-analysis)
[![CodeQL](https://github.com/dekobon/big-code-analysis/actions/workflows/codeql.yml/badge.svg?branch=main)](https://github.com/dekobon/big-code-analysis/actions/workflows/codeql.yml?query=branch%3Amain)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/dekobon/big-code-analysis/badge)](https://scorecard.dev/viewer/?uri=github.com/dekobon/big-code-analysis)
[![OpenSSF Best Practices](https://www.bestpractices.dev/projects/13461/badge)](https://www.bestpractices.dev/projects/13461)
[![docs.rs](https://docs.rs/big-code-analysis/badge.svg)](https://docs.rs/big-code-analysis)
[![License](https://img.shields.io/crates/l/big-code-analysis.svg)](LICENSE)

English | [日本語](README.ja.md)

**big-code-analysis** measures how maintainable your code is. The `bca`
command line tool computes per-function metrics for
[more than twenty programming languages](https://dekobon.github.io/big-code-analysis/languages.html):
cyclomatic and
[cognitive](https://www.sonarsource.com/docs/CognitiveComplexity.pdf)
complexity,
[Halstead](https://en.wikipedia.org/wiki/Halstead_complexity_measures),
maintainability index, ABC, lines-of-code variants, and the rest of
[the metric suite](https://dekobon.github.io/big-code-analysis/metrics.html). It parses with
[tree-sitter](https://tree-sitter.github.io/tree-sitter/), so it needs
no compiler, build step, or language runtime: point it at a directory
and it prints numbers.

The project is a hard fork of Mozilla's
[rust-code-analysis](https://github.com/mozilla/rust-code-analysis)
that grows the metric engine into a code-quality toolchain:

- `bca check`: a threshold gate with baselines, in-source suppression
  markers, and CI-friendly exit codes.
- Agent feedback: violations piped back into
  [Claude Code](https://code.claude.com/docs/en/overview) or
  [opencode](https://opencode.ai/) after every edit
  ([below](#feed-metrics-to-your-coding-agent)).
- `bca report`: Markdown and HTML hotspot reports.
- `bca vcs`: change-history metrics over a git tree (churn, ownership
  dilution, bug-fix history).
- Library bindings: the same engine as a Rust crate, a
  [Python package](https://pypi.org/project/big-code-analysis/), and a
  REST server (`bca-web`).

`bca` analyses its own source on every push to `main` and publishes
the result, so you can see what a real run looks like before
installing anything:

- [**Live HTML hotspot report**](https://dekobon.github.io/big-code-analysis/reports/index.html)
  — the browsable per-file, per-function view.
- [**Live Markdown report**](https://dekobon.github.io/big-code-analysis/reports/report.md)
  — the same run as a pull-request comment.

The full documentation lives in
[**the book**](https://dekobon.github.io/big-code-analysis/): metrics
definitions, command reference, CI recipes, and library guides.

## Feed metrics to your coding agent

Coding agents write a lot of code, and nothing in their loop tells
them a function has become too complex to maintain. `bca check` closes
that loop: it checks each file the agent edits and reports the
offending functions back into the model's context the moment the edit
lands. All it needs is `bca` on `PATH` (see
[Quick start](#quick-start)) plus a few lines of config.

- **Claude Code**: a `PostToolUse` hook runs `bca check` on the edited
  file and feeds violations back to the model. This repository
  dogfoods a reference hook at
  [`.claude/hooks/bca-check.sh`](.claude/hooks/bca-check.sh).
- **opencode**: a `tool.execute.after` plugin does the same; the
  reference copy is at
  [`.opencode/plugins/bca-check.js`](.opencode/plugins/bca-check.js).

The
[agent feedback recipe](https://dekobon.github.io/big-code-analysis/recipes/agent-feedback.html)
has copy-pasteable wiring for both tools, plus the guidance block that
keeps an agent from gaming the metric instead of simplifying the code.

## Quick start

Install a prebuilt `bca` from the
[releases page](https://github.com/dekobon/big-code-analysis/releases)
(signed tarballs for Linux, macOS, and Windows, plus `.deb`, `.rpm`,
and `.apk` packages), or install it from a package registry:

```console
cargo install big-code-analysis-cli    # or: pip install big-code-analysis-cli
```

Then, from a project root:

```console
bca metrics src/main.rs      # per-function metric tree for one file
bca init                     # scaffold bca.toml, .bcaignore, .bca-baseline.toml
bca check                    # exit 2 when a function crosses a threshold
bca report -O html -o report.html
```

The [Commands](https://dekobon.github.io/big-code-analysis/commands/index.html)
chapter of the book documents every subcommand, flag, and output
format.

## Quality gates and reports in CI

`bca check` reads thresholds, baselines, and excludes from a committed
`bca.toml`, so CI, local runs, and agent hooks all gate on the same
signal. `bca report` turns the same run into a Markdown comment for a
pull request or an HTML hotspot page. This repository gates itself on
every push and publishes the result:

- HTML hotspot report:
  <https://dekobon.github.io/big-code-analysis/reports/index.html>
- Markdown PR/MR comment:
  <https://dekobon.github.io/big-code-analysis/reports/report.md>

The [CI integration recipe](https://dekobon.github.io/big-code-analysis/recipes/ci.html)
is the adoption guide: a pinned-release install with checksum
verification, ready-made GitHub Actions and GitLab CI jobs, and the
[baselines](https://dekobon.github.io/big-code-analysis/recipes/baselines.html)
and
[local threshold gates](https://dekobon.github.io/big-code-analysis/recipes/local-gates.html)
recipes for ratcheting an existing codebase.

## Use it as a library

The `big-code-analysis` crate is published on crates.io under a
written stability contract ([STABILITY.md](./STABILITY.md)): the
public API holds stable across patch and minor bumps within `2.x`,
and breaking changes wait for the next major. Metric *values* may
still drift across minor bumps when a grammar pin moves or a metric
definition is fixed; the contract spells out exactly what is and is
not promised.

```toml
[dependencies]
big-code-analysis = "2"
```

Every grammar sits behind a per-language Cargo feature; the default is
all of them, and consumers who need a subset can disable default
features and re-enable individual languages. See
[Per-language Cargo features](https://dekobon.github.io/big-code-analysis/library/cargo-features.html)
in the book, and the
[Using as a Library](https://dekobon.github.io/big-code-analysis/library/index.html)
chapter for task-oriented walkthroughs (quick start, in-memory
analysis, walking `FuncSpace` results, error handling). The API
reference is on [docs.rs](https://docs.rs/big-code-analysis).

Python bindings ([PyO3](https://pyo3.rs/)) live in
[`big-code-analysis-py/`](./big-code-analysis-py/README.md) and ship
the same metric pipeline as the
[`big-code-analysis` package on PyPI](https://pypi.org/project/big-code-analysis/).
The book's
[Python Bindings](https://dekobon.github.io/big-code-analysis/python/index.html)
chapter covers installation, batch and async processing, and
[SARIF](https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html)
output.

For a service, `bca-web` wraps the library in a REST API; see
[Operating bca-web](https://dekobon.github.io/big-code-analysis/commands/web-server.html).

## Building and contributing

The repository is a Cargo workspace with a `Makefile` wrapper for
common tasks. Run `make help` for the full list.

```console
make build        # debug build of the entire workspace
make test         # full test suite (workspace, all features)
make pre-commit   # full local gate, mirrors CI
```

[CONTRIBUTING.md](./CONTRIBUTING.md) covers the contribution workflow,
and the
[Developers Guide](https://dekobon.github.io/big-code-analysis/developers/index.html)
in the book covers internals: adding a language, implementing a
metric, and updating grammars.

## Licenses

- The vendored grammar crates (`tree-sitter-ccomment`,
  `tree-sitter-mozcpp`, `tree-sitter-mozjs`, `tree-sitter-preproc`,
  `tree-sitter-tcl`) are released under the MIT license.

- **big-code-analysis**, **big-code-analysis-cli**,
  **big-code-analysis-web**, and **big-code-analysis-py** are released
  under the
  [Mozilla Public License v2.0](https://www.mozilla.org/MPL/2.0/).
