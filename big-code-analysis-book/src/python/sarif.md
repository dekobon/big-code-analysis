# SARIF output

`bca.to_sarif(result, *, thresholds=None)` renders an analysis
result (or an iterable of them) into a [SARIF
2.1.0](https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html)
JSON document, ready for upload to GitHub Code Scanning or any
other SARIF consumer. The output is produced by the same Rust
writer that backs `bca check --report-format sarif`, so the schema URL, tool
driver name / version, and rule descriptions match the CLI
byte-for-byte.

```python
{{#include ../../../big-code-analysis-py/examples/sarif_output.py:17:33}}
```

`to_sarif` accepts:

* A single `dict` returned by `bca.analyze` or
  `bca.analyze_source`.
* Any iterable yielding such dicts and / or `bca.AnalysisFailure`
  instances (the natural shape of `bca.analyze_batch`'s return
  value). `AnalysisFailure` entries are skipped silently — they
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

`to_sarif` emits a finding at every space — the file unit, each
container, and each leaf function or closure — whose **own** value
breaches its limit, exactly matching `bca check --report-format sarif`. For most
metrics the JSON headline at a space already is that space's own value.
The four subtree-aggregate metrics — `cyclomatic`,
`cyclomatic.modified`, `cognitive`, and `abc` — additionally expose a
`sum` / `magnitude` rolled up across child spaces; the binding reads
their per-space `value` field instead, so it reports an interior breach
(for example a function whose own complexity breaches even though a
nested closure's does not) without being fooled by the larger
aggregate. Before the `value` field existed the binding could read only
the aggregate and so emitted these four only at leaf spaces, missing
genuine interior breaches the CLI reports (#958).

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
