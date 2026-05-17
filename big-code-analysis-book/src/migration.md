# Migration: Flag CLI to Subcommand CLI

The CLI was restructured from a flat flag-style interface (one process,
many mutually-exclusive `--action` flags) into a subcommand-style
interface (`bca <verb>`). This page maps every old invocation to its
replacement.

## Why the change

The flag CLI overloaded `--output-format` with two unrelated meanings:
per-file serialization (`-O json/yaml/toml/cbor`) and a post-walk
aggregated report (`-O markdown`). It needed two clap `ArgGroup`s plus
runtime checks to police invalid combinations, and `--top` /
`--strip-prefix` lived as global flags that only applied to one format.
Future aggregated formats (e.g. HTML) would compound the fragility.

The subcommand CLI fixes the structure: `bca metrics` and `bca ops` emit
per-file output; `bca report <FORMAT>` emits an aggregated report; each
verb has its own scoped flag set.

## Migration mapping

| Old | New |
| --- | --- |
| `--metrics -O markdown` (+ `--top`, `--strip-prefix`) | `report markdown` |
| `--metrics -O json/yaml/toml/cbor` | `metrics -O json/yaml/toml/cbor` |
| `--metrics -O checkstyle/sarif/clang-warning/msvc-warning` | `check --threshold ... --output-format <fmt> [--output FILE]` |
| `--ops -O ...` | `ops -O ...` |
| `--dump` | `dump` |
| `--find <NODE>` | `find <NODE> [<NODE>...]` |
| `--count <LIST>` | `count <NODE> [<NODE>...]` |
| `--function` | `functions` |
| `--comments [--in-place]` | `strip-comments [--in-place]` |
| `--preproc <FILE> <FILE>...` (producer) | `preproc -o <OUT>` |
| `--preproc <FILE>` (consumer) | `--preproc-data <FILE>` (global) |
| `--list-metrics [MODE]` | `list-metrics [MODE]` |
| `--pr` (pretty) | `--pretty` (on `metrics` and `ops`) |
| `-p`, `-I`, `-X`, `-j`, `-l`, `--ls`, `--le`, `-w` | unchanged; global |

## Side-by-side examples

### Aggregated markdown report

```bash
# OLD
big-code-analysis-cli \
    --metrics \
    --paths "$PWD" \
    --output-format markdown \
    --num-jobs $(nproc) \
    --top 20 \
    --strip-prefix "$PWD/"

# NEW
bca \
    --paths "$PWD" \
    --num-jobs $(nproc) \
    report markdown \
    --top 20 \
    --strip-prefix "$PWD/"
```

### Per-file metric extraction

```bash
# OLD
big-code-analysis-cli --metrics --paths ./src --output-format json --output ./out/

# NEW
bca --paths ./src metrics -O json --output ./out/
```

### Per-file ops extraction

```bash
# OLD: big-code-analysis-cli --ops --paths ./src -O json -o ./out/
# NEW: bca --paths ./src ops -O json -o ./out/
```

### AST dump

```bash
# OLD: big-code-analysis-cli --dump --paths ./file.rs
# NEW: bca --paths ./file.rs dump
```

### Find / count nodes

```bash
# OLD: big-code-analysis-cli --find call_expression --paths ./src
# NEW: bca --paths ./src find call_expression

# OLD: big-code-analysis-cli --count if_statement,for_statement --paths ./src
# NEW: bca --paths ./src count if_statement for_statement
```

> Note: `count` now takes one node type per positional argument (space
> separated) rather than one comma-separated string.

### Function spans

```bash
# OLD: big-code-analysis-cli --function --paths ./src
# NEW: bca --paths ./src functions
```

### Strip comments

```bash
# OLD: big-code-analysis-cli --comments --in-place --paths ./src
# NEW: bca --paths ./src strip-comments --in-place
```

### Preproc data — producer

```bash
# OLD
big-code-analysis-cli --metrics --preproc a.h --preproc b.h \
    --paths ./src -o /tmp/p.json

# NEW
bca --paths ./src preproc -o /tmp/p.json
```

### Preproc data — consumer

```bash
# OLD
big-code-analysis-cli --metrics --preproc /tmp/p.json \
    --paths ./src -O json -o ./out/

# NEW
bca --paths ./src --preproc-data /tmp/p.json \
    metrics -O json -o ./out/
```

### List metrics

```bash
# OLD: big-code-analysis-cli --list-metrics descriptions
# NEW: bca list-metrics descriptions
```

## Migration hint at runtime

If you run a legacy invocation, the CLI prints a hint identifying the
recognized old flags and their new equivalents before clap's own error.
For example:

```text
$ bca --metrics -O markdown
note: the CLI was restructured into subcommands. See migration.md for the full mapping.
  --metrics  ->  bca metrics
  -O markdown  ->  bca report markdown [--top N] [--strip-prefix P]
  Run `bca --help` for the new command list.

error: unexpected argument '--metrics' found
```
