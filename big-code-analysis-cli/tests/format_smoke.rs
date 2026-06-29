//! End-to-end format-validity smoke tests.
//!
//! Each test runs the binary against a small Rust fixture in a
//! `tempfile::tempdir()`, captures stdout, and pipes the captured
//! bytes through the matching format validator (the duplicates of
//! the lib-crate helpers under
//! `big-code-analysis-cli/tests/common/validators.rs`). This catches
//! CLI-dispatch bugs that bypass the writer-level tests in the lib
//! crate (e.g., a routing regression that emits Checkstyle XML when
//! `--output-format sarif` was requested).
//!
//! The offender formats (Checkstyle, SARIF) are emitted by
//! `bca check --output-format <fmt>`. Each test sets a tight
//! threshold against a deliberately branchy Rust fixture so the
//! `check` walk produces at least one offender record; the resulting
//! document is run through the format validator. (Clean runs are
//! valid input for the validators too, but exercising the document
//! with real offender records catches more dispatch regressions —
//! e.g. a route that emits an empty SARIF run even when offenders
//! were found.)

// The cross-format round-trip tests (#543) compare integer-valued
// metrics. After #530 those serialize as integers (TOML/CBOR `Integer`),
// so the extractors coerce them to f64 for a uniform compare; the counts
// are tiny and exact in f64. Exact equality is the intended assertion —
// float-magnitude metrics are deliberately not compared, and the
// integer→f64 coercion cannot lose precision at these magnitudes.
#![allow(clippy::float_cmp, clippy::cast_precision_loss)]

use assert_cmd::Command;
use big_code_analysis::CSV_HEADER;
use predicates::prelude::*;
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

mod common;
use common::validators::{assert_checkstyle_well_formed_and_structural, validate_sarif};

/// Hermetic `bca` builder: anchors the process cwd at `dir` (a
/// `tempfile::tempdir()` with no `.git` ancestor) so `bca check` cannot
/// auto-discover the repo's own `bca.toml` / `.bca-baseline.toml` and
/// filter the offender document against repo state (#491).
fn cli(dir: &TempDir) -> Command {
    common::cli_in(dir.path())
}

fn write_rust_fixture(dir: &TempDir) -> String {
    let path = dir.path().join("fixture.rs");
    let mut f = std::fs::File::create(&path).expect("create fixture");
    f.write_all(b"fn f(x: u32) -> u32 { if x > 0 { x } else { 0 } }\n")
        .expect("write fixture");
    path.to_str().expect("fixture path is utf-8").to_string()
}

fn run_metrics(dir: &TempDir, format: &str, fixture_path: &str) -> String {
    let output = cli(dir)
        .args(["metrics", "--paths", fixture_path, "-O", format])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(output).expect("CLI output is UTF-8")
}

/// Run `bca check --threshold cyclomatic=1 --output-format <fmt>
/// --no-fail` so the walk produces offender records (branchy fixture
/// vs cyclomatic=1) without bumping the exit code. Returns the
/// document stdout.
fn run_check_offender_doc(dir: &TempDir, format: &str, fixture_path: &str) -> String {
    let output = cli(dir)
        .args([
            "check",
            "--paths",
            fixture_path,
            "--threshold",
            "cyclomatic=1",
            "--output-format",
            format,
            "--no-fail",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(output).expect("CLI output is UTF-8")
}

/// #661: `metrics`/`ops --output` without a structured `--format` errors
/// (exit 1) instead of silently writing nothing on exit 0 — mirroring the
/// #600 fix on `check`. A given `--format` still writes; the text default
/// without `--output` still streams to stdout.
#[test]
fn metrics_ops_output_without_format_errors() {
    let dir = TempDir::new().unwrap();
    let fixture = write_rust_fixture(&dir);
    let out_dir = dir.path().join("out");
    for command in ["metrics", "ops"] {
        // `--output` under the default text format → exit 1, clear message.
        cli(&dir)
            .args([
                command,
                "--paths",
                &fixture,
                "--output",
                out_dir.to_str().unwrap(),
            ])
            .assert()
            .failure()
            .code(1)
            .stderr(predicate::str::contains("needs a structured format"));
        // With a structured format, `--output` writes as before.
        cli(&dir)
            .args([
                command,
                "--paths",
                &fixture,
                "-O",
                "json",
                "--output",
                out_dir.to_str().unwrap(),
            ])
            .assert()
            .success();
        // The text default without `--output` still streams to stdout.
        cli(&dir)
            .args([command, "--paths", &fixture])
            .assert()
            .success();
    }
}

#[test]
fn cli_check_sarif_output_validates_against_schema() {
    let dir = TempDir::new().unwrap();
    let fixture = write_rust_fixture(&dir);
    let out = run_check_offender_doc(&dir, "sarif", &fixture);
    if let Err(violations) = validate_sarif(&out) {
        panic!(
            "SARIF schema violations from CLI output:\n  {}\n\nfull document:\n{}",
            violations.join("\n  "),
            out,
        );
    }
    // The fixture has an `if` branch, so cyclomatic=1 produces an
    // offender. Guard against a routing regression that would emit
    // an empty results array: parse the JSON and require at least one
    // entry in `runs[0].results[]`. (A substring check on `"results"`
    // would still match the empty-array case `"results": []`.)
    let doc: serde_json::Value = serde_json::from_str(&out).expect("SARIF stdout parses as JSON");
    let results = doc["runs"][0]["results"]
        .as_array()
        .expect("runs[0].results is an array");
    assert!(
        !results.is_empty(),
        "expected at least one SARIF result for branchy fixture; doc was:\n{out}",
    );
}

#[test]
fn cli_check_checkstyle_output_is_well_formed() {
    let dir = TempDir::new().unwrap();
    let fixture = write_rust_fixture(&dir);
    let out = run_check_offender_doc(&dir, "checkstyle", &fixture);
    assert_checkstyle_well_formed_and_structural(&out);
    // Same routing-regression guard as the SARIF test: an empty
    // `<checkstyle version="4.3"/>` document is well-formed but
    // wouldn't catch a route that drops offenders silently. With
    // `cyclomatic=1` against a fixture that has an `if` branch, the
    // document must carry at least one `<file>` element with an
    // `<error>` child carrying the cyclomatic metric name.
    assert!(
        out.contains("<file"),
        "expected at least one <file> element in checkstyle output; out was:\n{out}",
    );
    assert!(
        out.contains("cyclomatic"),
        "expected cyclomatic metric in checkstyle output; out was:\n{out}",
    );
}

#[test]
fn cli_csv_output_round_trips_through_csv_crate() {
    // CSV is a per-file format. Re-parse the captured stdout via
    // the same `csv` crate the project already uses; reject any row
    // whose field count differs from CSV_HEADER, and require at least
    // one data row so we know real metric content is being emitted.
    let dir = TempDir::new().unwrap();
    let fixture = write_rust_fixture(&dir);
    let out = run_metrics(&dir, "csv", &fixture);

    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(out.as_bytes());

    let mut rows = 0;
    for record in rdr.records() {
        let record = record.expect("CLI CSV output round-trips through csv::Reader");
        // Pin against the public CSV_HEADER constant rather than the
        // first row's width — catches a regression that emits the
        // wrong number of columns consistently across all rows.
        assert_eq!(
            record.len(),
            CSV_HEADER.len(),
            "row {rows} field count {} differs from CSV_HEADER ({})",
            record.len(),
            CSV_HEADER.len(),
        );
        rows += 1;
    }
    // Header row plus at least one data row.
    assert!(
        rows >= 2,
        "expected header + at least one data row in CLI csv output, got {rows} rows"
    );
}

#[test]
fn cli_check_code_climate_output_matches_gitlab_shape() {
    // Routing + spec-compliance smoke for `--output-format code-climate`.
    // Mirrors the SARIF/Checkstyle tests above: branchy fixture +
    // tight threshold so the document carries real offenders, then
    // assert it conforms to GitLab's strict subset of the Code
    // Climate JSON spec (single array of objects, with each entry
    // carrying the five required fields and `severity` from the
    // GitLab enum).
    const REQUIRED_KEYS: &[&str] = &[
        "description",
        "check_name",
        "fingerprint",
        "severity",
        "location",
    ];
    const SEVERITY_ENUM: &[&str] = &["info", "minor", "major", "critical", "blocker"];

    let dir = TempDir::new().unwrap();
    let fixture = write_rust_fixture(&dir);
    let out = run_check_offender_doc(&dir, "code-climate", &fixture);

    let doc: serde_json::Value =
        serde_json::from_str(&out).expect("code-climate stdout parses as JSON");
    let issues = doc
        .as_array()
        .expect("code-climate output must be a JSON array");
    assert!(
        !issues.is_empty(),
        "expected at least one code-climate issue for branchy fixture; doc was:\n{out}",
    );

    for (i, issue) in issues.iter().enumerate() {
        for key in REQUIRED_KEYS {
            assert!(
                issue.get(*key).is_some(),
                "issue[{i}] missing required key {key:?}; entry was:\n{issue}",
            );
        }
        let description = issue["description"]
            .as_str()
            .expect("description is a string");
        assert!(
            !description.is_empty(),
            "issue[{i}] description must not be empty (GitLab MR widget would render a blank row)",
        );
        let severity = issue["severity"].as_str().expect("severity is a string");
        assert!(
            SEVERITY_ENUM.contains(&severity),
            "issue[{i}] severity {severity:?} not in GitLab enum {SEVERITY_ENUM:?}",
        );
        let path = issue["location"]["path"]
            .as_str()
            .expect("path is a string");
        assert!(
            !path.starts_with("./"),
            "issue[{i}] location.path {path:?} starts with forbidden './'",
        );
        assert!(
            !path.contains('\\'),
            "issue[{i}] location.path {path:?} contains backslash",
        );
        assert!(
            issue["check_name"]
                .as_str()
                .expect("check_name is a string")
                .starts_with("big-code-analysis/"),
            "issue[{i}] check_name not namespaced",
        );
        let fp = issue["fingerprint"]
            .as_str()
            .expect("fingerprint is a string");
        assert_eq!(
            fp.len(),
            32,
            "issue[{i}] fingerprint must be 32 hex chars, got {fp:?}",
        );
        let begin = issue["location"]["lines"]["begin"]
            .as_u64()
            .expect("lines.begin is an integer");
        assert!(
            begin >= 1,
            "issue[{i}] lines.begin must be >= 1, got {begin}",
        );
    }

    // GitLab forbids a BOM at the start of the artifact.
    assert!(
        !out.starts_with('\u{FEFF}'),
        "code-climate output must not start with a BOM",
    );
}

/// #995 / #530: integer-valued metrics (counts, sums, min/max) must
/// serialize as JSON *integers* (`3`), never floats (`3.0`). This is the
/// exact invariant the `python-cli-wheels.yml` wheel smoke asserts against
/// the packaged binary — and the one that silently rotted (the smoke still
/// expected the pre-2.0 `"3.0"`) until the `v2.0.0` tag forced the smoke to
/// run (#995). Pinning it in a per-PR test means a regression of the `u64`
/// wire fields (a serde rename, a field-type flip back to `f64`) reds a PR
/// check instead of a release.
///
/// The round-trip tests below coerce every metric through `as_f64()`, so
/// they pass for both `3` and `3.0` and cannot catch this. The
/// distinguishing check is `is_u64()`: `serde_json` parses `3` as an integer
/// (`is_u64() == true`) but `3.0` as a float (`is_u64() == false`).
#[test]
fn cli_metrics_json_serializes_integer_metrics_as_integers() {
    let dir = TempDir::new().unwrap();
    let fixture = write_rust_fixture(&dir);
    let out = run_metrics(&dir, "json", &fixture);
    let doc: serde_json::Value = serde_json::from_str(&out).expect("metrics JSON parses");

    // expected: the fixture's unit-level cyclomatic.sum is 3 (function
    // entry + `if` + `else`) and loc.sloc is 1 — both integral, so both
    // must serialize as JSON integers, not floats.
    let cyclomatic_sum = &doc["metrics"]["cyclomatic"]["sum"];
    assert!(
        cyclomatic_sum.is_u64(),
        "cyclomatic.sum must serialize as a JSON integer (#530), got {cyclomatic_sum} in:\n{out}",
    );
    assert_eq!(cyclomatic_sum.as_u64(), Some(3), "cyclomatic.sum value");

    let sloc = &doc["metrics"]["loc"]["sloc"];
    assert!(
        sloc.is_u64(),
        "loc.sloc must serialize as a JSON integer (#530), got {sloc} in:\n{out}",
    );

    // Negative control: a genuinely fractional metric (`halstead.volume`,
    // ~58.81 for this fixture) is an `f64` wire field and MUST stay a JSON
    // float. This proves `is_u64()` above actually discriminates integers
    // from floats rather than passing vacuously — if every metric
    // serialized as a float (the pre-#530 regression), this assertion would
    // still hold while the integer checks above failed, and vice versa.
    let volume = &doc["metrics"]["halstead"]["volume"];
    assert!(
        volume.is_f64() && !volume.is_u64(),
        "halstead.volume must serialize as a JSON float, got {volume} in:\n{out}",
    );
}

// --- TOML / YAML / CBOR round-trip smoke tests (issue #543) ---------------
//
// These three formats had no validity / round-trip coverage, so a shape
// regression (e.g. a serde rename, a struct-field reorder that makes TOML
// emit an array-of-tables before a scalar, or a CBOR routing bug) would
// ship silently. Each test emits the format for the same Rust fixture,
// parses the bytes back into the format's own `Value` type, asserts the
// top-level `FuncSpace` keys survive, and checks that an integer-valued
// metric round-trips to the same number JSON produced. A final test pins
// cross-format agreement so no single serializer can drift on its own.
//
// The numbers compared are integer-valued metrics (`loc.sloc`,
// `cyclomatic.sum`) that serde emits as f64; exact f64 equality is safe
// here because small integers are represented exactly. Float-magnitude
// metrics (volume / effort) are deliberately avoided per the bit-brittle
// guidance in AGENTS.md.

/// Metric value JSON emits for the representative fixture at
/// `metrics.<group>.<key>`. Both are integer-valued floats, so exact
/// equality survives a serde round-trip through every format.
const SLOC_KEY: (&str, &str) = ("loc", "sloc");
const CYCLOMATIC_KEY: (&str, &str) = ("cyclomatic", "sum");

/// Top-level `FuncSpace` keys every serializer must preserve.
const STRUCTURAL_KEYS: &[&str] = &["name", "kind", "spaces", "metrics"];

/// Run `bca metrics -O <format> --output-dir <outdir>` and return the
/// single emitted file's bytes. Output filenames mirror the
/// (path-cleaned) input path under `outdir`, so rather than reconstruct
/// the production `handle_path` mapping we walk `outdir` for the one file
/// with `extension`. CBOR has no stdout path, so this file route is the
/// only way to smoke it. Uses `--output-dir` (the per-file-tree mode,
/// #669) so each document keeps its single-`FuncSpace` shape — `--output`
/// would emit one aggregate array instead.
fn run_metrics_to_file(
    dir: &TempDir,
    format: &str,
    fixture_path: &str,
    extension: &str,
) -> Vec<u8> {
    let out_dir = TempDir::new().expect("create metrics output dir");
    cli(dir)
        .args([
            "metrics",
            "--paths",
            fixture_path,
            "-O",
            format,
            "--output-dir",
            out_dir.path().to_str().expect("outdir path is utf-8"),
        ])
        .assert()
        .success();
    let file = find_single_file_with_extension(out_dir.path(), extension);
    std::fs::read(&file).expect("read emitted metrics file")
}

/// Recursively collect the single file under `root` whose name ends in
/// `extension`. Panics if zero or more than one match, so a routing
/// regression that emits the wrong (or no) file fails loudly.
fn find_single_file_with_extension(root: &std::path::Path, extension: &str) -> PathBuf {
    let mut matches = Vec::new();
    collect_files_with_extension(root, extension, &mut matches);
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one *{extension} file under {}, found {matches:?}",
        root.display(),
    );
    matches.pop().expect("one match present")
}

fn collect_files_with_extension(dir: &std::path::Path, extension: &str, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).expect("read output dir");
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_files_with_extension(&path, extension, out);
        } else if path.to_str().is_some_and(|name| name.ends_with(extension)) {
            out.push(path);
        }
    }
}

/// The JSON value of `metrics.<group>.<key>` for the fixture, used as the
/// cross-format reference. JSON is already covered by the lib-crate
/// writer tests, so it is the trusted baseline here.
fn json_metric(dir: &TempDir, fixture: &str, key: (&str, &str)) -> f64 {
    let out = run_metrics(dir, "json", fixture);
    let doc: serde_json::Value = serde_json::from_str(&out).expect("metrics JSON parses");
    doc["metrics"][key.0][key.1]
        .as_f64()
        .expect("JSON metric is a number")
}

#[test]
fn cli_metrics_yaml_round_trips() {
    let dir = TempDir::new().unwrap();
    let fixture = write_rust_fixture(&dir);
    let out = run_metrics(&dir, "yaml", &fixture);

    let doc: serde_yaml::Value = serde_yaml::from_str(&out).expect("metrics YAML parses");
    let map = doc.as_mapping().expect("YAML root is a mapping");
    for key in STRUCTURAL_KEYS {
        assert!(
            map.contains_key(serde_yaml::Value::String((*key).to_string())),
            "YAML output missing top-level key {key:?}; doc was:\n{out}",
        );
    }

    let sloc = doc["metrics"][SLOC_KEY.0][SLOC_KEY.1]
        .as_f64()
        .expect("YAML loc.sloc is a number");
    assert_eq!(
        sloc,
        json_metric(&dir, &fixture, SLOC_KEY),
        "YAML loc.sloc diverged from JSON",
    );
}

#[test]
fn cli_metrics_toml_round_trips() {
    // This is the test the issue calls out: `FuncSpace` interleaves the
    // array-of-tables `spaces` ahead of the `metrics` table, so a naive
    // TOML emitter would emit a table after an array-of-tables and
    // `toml::from_str` would reject it (`ValueAfterTable`). Parsing the
    // emitted document back without error pins that the document stays
    // valid TOML regardless of struct field order.
    let dir = TempDir::new().unwrap();
    let fixture = write_rust_fixture(&dir);
    let out = run_metrics(&dir, "toml", &fixture);

    let doc: toml::Value = toml::from_str(&out).unwrap_or_else(|e| {
        panic!("metrics TOML failed to parse ({e}); doc was:\n{out}");
    });
    let table = doc.as_table().expect("TOML root is a table");
    for key in STRUCTURAL_KEYS {
        assert!(
            table.contains_key(*key),
            "TOML output missing top-level key {key:?}; doc was:\n{out}",
        );
    }

    let sloc = toml_num(&doc["metrics"][CYCLOMATIC_KEY.0][CYCLOMATIC_KEY.1])
        .expect("TOML cyclomatic.sum is numeric");
    assert_eq!(
        sloc,
        json_metric(&dir, &fixture, CYCLOMATIC_KEY),
        "TOML cyclomatic.sum diverged from JSON",
    );
}

#[test]
fn cli_metrics_cbor_round_trips() {
    // CBOR errors on stdout (it is binary), so it is emitted to a file
    // and read back. Round-trip through `ciborium::value::Value` and
    // assert the same structural keys + numeric fidelity as the text
    // formats.
    let dir = TempDir::new().unwrap();
    let fixture = write_rust_fixture(&dir);
    let bytes = run_metrics_to_file(&dir, "cbor", &fixture, ".cbor");

    let doc: ciborium::value::Value =
        ciborium::from_reader(bytes.as_slice()).expect("metrics CBOR parses into a Value");
    let ciborium::value::Value::Map(map) = &doc else {
        panic!("CBOR root is not a map: {doc:?}");
    };
    for key in STRUCTURAL_KEYS {
        let target = ciborium::value::Value::Text((*key).to_string());
        assert!(
            map.iter().any(|(k, _)| *k == target),
            "CBOR output missing top-level key {key:?}",
        );
    }

    let sloc = cbor_metric(&doc, SLOC_KEY).expect("CBOR loc.sloc present and numeric");
    assert_eq!(
        sloc,
        json_metric(&dir, &fixture, SLOC_KEY),
        "CBOR loc.sloc diverged from JSON",
    );
}

#[test]
fn cli_metrics_all_formats_agree_on_metric() {
    // Cross-format guard: the same integer-valued metric must be equal
    // across JSON, YAML, TOML, and CBOR for one tree. A serializer that
    // drifts on its own (a rename, a unit conversion, a rounding bug)
    // breaks this even if its own round-trip test still passes.
    let dir = TempDir::new().unwrap();
    let fixture = write_rust_fixture(&dir);

    let json = json_metric(&dir, &fixture, CYCLOMATIC_KEY);

    let yaml_out = run_metrics(&dir, "yaml", &fixture);
    let yaml: serde_yaml::Value = serde_yaml::from_str(&yaml_out).expect("YAML parses");
    let yaml = yaml["metrics"][CYCLOMATIC_KEY.0][CYCLOMATIC_KEY.1]
        .as_f64()
        .expect("YAML metric is numeric");

    let toml_out = run_metrics(&dir, "toml", &fixture);
    let toml: toml::Value = toml::from_str(&toml_out).expect("TOML parses");
    let toml = toml_num(&toml["metrics"][CYCLOMATIC_KEY.0][CYCLOMATIC_KEY.1])
        .expect("TOML metric is numeric");

    let cbor_bytes = run_metrics_to_file(&dir, "cbor", &fixture, ".cbor");
    let cbor_doc: ciborium::value::Value =
        ciborium::from_reader(cbor_bytes.as_slice()).expect("CBOR parses");
    let cbor = cbor_metric(&cbor_doc, CYCLOMATIC_KEY).expect("CBOR metric is numeric");

    assert_eq!(json, yaml, "JSON vs YAML cyclomatic.sum");
    assert_eq!(json, toml, "JSON vs TOML cyclomatic.sum");
    assert_eq!(json, cbor, "JSON vs CBOR cyclomatic.sum");
}

/// Extract `metrics.<group>.<key>` from a parsed CBOR document as f64.
/// `ciborium::value::Value` has no string-indexing sugar, so the map
/// walk is explicit. Accepts both `Integer` (integral metrics — counts/
/// sums/min/max) and `Float` (ratios/averages/derived scores) so the
/// check is agnostic to a metric's numeric representation.
fn cbor_metric(doc: &ciborium::value::Value, key: (&str, &str)) -> Option<f64> {
    let metrics = cbor_get(doc, "metrics")?;
    let group = cbor_get(metrics, key.0)?;
    match cbor_get(group, key.1)? {
        ciborium::value::Value::Float(f) => Some(*f),
        ciborium::value::Value::Integer(i) => Some(i128::from(*i) as f64),
        _ => None,
    }
}

/// Read a `toml::Value` metric as f64, accepting both TOML integers
/// (integral metrics) and floats (ratios/averages), mirroring
/// [`cbor_metric`] so the round-trip checks are representation-agnostic.
fn toml_num(value: &toml::Value) -> Option<f64> {
    value
        .as_float()
        .or_else(|| value.as_integer().map(|i| i as f64))
}

fn cbor_get<'a>(
    value: &'a ciborium::value::Value,
    key: &str,
) -> Option<&'a ciborium::value::Value> {
    let ciborium::value::Value::Map(map) = value else {
        return None;
    };
    let target = ciborium::value::Value::Text(key.to_string());
    map.iter().find_map(|(k, v)| (*k == target).then_some(v))
}
