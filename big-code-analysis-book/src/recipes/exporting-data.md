# Exporting metric data

The `metrics`, `ops`, and `preproc` subcommands all support structured
output formats meant for machine consumption. This page covers writing
those documents, diffing two runs, discovering the metric catalog at
runtime, and stripping comments for downstream tools. Pair the output
with a JSON processor like [`jq`](https://jqlang.github.io/jq/) for
ad-hoc analysis, or feed it into a database or dashboard.

## Export per-file metrics as JSON

```bash
bca metrics \
    --paths src/ \
    -O json \
    --output-dir /tmp/metrics
```

This writes one JSON file per analyzed source file under
`/tmp/metrics/`. The output filename mirrors the input path with the
format extension appended — `src/lib.rs` becomes `src/lib.rs.json`,
not `src/lib.json`. Use `--pretty` if you intend to read the files by
hand:

```bash
bca metrics -p src/ --pretty -O json --output-dir /tmp/metrics
```

To collect the whole run into one file instead of a tree, use
`--output <file>`; it writes a single aggregate document (a top-level
JSON array of the per-file results):

```bash
bca metrics -p src/ -O json --output /tmp/metrics.json
```

CBOR (`-O cbor`) is the most compact format; it is binary and so
requires a destination (`--output` or `--output-dir`). JSON, TOML, and
YAML can all be streamed to stdout when no destination is given, which
is useful for pipelines.

## Compare two metric runs with `bca diff`

`bca diff` compares two JSON metric runs and reports, per metric, which
files changed (old → new), plus any files added or removed between the
two sets. Each side is either a single per-file JSON document or a whole
directory tree of them (the form `metrics -O json --output-dir <dir>`
writes), so the common workflow is two `bca metrics` runs into separate
directories:

```bash
# Capture the "before" state.
bca metrics -p src/ -O json --output-dir /tmp/before

# ...make a change (e.g. bump a tree-sitter grammar)...

# Capture the "after" state and diff.
bca metrics -p src/ -O json --output-dir /tmp/after
bca diff /tmp/before /tmp/after
```

The output buckets every per-file delta by metric name — the same names
`bca list-metrics` prints (`cyclomatic`, `cognitive`, `sloc`, …):

```text
2 metric(s) changed, 1 added file(s), 0 removed file(s)

## Added files
  src/new_module.rs.json

## cyclomatic (2 change(s))
  src/lib.rs.json.sum  12 → 14
  src/lib.rs.json.max   4 → 6

## halstead (1 change(s))
  src/lib.rs.json.effort  5820.3 → 7104.9
```

Useful flags:

- `--format markdown` for a sticky PR comment, `--format json` for a
  stable machine-readable schema (CI consumers).
- `--min-change <N>` reports only deltas whose absolute change is at
  least `N` (the default `0` reports any change).
- `--metric <name>` (repeatable) restricts the diff to specific metrics.

### Diff against a git ref with `--since`

For the interactive "what did my uncommitted change do to the metrics?"
case, `--since <ref>` skips the two-capture dance: it analyzes the tree
**at a git ref** for the before side, and diffs it against the current
working tree (or an explicit after-side tree):

```bash
# Before = the tree at HEAD~1; after = the current working tree.
bca diff --since HEAD~1 -p src/

# After = an explicit tree (e.g. a second checkout) instead of the
# working tree. The single positional is the after side.
bca diff --since main /path/to/other-checkout -p src/
```

`--since` materializes the ref's tree into a temporary directory (via
`git ls-tree` + `git cat-file`), runs the same metric walk against it,
then diffs — the temp tree is removed automatically, including on
error. The same `-p/--paths`, `-I/--include`, and `-X/--exclude`
selection applies to both sides so they analyze the same file set.
Selection paths are **repo-root-relative** and must be relative: the
working-tree side is anchored at the repository root (matching the
whole-tree materialization of the ref), so `bca diff --since` produces
the same result from any subdirectory. An absolute `--paths` is
rejected (exit 1) — it cannot address the extracted ref tree.

Unlike `bca check --since` (best-effort), `bca diff --since` is an
explicit request: an unresolvable ref, a missing `git`, or a non-git
working directory is a hard error (exit 1). `--since` takes at most one
positional (the after side); passing two is an error.

`bca diff` exits 0 on success — it is informational, not a gate —
unless the opt-in `--exit-code` flag is passed, which exits 2 when
the filtered diff is non-empty.

## Pull a single metric across an entire tree

Combine streamed JSON output with `jq` to extract one value per file:

```bash
bca metrics -p src/ -O json \
  | jq -c '{file: .name, mi: .metrics.mi.visual_studio}'
```

The same idea works for any metric — `cyclomatic.sum`,
`cognitive.sum`, `loc.sloc`, and so on. Run `bca list-metrics
descriptions` to see the catalog.

## Discover the metric catalog at runtime

Tooling that drives the CLI shouldn't hard-code metric names. Ask the
binary:

```bash
bca list-metrics                # one name per line
bca list-metrics descriptions   # name + summary
```

This is the right input for code generators, schema definitions, or
tab-completion.

## Extract operands and operators (Halstead)

`ops` emits the raw operand and operator lists per file, which is the
input to
[Halstead](https://en.wikipedia.org/wiki/Halstead_complexity_measures)-style
metric calculations beyond what the built-in report shows:

```bash
bca ops \
    --include "*.rs" \
    --paths src/ \
    -O json --pretty \
    --output-dir /tmp/ops
```

> **One glob per occurrence.** `--include` and `--exclude` take
> exactly one value each time they appear; repeat the flag for
> multiple globs (`--include "*.rs" --include "*.py"`). A positional
> path that follows is never swallowed. The `=` form
> (`--include="*.rs"`) also works.

Each output file mirrors the input path under `/tmp/ops/`.

Both lists are sorted, so re-running `ops` over unchanged sources
produces byte-identical output — safe to diff between runs, check into
a repository, or use as a cache key.

## Strip comments from a tree

`strip-comments` rewrites source so that downstream tools that don't
understand comment syntax can still consume the code. Output routing has
three modes:

- **stdout (default).** With neither flag, the stripped source streams
  to stdout — best for a single file in a pipeline.
- **`--output` / `-o <FILE>` (single file).** Writes the stripped source
  to `<FILE>`, leaving the input untouched. Only meaningful for one
  input file; mutually exclusive with `--in-place`.
- **`--in-place` (multi-file).** Rewrites each matched input file on
  disk. Use this for a whole tree; mutually exclusive with `--output`.

```bash
# Stream a single file with comments removed.
bca strip-comments --paths src/lib.rs

# Write a single stripped file to a new path (input untouched).
bca strip-comments --paths src/lib.rs --output src/lib.stripped.rs

# Rewrite every Python file in src/ in place.
bca strip-comments --include "*.py" --paths src/ \
    --in-place
```

`--in-place` is destructive — make sure the tree is committed or
backed up first. Passing both `--in-place` and `--output` is a usage
error.
