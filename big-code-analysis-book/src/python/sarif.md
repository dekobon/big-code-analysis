# SARIF output

`bca.to_sarif(result, *, thresholds=None)` renders an analysis
result (or an iterable of them) into a [SARIF
2.1.0](https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html)
JSON document, ready for upload to [GitHub Code
Scanning](https://docs.github.com/en/code-security/code-scanning) or any
other SARIF consumer. The output is produced by the same Rust
writer that backs `bca check --report-format sarif`, so the schema URL, tool
driver name / version, and rule descriptions match the CLI
byte-for-byte.

Findings match in order as well as in content. Both surfaces sort their
findings by path, then start line, then metric name — the order
`bca check` applies after its walk — and findings tying on all three
keep the depth-first source order of the space tree. For the same files
and thresholds the two `results` arrays therefore line up entry for
entry against `bca check --no-suppress`. `to_sarif` compares raw metric
values, so it applies none of the in-source
[suppression markers](../commands/suppression.md) `bca check` honours by
default (each marked space keeps its `suppressed` key, for a caller that
wants to filter), no baseline, and no `[check] exclude` globs.

"The same files" means a **unique file set**. The CLI folds repeated path
seeds together, so `bca check -p a.py -p a.py` analyses `a.py` once and
emits one finding per breach. `analyze_batch` instead returns one result
per input, and `to_sarif` renders every result it is handed, so
`to_sarif(analyze_batch([a, a]), ...)` emits each finding twice.
Deduplicating in the binding would be wrong — two distinct results may
legitimately share a name, since `analyze_source` takes the caller's —
so hand `to_sarif` a unique file set when comparing the two documents
positionally.

Examples on this page import the package as `bca`
(`import big_code_analysis as bca`). A bare `bca` in a shell command is
the CLI binary.

```python
{{#include ../../../big-code-analysis-py/examples/sarif_output.py:17:33}}
```

`to_sarif` accepts:

* A single `dict` returned by `bca.analyze` or
  `bca.analyze_source`.
* Any iterable yielding such dicts, `bca.AnalysisFailure`
  instances, and/or `None` (the natural shape of
  `bca.analyze_batch`'s return value). `AnalysisFailure` and `None`
  entries are skipped silently — they represent files for which no
  record was emitted, not findings.
* A scalar `None`, the documented return of `bca.analyze` for a
  skipped file; it yields an empty SARIF run.

## Thresholds

Accepted threshold names mirror the CLI's `EXTRACTORS` table in
[`big-code-analysis-cli/src/thresholds.rs`](https://github.com/dekobon/big-code-analysis/blob/main/big-code-analysis-cli/src/thresholds.rs):

* `cognitive`, `cyclomatic`, `cyclomatic.modified`
* `halstead.volume`, `halstead.difficulty`, `halstead.effort`,
  `halstead.time`, `halstead.bugs`
* `loc.sloc`, `loc.ploc`, `loc.lloc`, `loc.cloc`, `loc.blank`
* `nom`, `tokens`, `nexits`, `nargs`
* `mi.original`, `mi.sei`, `mi.visual_studio`
* `abc`, `wmc`, `npm`, `npa`

An unknown name raises `ValueError` listing the accepted set, so
a typo fails fast instead of silently producing an empty SARIF
run.

`thresholds=None` (the default) and `thresholds={}` both produce
a well-formed SARIF document with empty `results` and `rules`
arrays. This matches `bca check`, which applies **no implicit
limits**: every run supplies its own, from `--threshold` or a
`bca.toml` (which `bca init` scaffolds with a starting table).

## Upload to GitHub Code Scanning

```yaml
# .github/workflows/code-scanning.yml (excerpt)
- name: Compute metric SARIF
  run: |
    python - <<'PY'
    import big_code_analysis as bca
    with open("paths.txt", encoding="utf-8") as paths_fh:
        results = bca.analyze_batch(paths_fh.read().splitlines())
    with open("metrics.sarif", "w", encoding="utf-8") as fh:
        fh.write(bca.to_sarif(results, thresholds={"cyclomatic": 15}))
    PY
- name: Upload to Code Scanning
  uses: github/codeql-action/upload-sarif@v3
  with:
    sarif_file: metrics.sarif
```

The upload action is documented under
[`github/codeql-action/upload-sarif`](https://github.com/github/codeql-action#using-the-codeql-action).
The bindings produce one SARIF run per call; the action handles
the upload to the repository's Code Scanning alerts.

## Which spaces produce findings

`to_sarif` emits a finding at every space — the file unit, each
container, and each leaf function or closure — whose **own** value
breaches its limit, exactly matching `bca check --report-format sarif`. For most
metrics the JSON headline at a space already is that space's own value.
The five subtree-aggregate metrics — `cyclomatic`,
`cyclomatic.modified`, `cognitive`, `abc` and `nargs` — additionally
expose a `sum` / `magnitude` / `total` rolled up across child spaces;
the binding reads their per-space `value` field instead, so it reports
an interior breach (for example a function whose own complexity breaches
even though a nested closure's does not) without being fooled by the
larger aggregate. For `nargs` that means a function is gated on its own
parameter list, exactly as `bca check` has been since
[#1196](https://github.com/dekobon/big-code-analysis/issues/1196); a
closure with its own space produces its own finding rather than
inflating the enclosing function's.

Unit findings carry `logicalLocations: [{"fullyQualifiedName":
"<file>"}]`. Every other space carries its qualified symbol. Within
that symbol, a closure/lambda (the `<anonymous>` name every grammar
emits) and the `None`-name parse-failure case both collapse to
`<anon@L{start_line}>`, matching the CLI's `space_segment`.

## See also

* [Batch processing](batch.md) — the natural source of input
  iterables for `to_sarif`; `AnalysisFailure` entries are skipped
  silently.
* [Metric selection](metrics.md) — threshold names are a closed
  set independent of `metrics=`; requesting a narrower metric
  suite while gating on a dropped threshold yields an empty
  SARIF run.
* [Error handling](errors.md) — the typed exceptions `to_sarif`
  raises for bad caller input (`TypeError` / `ValueError`).
