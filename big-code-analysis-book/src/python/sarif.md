# SARIF output

`bca.to_sarif(result, *, thresholds=None)` renders an analysis
result (or an iterable of them) into a [SARIF
2.1.0](https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html)
JSON document, ready for upload to GitHub Code Scanning or any
other SARIF consumer. The output is produced by the same Rust
writer that backs `bca check -O sarif`, so the schema URL, tool
driver name / version, and rule descriptions match the CLI
byte-for-byte.

```python
{{#include ../../../big-code-analysis-py/examples/sarif_output.py:17:33}}
```

`to_sarif` accepts:

* A single `dict` returned by `bca.analyze` or
  `bca.analyze_source`.
* Any iterable yielding such dicts and / or `bca.AnalysisError`
  instances (the natural shape of `bca.analyze_batch`'s return
  value). `AnalysisError` entries are skipped silently — they
  represent files that could not be analysed, not findings.

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
arrays. This matches the CLI's posture: there are **no built-in
default thresholds**; every check run supplies its own limits.

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

## What "Unit" findings mean

`to_sarif` emits file-scope (unit-space) findings for every
metric whose JSON headline at the unit space matches the CLI's
per-space accessor (`loc.*`, `halstead.*`, `mi.*`, `nom`,
`nargs`, `nexits`, `tokens`, `abc`, `wmc`, `npm`, `npa`). The
three exceptions — `cyclomatic`, `cyclomatic.modified`,
`cognitive` — are skipped at the unit level because the JSON
exposes the aggregate `sum` across children while the CLI's
per-space accessor returns just the unit's own scalar.

Unit findings carry `logicalLocations: [{"fullyQualifiedName":
"<file>"}]`. Nameless non-unit spaces (rare parse-failure case)
carry `"<unnamed>"` — both matching the CLI's `function_token`
placeholders.

## See also

* [Batch processing](batch.md) — the natural source of input
  iterables for `to_sarif`; `AnalysisError` entries are skipped
  silently.
* [Metric selection](metrics.md) — threshold names are a closed
  set independent of `metrics=`; requesting a narrower metric
  suite while gating on a dropped threshold yields an empty
  SARIF run.
* [Error handling](errors.md) — the typed exceptions `to_sarif`
  raises for bad caller input (`TypeError` / `ValueError`).
