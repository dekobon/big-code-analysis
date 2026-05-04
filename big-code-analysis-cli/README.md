# big-code-analysis-cli

`big-code-analysis-cli` analyzes source code and emits per-file structured
metrics, aggregated reports, AST dumps, node lookups, and more.

> **Migrating from the flag-style CLI?** The CLI is now subcommand-driven.
> See the [migration guide](../big-code-analysis-book/src/migration.md)
> for old-form -> new-form mappings of every flag.

## Installation

```sh
cd big-code-analysis-cli/
cargo build
```

## Usage

```sh
big-code-analysis-cli [GLOBAL OPTIONS] <COMMAND> [COMMAND OPTIONS]
```

The global options describe *what to walk* (paths, includes/excludes,
parallelism, language overrides). The command picks *what to do* with each
file, with command-specific options as needed.

## Commands

| Command | Purpose |
| --- | --- |
| `metrics` | Per-file metric output (`-O json/yaml/toml/cbor`, `-o DIR`). |
| `ops` | Per-file operand/operator output (same formats as `metrics`). |
| `report <FORMAT>` | Aggregated report. `markdown` today; `html` reserved. |
| `dump` | AST dump to stdout. |
| `find <NODE>...` | Find nodes of one or more types. |
| `count <NODE>...` | Count nodes of one or more types. |
| `functions` | List functions/methods and their spans. |
| `strip-comments` | Remove comments from source files (`--in-place`). |
| `preproc` | Build preprocessor-data JSON for C/C++ analysis. |
| `list-metrics [names\|descriptions]` | List computable metrics. |

Run `big-code-analysis-cli <COMMAND> --help` for command-specific options.

## Global options

- `-p, --paths <FILE>...` — input files or directories.
- `-I, --include [<GLOB>...]` — include files matching pattern.
- `-X, --exclude [<GLOB>...]` — exclude files matching pattern.
- `-j, --num-jobs <N>` — worker threads.
- `-l, --language-type <LANG>` — force a language instead of inferring.
- `--ls <LINE_START>` / `--le <LINE_END>` — line range (used by `dump`,
  `find`).
- `-w, --warning` — print warnings (skipped files, unrecognized
  languages).
- `--preproc-data <FILE>` — consume an existing preproc JSON during C/C++
  analysis. Build one with `bca preproc`.

Global options work both before and after the subcommand.

## Examples

Per-file JSON metrics:

```sh
big-code-analysis-cli --paths ./src metrics -O json -o ./out/
```

Aggregated markdown quality report:

```sh
big-code-analysis-cli --paths "$PWD" --num-jobs $(nproc) \
    report markdown --top 20 --strip-prefix "$PWD/"
```

AST dump for one file:

```sh
big-code-analysis-cli --paths ./file.rs dump
```

List all metrics with one-line descriptions:

```sh
big-code-analysis-cli list-metrics descriptions
```
