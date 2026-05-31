# Exporting metric data

The `metrics`, `ops`, and `preproc` subcommands all support structured
output formats meant for machine consumption. Pair them with a JSON
processor like [`jq`](https://jqlang.github.io/jq/) for ad-hoc
analysis, or feed them into a database or dashboard.

## Export per-file metrics as JSON

```bash
bca \
    --paths src/ \
    metrics \
    -O json \
    -o /tmp/metrics
```

This writes one JSON file per analyzed source file under
`/tmp/metrics/`. The output filename mirrors the input path with the
format extension appended — `src/lib.rs` becomes `src/lib.rs.json`,
not `src/lib.json`. Use `--pretty` if you intend to read the files by
hand:

```bash
bca -p src/ metrics --pretty -O json -o /tmp/metrics
```

CBOR (`-O cbor`) is the most compact format; it is binary and
therefore requires `-o`. JSON, TOML, and YAML can all be streamed to
stdout when `-o` is omitted, which is useful for pipelines.

## Compare two metric runs with `bca diff`

`bca diff` compares two JSON metric runs and reports, per metric, which
files changed (old → new), plus any files added or removed between the
two sets. Each side is either a single per-file JSON document or a whole
directory tree of them (the form `metrics -O json -o <dir>` writes), so
the common workflow is two `bca metrics` runs into separate directories:

```bash
# Capture the "before" state.
bca -p src/ metrics -O json -o /tmp/before

# ...make a change (e.g. bump a tree-sitter grammar)...

# Capture the "after" state and diff.
bca -p src/ metrics -O json -o /tmp/after
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

`bca diff` always exits 0 on success — it is informational, not a gate.
It replaces the former `json-minimal-tests` + `split-minimal-tests.py`
chain used to validate that a grammar bump did not regress metrics; the
`check-grammar-crate.py` helper now calls `bca diff` internally.

## Pull a single metric across an entire tree

Combine streamed JSON output with `jq` to extract one value per file:

```bash
bca -p src/ metrics -O json \
  | jq -c '{file: .name, mi: .metrics.mi.mi_visual_studio}'
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
input to Halstead-style metric calculations beyond what the built-in
report shows:

```bash
bca \
    --include "*.rs" \
    --paths src/ \
    ops \
    -O json --pretty \
    -o /tmp/ops
```

> **Flag ordering.** Variadic flags like `--include` and `--exclude`
> consume tokens until the next flag, so put them before `--paths`
> (or use the `--include=GLOB` single-value form) to keep the
> subcommand from being eaten as a glob.

Each output file mirrors the input path under `/tmp/ops/`.

## Strip comments from a tree

`strip-comments` rewrites source so that downstream tools that don't
understand comment syntax can still consume the code. It defaults to
streaming the result to stdout; pass `--in-place` to overwrite files
on disk:

```bash
# Stream a single file with comments removed.
bca --paths src/lib.rs strip-comments

# Rewrite every Python file in src/ in place.
bca --include "*.py" --paths src/ \
    strip-comments --in-place
```

`--in-place` is destructive — make sure the tree is committed or
backed up first.
