# Commands

**bca** offers a range of **commands** to analyze and extract
information from source code. Each command **may** include parameters
specific to the task it performs. Below, we describe the core types
of commands available in **bca**.

## Installation

The `bca` command-line tool is available as a pip-installable wheel.
The **distribution name is `big-code-analysis-cli`** and the installed
**command is `bca`** — the two differ deliberately (the `bca` name on
PyPI belongs to an unrelated project, and `big-code-analysis` is this
project's importable *library* bindings):

```bash
pip install big-code-analysis-cli   # installs the `bca` command on PATH
bca --version
```

This drops the compiled `bca` binary onto your `PATH` the way
`pip install ruff` gives you the `ruff` command — no Rust toolchain
required. The wheel carries the full `all-languages` grammar set, so
every [supported language](../languages.md) works out of the box. A
single `py3-none-<platform>` wheel covers every CPython 3.x (and PyPy)
on that platform; prebuilt wheels ship for Linux (`manylinux_2_28`
`x86_64` / `aarch64`), macOS (`x86_64` / `arm64`), and Windows
(`x86_64`). On any other platform `pip` falls back to a source build,
which needs a Rust toolchain.

This is the binary CLI, distinct from the importable
[Python bindings](../python/installation.md)
(`pip install big-code-analysis`). Other install paths — Homebrew,
`.deb` / `.rpm` / `.apk` packages, prebuilt release archives, or
`cargo install big-code-analysis-cli` — are described in the
repository README.

The wheel build and publish matrix is defined in
[`.github/workflows/python-cli-wheels.yml`](https://github.com/dekobon/big-code-analysis/blob/main/.github/workflows/python-cli-wheels.yml).

## Exit codes

`bca` follows one exit-code convention across every subcommand, so CI
scripts can branch on the process status without inspecting output:

| Code | Meaning |
|------|---------|
| `0`  | Success. |
| `1`  | Tool error — a bad flag / threshold / glob spec, unreadable input, or a parse failure. This includes **usage errors** (unknown flag, bad subcommand, a malformed `--threshold` value rejected by clap). **Never** a metric signal. |
| `2`  | Metric gate: [`check`](check.md) thresholds were exceeded, [`vcs commit --fail-above`](vcs.md) was breached, or [`diff`](../recipes/exporting-data.md) / [`diff-baseline`](../recipes/baselines.md) under `--exit-code` found a non-empty filtered diff. |
| `3`–`5` | [`check --exit-codes=tiered`](check.md#tiered-exit-codes---exit-codestiered) only: tiered violation severity (new-only / regression-only / mixed / hard-breach). |

Codes `2`–`5` are gate signals, emitted only by [`check`](check.md),
[`vcs commit --fail-above`](vcs.md), and `diff` / `diff-baseline` under
the opt-in `--exit-code` flag; they report a metric result, not a
failure of the tool. Every other subcommand — `metrics`, `ops`,
`report`, `diff`, `diff-baseline`, `exemptions`, `init`, and the rest —
exits `0` on success and `1` on error. Because `1` is reserved for tool
errors — usage errors included, so a typo'd flag never lands in the
gate band — CI can always distinguish "the gate found a regression"
(`2`–`5`) from "the tool itself crashed" (`1`).

## Flag placement and input paths

Every subcommand reads the input it analyzes as a trailing positional
path, so the common case reads like every other code tool
(`tokei`, `cloc`, `scc`, `rg`):

```bash
bca metrics src/            # analyze the src/ tree
bca check src/ tests/       # gate two subtrees
bca find -t function_item . # find every function in the current tree
```

Flags are **scoped to the subcommand that consumes them** and must be
written **after** the subcommand token:

```bash
bca metrics --exclude '*.generated.rs' src/   # correct
bca --exclude '*.generated.rs' metrics src/   # ERROR (exit 1)
```

Only `-w` / `--warnings` and `--report-skipped` are universal and accepted
in any position. Every input-selection flag (`-p` / `--paths`, `-I` /
`--include`, `-X` / `--exclude`, `-l` / `--language`, `--paths-from`,
`--exclude-from`, `--no-ignore`, `--no-skip-generated`, `--no-config`),
walker-tuning flag (`-j` / `--jobs`, `--exclude-tests`,
`--cyclomatic-count-try`), the preprocessor flag (`--preproc-data`), and the
output flag (`--color`) lives in a help-grouped section
(*Input selection* / *Walker tuning* / *Preprocessor* / *Output*) on the
subcommands that read it. A flag passed to a subcommand that never
consumed it is a hard usage error (exit 1) rather than a silent no-op — so
`bca vcs commit --exclude-tests` and `bca list-metrics --paths` both
error, and `bca list-metrics --help` does not advertise walker flags.

The `-p` / `--paths` flag still works and is **unioned** with the
positional paths, so `bca metrics a.rs --paths b.rs` walks both. The
[`find`](nodes.md) and [`count`](nodes.md) subcommands take their node
kinds via a repeatable `-t` / `--type` flag (so the positional slot is
free for paths): `bca find -t function_item -t struct_item src/`.

## Metrics

Metrics provide quantitative measures about source code, which can help in:

- Compare different programming languages
- Provide information on the quality of a code
- Tell developers where their code is more tough to handle
- Discovering potential issues early in the development process

**big-code-analysis** calculates the metrics starting from the
source code of a program. These kind of metrics are called *static metrics*.

## Nodes

To represent the structure of program code, **bca** builds
an
<a href="https://en.wikipedia.org/wiki/Abstract_syntax_tree" target="_blank">Abstract Syntax Tree (AST)</a>.
A **node** is an element of this tree and denotes any syntactic construct
present in a language.

Nodes can be used to:

- Create the syntactic structure of a source file
- Discover if a construct of a language is present in the analyzed
  code
- Count the number of constructs of a certain kind
- Detect errors in the source code

## REST API

**bca-web** runs a server offering a REST API. This allows users to
send source code via HTTP and receive corresponding metrics in `JSON`
format.

## Skipping generated code

Generated bindings (protobuf stubs, OpenAPI clients, lex/yacc output,
build-system plumbing) inflate metrics for code no human will refactor.
By default, `bca` scans the first ~50 lines / 5 KiB of
each file for a generated-code marker and skips matches **before** parsing,
so the skipped file pays no tree-sitter parse cost.

Recognized markers (case-insensitive):

- `@generated` — Facebook / Meta convention; also emitted by buck2,
  rustfmt, prettier, and many code generators.
- `DO NOT EDIT` — Go's `// Code generated by … DO NOT EDIT.` is the
  canonical form; the bare phrase is also widely copied (Bazel, protoc,
  OpenAPI clients).
- `GENERATED CODE` — Lizard's marker, recognized for compatibility.

A marker phrase that appears only deep in the file body (past the scan
window) does **not** trigger the skip — the detector deliberately looks
only at the file header.

The skip applies uniformly to `bca metrics`, `bca report`, and the
threshold engine.

### Flags

- `--no-skip-generated` — disable the auto-skip and restore the previous
  behavior (every file is parsed).
- `--report-skipped` — log `skipped (generated): <path>` to stderr for
  each file the detector excludes, so you can audit the exclusions and
  add an explicit include if a file was wrongly tagged.

## Respecting `.gitignore`

When a directory is passed to `--paths`, `bca` walks
it with `.gitignore` awareness by default. Files matched by any of the
following are skipped before parsing:

- `.gitignore` files inside the walked tree.
- `.ignore` files (the ripgrep / `fd` convention).
- `.git/info/exclude`.
- The global gitignore (`~/.config/git/ignore`, or whatever
  `core.excludesFile` points at).
- `.gitignore` files in ancestor directories of the seed (so
  `bca metrics src/` from a project root picks up the project's
  top-level `.gitignore`).

The walker honors `.gitignore` even outside a checked-in git
repository, so an extracted source tarball with a `.gitignore` file
gets the same treatment as a fresh `git clone`.

Hidden files (those whose basename starts with `.`) are filtered
during the walk, matching the previous behavior.

### Explicit paths bypass the filter

Files passed by name — via `--paths` or `--paths-from` — are always
analyzed, even when they would be excluded by `.gitignore`. This makes
it safe to do `bca metrics --paths-from -` from `git diff
--name-only`-style pipelines without losing files that happen to be
covered by a wildcard ignore rule.

### Path discovery flags

- `--no-ignore` — disable `.gitignore` / `.ignore` / global-gitignore
  awareness when expanding directory seeds.
- `--paths-from <FILE>` — read newline-separated input paths from
  `<FILE>`, or from stdin when `<FILE>` is `-`. Combined as a union
  with any `--paths` values; `-I` / `-X` globs still apply. Blank
  lines are skipped; `#` is treated as a path character (not a
  comment). To pass a file literally named `-`, write `./-`.
- `--exclude-from <FILE>` — read newline-separated `--exclude` glob
  patterns from `<FILE>`, or from stdin when `<FILE>` is `-`.
  Patterns are unioned with any inline `--exclude` / `-X` values
  into a single deny-set; order does not matter. `.gitignore`-style:
  blank lines and lines whose first non-whitespace character is `#`
  are skipped, and a leading UTF-8 BOM is stripped. Convention is a
  `.bcaignore` at the repo root, mirroring `.gitignore` /
  `.dockerignore`. To pass a file literally named `-`, write `./-`.
