# Test fixtures: vendored format schemas

This directory holds external schemas that the integration test suite
validates emitted output against. Files are vendored, never fetched at
test time, so the suite stays offline and reproducible.

## Files

### `sarif-2.1.0.json`

The [SARIF 2.1.0](https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html)
[JSON Schema](https://json-schema.org/) (Draft-07). Used by
`tests/output_formats/sarif_test.rs` via `include_str!` and the
[`jsonschema`](https://crates.io/crates/jsonschema) crate to validate
that every emitted SARIF document conforms to the spec.

- **Source**: <https://json.schemastore.org/sarif-2.1.0.json>
  (which 301-redirects to the schemastore-hosted copy of the OASIS
  canonical schema at
  <https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json>)
- **`$id`**: `https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json`
- **Dialect**: `http://json-schema.org/draft-07/schema#`
- **Fetched**: 2026-05-08
- **Size**: ~110 KB, self-contained (all `$ref` are internal `#/definitions/...`)

### `checkstyle-report-1.0.0.xsd`

The [Checkstyle](https://checkstyle.org/) 4.3 XML output schema.
Vendored as **documentation only**:
`tests/output_formats/checkstyle_test.rs` mirrors the constraints in a
[`quick-xml`](https://crates.io/crates/quick-xml)-driven structural
walker. The walker checks the constraints without an
[XSD](https://www.w3.org/XML/Schema) validator: no mature pure-Rust
one exists, and using [libxml2](https://gitlab.gnome.org/GNOME/libxml2)
would require the `libxml2-dev` system package on contributor machines
and CI runners.

- **Source**: <https://github.com/checkstyle/checkstyle/blob/master/config/checkstyle-report-1.0.0.xsd>
- **Origin PR**: [checkstyle#17532](https://github.com/checkstyle/checkstyle/pull/17532), merged 2025-08-03
- **Fetched**: 2026-05-08
- **Size**: ~1.6 KB, three complex types + severity enum

## Refresh procedure

When upstream publishes a new version of either schema, refresh both
the file and the metadata above in one commit:

```bash
# SARIF
curl -sL "https://json.schemastore.org/sarif-2.1.0.json" \
  -o tests/fixtures/sarif-2.1.0.json

# Checkstyle (uses gh because the file lives in a GitHub repo)
gh api repos/checkstyle/checkstyle/contents/config/checkstyle-report-1.0.0.xsd \
  --jq '.content' \
  | base64 -d \
  > tests/fixtures/checkstyle-report-1.0.0.xsd
```

Then:

1. Update the **Fetched** date(s) above.
2. Re-run `cargo test --workspace --all-features`. If new validation
   failures surface, the upstream schema added or tightened
   constraints; either fix the writer to match, or document the
   intentional deviation in the test that catches it.
3. Update `src/output/sarif.rs::SARIF_SCHEMA` and the structural
   walker in `tests/common/validators.rs` (and its CLI duplicate at
   `big-code-analysis-cli/tests/common/validators.rs`) if the SARIF
   or Checkstyle contracts shifted in incompatible ways.

## Why offline?

`make pre-commit` and CI must run without network access. Vendoring
keeps the schemas reproducible: no "tests pass on Friday, fail on
Monday" because schemastore.org changed something. The trade-off is a
manual refresh when an upstream schema changes.
