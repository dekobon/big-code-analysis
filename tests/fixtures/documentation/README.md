# Documentation characterization fixtures

These fixtures let contributors check documentation recognition and metric preservation before
changing an adapter. They assume the distinction between source attachment and documentation line
counting in the [extraction contract](../../../docs/development/documentation_extraction.md#recognition).

`sources/` contains raw inputs. Read them as bytes: several omit the final newline or contain CRLF.
The directory's Git attributes disable newline conversion. AST test-support provides the same raw
reader to AST recognition tests and root metric tests.

## Requirements and observations

- `inventory.json` records the contract's primary owner, fragment ranges, profile and nonempty text
  for every source dialect. Helper grammars explicitly require `NoDeclarationModel`.
- `forms.json` records additional source requirements and qualifications. The
  [complete form inventory](../../../docs/development/documentation_extraction_cases.md#ownership)
  remains authoritative for later adapter work. Text and attachment requirements are independently
  specified; they are not generated from runtime capabilities.
- `legacy.json` anchors integer LOC values, physical documentation rows and ordered nested spaces.
  Rows are zero-based. An absent `excluded` entry means the `included` expectation applies in both
  pruning modes. These expectations preserve existing behavior, including named limitations.
- `reference/` holds selected whole-metric wire records from the maintained `verbatim-oracle`.
  `selection.json` fixes the exact language/input keys. Its manifest records input hashes, binary
  hashes, grammar pins and views. `provenance.json` records revisions and reference evidence.

Extraction is not implemented here. `assert_primary_extraction` is the shared assertion for future
adapters. It rejects unsupported or empty results for source languages. Current tests run production
parsing, recognition and metrics; they do not claim extraction conformance.

Rust outer docs and comments inside pruned attribute runs disappear with the item. Plain prefixes
and inner docs survive. The attribute-value reach case preserves the documented
[#1786 limitation](https://github.com/dekobon/big-code-analysis/issues/1786).
Java's empty block has a source requirement distinct from the older block billed by legacy DLOC.
Mixed Perl POD bills comment-only subregions inside a documentation token, although they are not
source prose. Tcl/iRules CRLF continuation cases preserve
[#1802](https://github.com/dekobon/big-code-analysis/issues/1802).

## Replay

Run from the repository root. The checked records use repository-relative input names passed to the
oracle before parsing. No observed text, numeric value, row order or output path is rewritten.
Binary and fixture-root paths in the capture manifest are provenance, not metric values.

```bash
cargo test -p big-code-analysis-ast --all-features --lib documentation_
cargo test -p big-code-analysis --all-features --lib documentation_
python3 -B -m unittest -q utils/drift-oracle-test.py utils/drift-compare-test.py
cargo build -p big-code-analysis-cli -p big-code-analysis-bench --bin bca --bin verbatim-oracle
out=$(mktemp -d /tmp/opencode/bca-documentation-replay.XXXXXX)
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

The selected capture exercises both MI documentation modes with all metrics and MI-only dependency
closure, pruning, and the combined pruning/try-count choice. The root test checks whole serialized
space trees as well as integer and row anchors. No external documentation generator runs in CI.

To extend the fixtures, specify source outcomes from the contract or a versioned reference first.
Add raw source and independent legacy anchors, select the input explicitly, then review its capture.
Use `utils/perturb.py` with the tracked characterization manifests to verify decision-bearing guards
on a clean checkpoint. The manifests state their sweep scope; they do not claim unique guards.
