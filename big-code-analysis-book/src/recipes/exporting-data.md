# Exporting metric data

The `metrics`, `ops`, and `preproc` subcommands all support structured
output formats meant for machine consumption. This page covers writing
those documents, diffing two runs, discovering the metric catalog at
runtime, and stripping comments for downstream tools. Pair the output
with a [JSON](https://www.rfc-editor.org/rfc/rfc8259) processor like
[`jq`](https://jqlang.github.io/jq/) for ad-hoc analysis, or feed it
into a database or dashboard.

## Export per-file metrics as JSON

```bash
bca metrics \
    --paths src/ \
    -O json \
    --output-dir /tmp/metrics
```

This writes one JSON file per analyzed source file under
`/tmp/metrics/`. The output filename is the input path with the
format extension appended: `src/lib.rs` becomes `src/lib.rs.json`,
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

Its elements are ordered by file path regardless of the order the
parallel workers finished in, so the document is byte-identical between
runs over unchanged sources. `bca ops --output <file>` orders its
aggregate the same way.

[CBOR](https://cbor.io/) (`-O cbor`) is the most compact format; it is
binary and so requires a destination (`--output` or `--output-dir`).
JSON, [TOML](https://toml.io/en/), and
[YAML](https://yaml.org/spec/1.2.2/) stream to stdout when no
destination is given, which suits pipelines.

## Compare two metric runs with `bca diff`

`bca diff` compares two JSON metric runs and reports, per metric, which
files changed (old → new), plus any files added or removed between the
two sets. Each side is a single per-file JSON document, the aggregate
file `metrics -O json --output <file>` writes, or a whole directory tree
of either (what `metrics -O json --output-dir <dir>` writes), so the
common workflow is two `bca metrics` runs into separate directories:

```bash
snap=$(mktemp -d "${TMPDIR:-/tmp}/bca-diff.XXXXXX")

# Capture the "before" state.
bca metrics -p src/ -O json --output-dir "$snap/before"

# ...make a change (for example, bump a tree-sitter grammar)...

# Capture the "after" state and diff.
bca metrics -p src/ -O json --output-dir "$snap/after"
bca diff "$snap/before" "$snap/after"
```

The output buckets every per-file delta by metric name, using the same
names `bca list-metrics` prints (`cyclomatic`, `cognitive`, `sloc`, …):

```text
2 metric(s) changed, 1 added file(s), 0 removed file(s), 0 added field(s), 0 removed field(s)

## Added files
  src/new_module.rs

## cyclomatic (2 change(s))
  src/lib.rs.max  4 → 6
  src/lib.rs.sum  12 → 14

## halstead (1 change(s))
  src/lib.rs.effort  5820.3 → 7104.9
```

A leaf that carries a number on one side only (a family the other
capture never computed because the two runs used different `--metrics`
sets, or a field a newer bca started emitting) has nothing to diff
against, so it is never compared with zero. It is listed under
`## Added fields` / `## Removed fields` (and as `added_fields` /
`removed_fields` rows of `{file, metric, field}` in the JSON form), is
counted in the summary line, and counts as a difference for
`--exit-code`. `--metric` filters these rows as it filters buckets;
`--min-change` does not apply to them, since there is no delta to
measure. "Present" means numeric: a leaf written as `null` on one side
(how a non-finite derived value, such as a `vcs` ratio with nothing to
divide by, is serialized) reads as absent there, so two captures by the
same bca version can list a field that became applicable between them.

A document pairs with the other side's document of the same `name`,
the source path bca recorded, compared in lexical form: `.` components
are dropped, so captures taken with `-p ./src` and `-p src` pair, and a
`..` is kept. A directory side reads every `*.json` file under it and
passes over any other file, such as a README. Input `bca diff` cannot
pair is an error (exit 1) naming it, never a file silently missing from
one side: a directory with no `*.json` file (a capture written with
`-O yaml`, say), JSON with no `metrics` object, two documents with one
`name`, and a document with no `name` unless it is a whole file inside
a directory side, where its relative path stands in. `bca metrics`
writes no `name` for a source path that is not valid UTF-8, and no
layout can diff such a file. A symlinked `*.json` file is read like the file it names, and one that
dangles is an error. A symlinked directory inside a side is not entered,
as the source walk enters none.

Useful flags:

- `--format markdown` produces Markdown for a sticky PR comment;
  `--format json` produces a stable machine-readable schema for CI
  consumers.
- `--min-change <N>` reports only deltas whose absolute change is at
  least `N` (the default `0` reports any change).
- `--metric <name>` (repeatable) restricts the diff to specific metrics.

A saved document carries no record of the walker settings that
produced it, so `bca diff` cannot tell two captures made under
different settings apart from a real change. Compare captures taken
by the same version with the same `exclude_tests`,
`cyclomatic_count_try` and `mi_charges_docs`; a different
[MI documentation mode](../metrics.md#mi-charges-docs) moves the
`mi.sei` value of every documented space. `--since` runs both sides
itself with one set of flags, so it cannot mix modes.

### Diff against a git ref with `--since`

To answer "what did my uncommitted change do to the metrics?",
`--since <ref>` skips the two captures: it analyzes the tree **at a
git ref** for the before side and diffs it against the current working
tree, which is always the after side:

```bash
# Before = the tree at HEAD~1; after = the current working tree.
bca diff --since HEAD~1 -p src/

# The same diff, scoped by the optional positional instead of -p. It
# selects the src/ subtree on both sides; it never names another tree.
bca diff --since HEAD~1 src
```

`--since` materializes the ref's tree into a temporary directory (via
`git ls-tree` + `git cat-file`), runs the same metric walk against it,
then diffs. The temp tree is removed when the command finishes,
whether it succeeds or exits with an error. Only a run killed by a
signal (Ctrl-C included) leaves it behind in the temporary directory.
The same `-p/--paths`, `-I/--include`, and `-X/--exclude`
selection applies to both sides so they analyze the same file set.
Each side reads an `--exclude-from` file for itself, a relative one
from its own root (the ref's copy for the before side), so
`--exclude-from -` (stdin, readable only once) is rejected, as is
`--paths-from`.
Selection paths are **repo-root-relative** and must be relative: the
working-tree side is anchored at the repository root (matching the
whole-tree materialization of the ref), so `bca diff --since` produces
the same result from any subdirectory. An absolute `--paths` or
positional is rejected (exit 1) because it cannot address the extracted
ref tree.

Unlike `bca check --since` (best-effort), `bca diff --since` is an
explicit request: an unresolvable ref, a missing `git`, or a non-git
working directory is a hard error (exit 1). `--since` takes at most one
positional, the relative path scope above; passing two is an error.

`bca diff` is informational, not a gate, and exits 0 on success. The
opt-in `--exit-code` flag changes that: it exits 2 when the filtered
diff is non-empty.

## Pull a single metric across an entire tree

Combine streamed JSON output with `jq` to extract one value per file:

```bash
bca metrics -p src/ -O json \
  | jq -c '{file: .name, mi: .metrics.mi.visual_studio}'
```

The same idea works for any metric: `cyclomatic.sum`,
`cognitive.sum`, `loc.sloc`, and so on. Run `bca list-metrics
descriptions` to see the catalog.

## Discover the metric catalog at runtime

Tooling that drives the CLI should not hard-code metric names. Ask
the binary:

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
produces byte-identical output. It is safe to diff between runs, check
into a repository, or use as a cache key.

## Strip comments from a tree

`strip-comments` rewrites source so that downstream tools that do not
understand comment syntax can still consume the code. Output routing
has three modes:

- **stdout (default).** With neither flag, the stripped source streams
  to stdout, which suits a single file in a pipeline.
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

`--in-place` is destructive: commit or back up the tree first. Passing
both `--in-place` and `--output` is a usage error.
