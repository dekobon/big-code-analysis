# big-code-analysis-cli

`bca` is the command-line front end for the
[big-code-analysis](https://crates.io/crates/big-code-analysis) library.
It analyzes source code and emits per-file structured metrics,
aggregated reports, threshold checks, AST dumps, and node lookups. The
full reference is the
[user guide](https://dekobon.github.io/big-code-analysis/commands/index.html).

> **Migrating from the flag-style CLI?** The CLI is now subcommand-driven.
> See the [migration guide](../big-code-analysis-book/src/migration.md)
> for old-form -> new-form mappings of every flag.

## Installation

### From [PyPI](https://pypi.org/) (pip)

Install a prebuilt wheel on Linux, macOS, or Windows without a Rust
toolchain:

```sh
pip install big-code-analysis-cli   # installs the `bca` command on PATH
bca --version
```

The [distribution name](https://pypi.org/project/big-code-analysis-cli/)
and the command name differ on purpose: you
`pip install big-code-analysis-cli`, but the installed executable is
`bca`. The `bca` name on PyPI belongs to an unrelated project, and
`big-code-analysis` is this project's importable *library* bindings
(`pip install big-code-analysis`), a different deliverable.

The wheel ships the full `all-languages` grammar set. A single
`py3-none-<platform>` wheel covers every CPython 3.x (and PyPy) on that
platform. Prebuilt wheels are published for Linux (`manylinux_2_28`
`x86_64` / `aarch64`), macOS (`x86_64` / `arm64`), and Windows
(`x86_64`); other platforms fall back to a source build.

### From crates.io ([Cargo](https://doc.rust-lang.org/cargo/))

```sh
cargo install big-code-analysis-cli
```

### From source

```sh
cd big-code-analysis-cli/
cargo build --release
```

## Usage

```sh
bca [OPTIONS] <COMMAND> [COMMAND OPTIONS]
```

The command picks *what to do*; its options describe *what to walk*
(paths, includes and excludes, parallelism, language overrides) and how
to format the result. Input-selection and walker-tuning flags are
**scoped to the subcommand that consumes them** and must be written
**after** the subcommand token. Only `-w` / `--warnings` and
`--report-skipped` are universal and accepted in any position.

## Commands

| Command | Purpose |
| --- | --- |
| `metrics` | Per-file metric output (`-O cbor/csv/json/toml/yaml`, `--output-dir DIR`). |
| `ops` | Per-file operand/operator output (same formats as `metrics`). |
| `report <FORMAT>` | Aggregated report (`markdown` or `html`). |
| `check` | Check per-function metrics against thresholds; exits 2 on threshold violations. |
| `dump` | AST dump to stdout (`--line-start`/`--line-end` to scope a range). |
| `find -t <NODE>...` | Find nodes of one or more types, one `-t`/`--type` per type (`--line-start`/`--line-end` to scope a range). |
| `count -t <NODE>...` | Count nodes of one or more types, one `-t`/`--type` per type. |
| `functions` | List functions/methods and their spans. |
| `strip-comments` | Remove comments from source files (`--in-place` or `--output FILE`). |
| `preproc` | Build preprocessor-data JSON for C/C++ analysis. |
| `list-metrics [names\|descriptions]` | List computable metrics. |
| `vcs [commit\|trend]` | Rank files by change-history risk; score one commit; sample a trend. |
| `init` | Scaffold `bca.toml`, `.bcaignore`, and `.bca-baseline.toml`. |
| `diff` | Compare two metric-output runs (or `--since <ref>` against the working tree). |
| `diff-baseline` | Diff two `.bca-baseline.toml` files. |
| `exemptions` | List everything the `check` gate skips: markers, `[check.exclude]` globs, baseline entries. |

Run `bca <COMMAND> --help` for command-specific options.

## Walking options

The walking subcommands (`metrics`, `ops`, `report`, `check`, `dump`,
`find`, `count`, `functions`, `strip-comments`, `preproc`, `exemptions`,
`init`, `diff`, and the bare `vcs` ranking) accept these input-selection
and walker-tuning flags, written **after** the subcommand. A flag passed
to a subcommand that never consumes it is a hard usage error (exit 1),
not a silent no-op.

- `-p, --paths <FILE>...`: input files or directories. Most walking
  subcommands also accept paths positionally (`bca metrics src/`); the
  exceptions are `report`, `vcs`, and `init` (`--paths` only; `init`
  scaffolds into `--dir`) and `diff` (its positionals are the two
  result sets).
- `-I, --include <GLOB>`: include files matching pattern (repeatable).
- `-X, --exclude <GLOB>`: exclude files matching pattern (repeatable).
- `-j, --jobs <N|auto>`: worker threads; the default `auto` uses the
  effective CPU count (`--num-jobs` is a deprecated alias).
- `-l, --language <LANG>`: force a language instead of inferring. Accepts
  a language name (`rust`) or extension (`rs`); unknown values error out.
- `-w, --warnings`: print warnings (skipped files, unrecognized
  languages). `--warning` is a deprecated alias.
- `--no-skip-generated`: disable auto-skip of files marked as generated
  (see [Skipping generated code](#skipping-generated-code)).
- `--report-skipped`: log a `skipped (generated): <path>` line to stderr
  for every file the generated-code detector excludes.
- `--preproc-data <FILE>`: consume an existing preproc JSON during C/C++
  analysis. Build one with `bca preproc`.

`-w` / `--warnings` and `--report-skipped` are the only universal options;
they are accepted in any position, before or after the subcommand.

## Building with a subset of languages

The shipped `bca` binary compiles in every supported
[tree-sitter](https://tree-sitter.github.io/tree-sitter/) grammar. The
`big-code-analysis-cli` crate pins the library's `all-languages` feature
set explicitly, so passing `--no-default-features` or a custom
`--features` list to `cargo build -p big-code-analysis-cli` does **not**
drop grammars from the resulting binary. Feature selection on the CLI
crate is not honoured: dropping a grammar silently from a user-facing
binary would surface as "language X stopped working" rather than as a
build error ([#252][issue-252]).

To ship a reduced grammar set, embed the `big-code-analysis` library in
your own Rust code and select features in your own `Cargo.toml`. The
library's [per-language Cargo features][cargo-features] chapter lists
the features and gives a worked example.

[cargo-features]: https://dekobon.github.io/big-code-analysis/library/cargo-features.html
[issue-252]: https://github.com/dekobon/big-code-analysis/issues/252

## Examples

Per-file JSON metrics:

```sh
bca metrics --paths ./src -O json --output-dir ./out/
```

Aggregated markdown quality report:

```sh
bca report --format markdown --paths "$PWD" --jobs $(nproc) \
    --top 20 --strip-prefix "$PWD/"
```

AST dump for one file:

```sh
bca dump ./file.rs
```

List all metrics with one-line descriptions:

```sh
bca list-metrics descriptions
```

## Skipping generated code

Generated files inflate metrics for code no human will refactor. By
default, `bca` scans the first ~50 lines / 5 KiB of each file for a
generated-code marker and skips matches before parsing.

Recognized markers (case-insensitive):

- `@generated`: a marker emitted by code generators.
- `DO NOT EDIT`: a marker used by
  [Go's generated-code convention](https://go.dev/s/generatedcode) and
  other generators.
- `GENERATED CODE`: [Lizard](https://github.com/terryyin/lizard)'s
  marker, recognized for compatibility.

A marker phrase that appears only deep in the file body (past the scan
window) does not trigger the skip.

To analyze everything, pass `--no-skip-generated`. To audit which files
were excluded, pass `--report-skipped`; the CLI logs
`skipped (generated): <path>` to stderr for each file.
