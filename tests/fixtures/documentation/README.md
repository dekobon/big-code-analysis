# Documentation characterization fixtures

These fixtures let contributors check documentation recognition and metric preservation before
changing an adapter. They assume the distinction between source attachment and documentation line
counting in the [extraction contract](../../../docs/development/documentation_extraction.md#recognition).

`sources/` contains raw inputs. Read them as bytes: several omit the final newline or contain CRLF.
The directory's [Git attributes](https://git-scm.com/docs/gitattributes) disable newline conversion.
AST test-support provides the same raw reader to AST recognition tests and root metric tests.

## Requirements and observations

- `inventory.json` records the contract's primary owner, fragment ranges, profile and nonempty text
  for every source dialect. `selected_owner_kind` names the required declaration's syntax kind;
  `owner_kinds` separately records every observed node sharing its range. Helper grammars require
  `NoDeclarationModel`, with null owner and selected kind.
- `forms.json` records additional source requirements and qualifications. The
  [complete form inventory](../../../docs/development/documentation_extraction_cases.md#ownership)
  remains authoritative for later adapter work. Text and attachment requirements are independently
  specified; they are not generated from runtime capabilities.
- `legacy.json` anchors integer LOC values, physical documentation rows and ordered nested spaces.
  Rows are zero-based. An absent `excluded` entry means the `included` expectation applies in both
  pruning modes. Optional `ploc_rows` and `cloc_rows` arrays belong to that anchor, independent of
  the filename. These expectations preserve existing behavior, including named limitations.
- `reference/` holds selected whole-metric wire records from the maintained
  [`verbatim-oracle`](../../../big-code-analysis-bench/src/bin/verbatim_oracle.rs).
  `selection.json` fixes the exact language/input keys. `reference/manifest.json` records input
  hashes, binary hashes, grammar pins and views. `provenance.json` records revisions, the relationship
  between the D01 baseline and semantic base, and reference evidence.

Extraction is available through the direct-AST collector and a hidden root method;
adapter support remains partial. `assert_primary_extraction` is the shared assertion for
adapter implementations. It rejects unsupported or empty results and wrong selected kinds for source languages.
For example, Python's primary projection is:

```json
[{"fragments":[[13,20]],"owner":[0,29],"owner_kind":"function_definition","text":"audit"}]
```

The module root shares that range but is the wrong owner. Adapter tests exercise extraction
where implemented; an inventory row alone does not claim conformance. Lua first-file and
Python chained assignment controls add raw requirements and recognition tests without extending
`selection.json`. Their source associations remain requirements for future adapters.

- Rust outer docs and comments inside pruned attribute runs disappear with the item. Plain prefixes
  and inner docs survive, as described by the [legacy pruning contract][recognition]. The
  attribute-value reach case preserves the [#1786 limitation][rust-reach].
- Java's empty block has a [source requirement][text] distinct from the older block billed by legacy
  DLOC (documentation lines of code).
- Mixed Perl POD bills comment-only subregions inside a documentation token, although the
  [source profile][profiles] excludes them from prose.
- Tcl/iRules CRLF continuation cases preserve [#1802][tcl-crlf]. The [ownership requirements][profiles]
  remain independent of those legacy row counts.

[recognition]: ../../../docs/development/documentation_extraction.md#recognition
[text]: ../../../docs/development/documentation_extraction.md#text
[profiles]: ../../../docs/development/documentation_extraction.md#profiles
[rust-reach]: https://github.com/dekobon/big-code-analysis/issues/1786
[tcl-crlf]: https://github.com/dekobon/big-code-analysis/issues/1802

## Replay

Run from the repository root. The checked records use repository-relative input names passed to the
oracle before parsing. No observed text, numeric value, row order or output path is rewritten.
Binary and fixture-root paths in the capture manifest are provenance, not metric values.

```bash
cargo test -p big-code-analysis-ast --all-features --lib documentation_
cargo test -p big-code-analysis --all-features --lib documentation_
python3 -B -m unittest -q utils/drift-oracle-test.py utils/drift-compare-test.py
cargo build -p big-code-analysis-cli -p big-code-analysis-bench --bin bca --bin verbatim-oracle
out=$(mktemp -d "${TMPDIR:-/tmp}/bca-documentation-replay.XXXXXX")
views=(
  verbatim_metrics_mi_charges_docs_false
  verbatim_metrics_mi_charges_docs_true
  verbatim_metrics_exclude_tests_mi_charges_docs_false
  verbatim_metrics_exclude_tests_mi_charges_docs_true
  verbatim_metrics_only_mi_mi_charges_docs_false
  verbatim_metrics_only_mi_mi_charges_docs_true
  verbatim_metrics_only_mi_exclude_tests_mi_charges_docs_false
  verbatim_metrics_only_mi_exclude_tests_mi_charges_docs_true
  verbatim_metrics_only_mi_exclude_tests_no_count_try_mi_charges_docs_false
  verbatim_metrics_only_mi_exclude_tests_no_count_try_mi_charges_docs_true
)
args=()
for view in "${views[@]}"; do args+=(--view "$view"); done
python3 -B utils/drift-oracle.py --bca target/debug/bca \
  --verbatim target/debug/verbatim-oracle --out "$out" \
  --root documentation=tests/fixtures/documentation/sources \
  --selection tests/fixtures/documentation/selection.json \
  --relative-paths --jobs 2 "${args[@]}"
python3 -B utils/drift-compare.py tests/fixtures/documentation/reference "$out"
```

The selected capture exercises both [maintainability index][mi] (MI) documentation modes, pruning,
and the combined pruning/try-count choice. MI depends on LOC, cyclomatic and Halstead metrics. An MI-only
request therefore computes and serializes those dependencies too; this is its dependency closure.
The root test checks whole serialized space trees as well as integer and row anchors. No external
documentation generator runs in CI.

[mi]: https://learn.microsoft.com/en-us/visualstudio/code-quality/code-metrics-maintainability-index-range-and-meaning

See [metric selection](../../../big-code-analysis-book/src/library/selecting-metrics.md) for the
library's dependency rules.

AST fixture readers require a repository checkout at runtime. The test-support feature still
compiles without external fixtures. When unpublished grammar versions block package verification,
run `python3 -B utils/documentation-package-check.py` for an isolated all-features build and rustdoc
check. This preserves workspace manifests and local grammar sources; it does not verify registry
resolution or the normalized package manifest. `--baseline REV` substitutes the helper from that
revision to demonstrate the old compile-time dependency on the omitted fixture.

To extend the fixtures, specify source outcomes from the contract or a versioned reference first.
Add raw source and independent legacy anchors, select the input explicitly, then review its capture.
Use `utils/perturb.py` with the tracked characterization manifests to verify decision-bearing guards
on a clean checkpoint. Each mutation must fail its expected tests. Other tests can catch the same
mutation, and these focused sweeps do not establish that a guard is unique across dependent crates.
