# Flat-record iteration

`bca.flatten_spaces(result)` walks the nested `FuncSpace` tree in
pre-order and yields one flat, scalar-only `dict` per node —
ready for `sqlite3.executemany`,
`pandas.DataFrame.from_records`, or any other tabular consumer.

Metric keys use the same dotted convention as the CLI's CSV
writer (`cyclomatic.modified.sum`, `halstead.volume`,
`loc.lloc_average`, …). Identity keys (`path`, `name`, `kind`,
`start_line`, `end_line`, `parent_name`, `depth`) are added on
every record.

## SQLite via `executemany`

The example below analyses one file and inserts one row per
`FuncSpace` into a sqlite table whose columns are the union of
all flattened keys.

```python
{{#include ../../../big-code-analysis-py/examples/flat_records.py}}
```

The iterator is **lazy and single-use**: it walks the input once
without materialising the whole list. A second iteration of the
same iterator yields nothing — call `list()` once if you need to
re-iterate.

## Pandas

`flatten_spaces` is the natural input to
`pandas.DataFrame.from_records`. Pandas is not a dependency of
the bindings; install it separately if you want the DataFrame
view.

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
writer, with a couple of intentional deltas:

* Identity columns use `name` / `kind` here; the CSV writer uses
  `space_name` / `space_kind`. Flat records also add
  `parent_name` / `depth`; the CSV writer omits those.
* `tokens.*` flattens to the JSON shape (`tokens.tokens`,
  `tokens.tokens_average`, …), while CSV renames those to
  `tokens.sum` / `.average` / `.min` / `.max`. Rename in the
  consumer if you need exact CSV alignment.

Anonymous spaces (Rust closures, JavaScript function expressions /
arrows) keep their `name == "<anonymous>"` marker verbatim —
`flatten_spaces` does not normalise.

## Caveats

* `parent_name` alone cannot disambiguate same-named siblings
  nested under different parents (e.g. two `Inner` classes under
  two different outer classes both surface as
  `parent_name == "Inner"` for their own children). Pair with
  `depth` and source-order position, or rebuild the qualified
  name in your consumer, if you need a fully-qualified path.
* Do not mutate the input `result` while iterating: the walker
  keeps references into it, so mutations to not-yet-yielded
  subtrees will be observed in later records.
* Missing metric subtrees produce no keys (absent, not `None`),
  matching the "Halstead disabled" edge case for [metric
  selection](metrics.md).
* `flatten_spaces` raises `TypeError` if the input is not a
  mapping; callers must filter `None` returns from `bca.analyze`
  (e.g. generated files with `skip_generated=True`) before
  passing.
