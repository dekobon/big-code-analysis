# Flat-record iteration

`bca.flatten_spaces(result)` walks the nested `FuncSpace` tree in
pre-order and yields one flat, scalar-only `dict` per node, ready
for [`sqlite3.executemany`](https://docs.python.org/3/library/sqlite3.html#sqlite3.Cursor.executemany),
[`pandas.DataFrame.from_records`](https://pandas.pydata.org/docs/reference/api/pandas.DataFrame.from_records.html),
or any other tabular consumer. This page is for a caller loading metrics
into a database or a DataFrame.

Metric keys use the same dotted convention as the CLI's CSV
writer (`cyclomatic.modified.sum`, `halstead.volume`,
`loc.lloc_average`, and so on). Identity keys (`path`, `name`,
`kind`, `start_line`, `end_line`, `parent_name`, `depth`) are added
on every record.

## SQLite via `executemany`

The example below analyses one file and inserts one row per
`FuncSpace` into a sqlite table whose columns are the union of
all flattened keys.

```python
{{#include ../../../big-code-analysis-py/examples/flat_records.py}}
```

The iterator is **lazy and single-use**: it walks the input once
without materialising the whole list. A second iteration of the
same iterator yields nothing; call `list()` once if you need to
re-iterate.

## Pandas

`flatten_spaces` is the natural input to
`pandas.DataFrame.from_records`. [pandas](https://pandas.pydata.org/)
is not a dependency of the bindings; install it separately if you
want the DataFrame view.

```python
import big_code_analysis as bca
import pandas as pd

result = bca.analyze("src/lib.rs")
if result is not None:
    df = pd.DataFrame.from_records(bca.flatten_spaces(result))
    print(df.head())
    # Group by space kind to inspect the average cyclomatic per
    # function vs. per class vs. per file.
    by_kind = df.groupby("kind")["cyclomatic.sum"].mean()
```

## Identity columns vs CLI CSV

The flat-record schema is mostly aligned with the CLI's CSV
writer, with a few intentional deltas:

* Identity columns use `name` / `kind` here; the CSV writer uses
  `space_name` / `space_kind`. Flat records also add
  `parent_name` / `depth`; the CSV writer omits those.
* The per-space `cognitive.value`, `cyclomatic.value`,
  `cyclomatic.modified.value`, `abc.value` and `nargs.value` leaves
  (the numbers `bca check` thresholds against) are present here, as
  in the JSON shape, but absent from the CSV header.
* `tokens.*` flattens to the JSON shape (`tokens.tokens`,
  `tokens.average`, `tokens.min`, `tokens.max`). Only the sum leaf
  differs from CSV, which spells it `tokens.sum`; the
  `average` / `min` / `max` leaves now match (#590). Rename the sum
  leaf in the consumer if you need exact CSV alignment.

Anonymous spaces (Rust closures, JavaScript function expressions
and arrows) keep their `name == "<anonymous>"` marker verbatim;
`flatten_spaces` does not normalise.

## Caveats

* `parent_name` alone cannot disambiguate same-named siblings
  nested under different parents (two `Inner` classes under two
  different outer classes both surface as `parent_name == "Inner"`
  for their own children). Pair it with `depth` and source-order
  position, or rebuild the qualified name in your consumer, if you
  need a fully-qualified path.
* Do not mutate the input `result` while iterating: the walker
  keeps references into it, so mutations to not-yet-yielded
  subtrees show up in later records.
* Missing metric subtrees produce no keys (absent, not `None`),
  matching the "Halstead disabled" edge case for [metric
  selection](metrics.md).
* `flatten_spaces` raises `TypeError` if the input is not a
  mapping. Filter out the `None` returns from `bca.analyze`
  (generated files under `skip_generated=True`) before passing,
  and likewise the `None` slots `analyze_batch` emits under
  `skip_generated=False`, which mark a file the read gate declined
  to parse.
