//! Integration tests for the `bca check` threshold engine.
//!
//! These tests drive the binary against tiny inline source fixtures so
//! they don't depend on any submodule. Each test exercises one branch of
//! the exit-code contract: 0 clean / 0 with --no-fail / 2 violations
//! / 1 tool error.

use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn cli() -> Command {
    Command::cargo_bin("bca").unwrap()
}

/// Rust function with cyclomatic complexity > 1: each branch contributes
/// to the count. Used by tests that need a guaranteed violation when
/// `cyclomatic` is given a tight limit.
const BRANCHY_RUST: &str = r#"
pub fn classify(n: i32) -> &'static str {
    if n < 0 {
        "neg"
    } else if n == 0 {
        "zero"
    } else if n < 10 {
        "small"
    } else if n < 100 {
        "medium"
    } else {
        "large"
    }
}
"#;

/// Rust function with cyclomatic == 1 (no branches). Threshold-clean for
/// any reasonable cyclomatic limit.
const TRIVIAL_RUST: &str = "
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
";

fn write_fixture(dir: &TempDir, name: &str, body: &str) -> String {
    let path = dir.path().join(name);
    fs::write(&path, body).expect("write fixture");
    path.to_str().expect("utf8 fixture path").to_string()
}

#[test]
fn check_clean_exits_zero_with_no_offenders() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "trivial.rs", TRIVIAL_RUST);

    cli()
        .args(["--paths", &path, "check", "--threshold", "cyclomatic=10"])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

#[test]
fn check_violation_exits_two_with_stable_stderr() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);

    cli()
        .args(["--paths", &path, "check", "--threshold", "cyclomatic=1"])
        .assert()
        .code(2)
        // The classify function exceeds cyclomatic=1; the offender line
        // must mention the file, function name, metric, and limit in the
        // documented format.
        .stderr(predicate::str::contains(&path))
        .stderr(predicate::str::contains("classify"))
        .stderr(predicate::str::contains("cyclomatic"))
        .stderr(predicate::str::contains("(limit 1)"));
}

#[test]
fn check_no_fail_keeps_exit_zero_but_still_reports() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);

    cli()
        .args([
            "--paths",
            &path,
            "check",
            "--threshold",
            "cyclomatic=1",
            "--no-fail",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("cyclomatic"))
        .stderr(predicate::str::contains("(limit 1)"));
}

#[test]
fn check_unknown_metric_exits_one_with_clear_error() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "trivial.rs", TRIVIAL_RUST);

    cli()
        .args(["--paths", &path, "check", "--threshold", "not_a_metric=1"])
        .assert()
        // Exit 1 (tool error), not 2 (threshold exceeded). This is the
        // pivot that lets CI distinguish "metric regression" from
        // "tool misconfigured".
        .code(1)
        .stderr(predicate::str::contains("unknown threshold metric"));
}

#[test]
fn check_requires_at_least_one_threshold() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "trivial.rs", TRIVIAL_RUST);

    cli()
        .args(["--paths", &path, "check"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no thresholds configured"));
}

#[test]
fn check_with_no_matching_files_exits_one() {
    // A directory that exists but contains no source files should produce
    // a tool error (exit 1), not a vacuous clean pass (exit 0). Otherwise
    // a typo in `--paths` silently green-lights CI.
    let dir = TempDir::new().unwrap();

    cli()
        .args([
            "--paths",
            dir.path().to_str().unwrap(),
            "check",
            "--threshold",
            "cyclomatic=10",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no input files matched"));
}

#[test]
fn check_reads_thresholds_from_toml_config() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let cfg_path = dir.path().join("thresholds.toml");
    fs::write(&cfg_path, "[thresholds]\ncyclomatic = 1\n").unwrap();

    cli()
        .args([
            "--paths",
            &path,
            "check",
            "--config",
            cfg_path.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cyclomatic"))
        .stderr(predicate::str::contains("(limit 1)"));
}

#[test]
fn check_cli_threshold_overrides_config() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let cfg_path = dir.path().join("thresholds.toml");
    // Config sets a tight limit; CLI flag relaxes it. The CLI must win,
    // so the run should pass cleanly.
    fs::write(&cfg_path, "[thresholds]\ncyclomatic = 1\n").unwrap();

    cli()
        .args([
            "--paths",
            &path,
            "check",
            "--config",
            cfg_path.to_str().unwrap(),
            "--threshold",
            "cyclomatic=100",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

#[test]
fn check_emits_one_line_per_metric_per_function() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);

    // Two thresholds tight enough that the same function violates both.
    // The contract is one line per (function, metric), so we expect at
    // least two lines for `classify` — one for each metric.
    let assert = cli()
        .args([
            "--paths",
            &path,
            "check",
            "--threshold",
            "cyclomatic=1",
            "--threshold",
            "cognitive=0",
        ])
        .assert()
        .code(2);
    let output = assert.get_output();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let cyclomatic_lines = stderr
        .lines()
        .filter(|l| l.contains("classify") && l.contains("cyclomatic"))
        .count();
    let cognitive_lines = stderr
        .lines()
        .filter(|l| l.contains("classify") && l.contains("cognitive"))
        .count();
    // Contract is exactly one line per (function, metric). `>= 1` would
    // silently accept a regression that double-emits — the recursion
    // descends into each child space once, and a stray double-recurse
    // would slip past the looser bound.
    assert!(
        cyclomatic_lines == 1 && cognitive_lines == 1,
        "expected exactly one line per (function, metric) for classify; \
         got cyclomatic={cyclomatic_lines}, cognitive={cognitive_lines}; stderr was:\n{stderr}",
    );
}

#[test]
fn check_uses_file_sentinel_for_top_level_space() {
    // The top-level space's name is the file path (post #128), so a
    // naive emission would produce `path:1-N: path: loc.sloc = ...`
    // — the path doubled. The contract substitutes the literal
    // `<file>` in the function slot so file-level violations on
    // aggregating metrics like `loc.sloc` are visually distinct
    // and the path doesn't repeat.
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);

    let assert = cli()
        // loc.sloc aggregates source lines at the file level, so a
        // threshold of 1 is guaranteed to fire there for any
        // non-trivial fixture.
        .args(["--paths", &path, "check", "--threshold", "loc.sloc=1"])
        .assert()
        .code(2);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let file_lines: Vec<&str> = stderr
        .lines()
        .filter(|l| l.contains("<file>") && l.contains("loc.sloc"))
        .collect();
    assert_eq!(
        file_lines.len(),
        1,
        "expected exactly one file-level violation line; stderr was:\n{stderr}",
    );
    // The file path appears once as the location prefix; the function
    // slot is the sentinel, not the path.
    let line = file_lines[0];
    assert!(
        line.starts_with(&path),
        "file-level line must start with the path; got {line:?}",
    );
    let path_count = line.matches(path.as_str()).count();
    assert_eq!(
        path_count, 1,
        "file path should appear once (location only), not as the function name; line was {line:?}",
    );
}

#[test]
fn check_sarif_output_to_file_with_violations_exits_two() {
    // Issue #235: `bca check --output-format sarif --output FILE`
    // writes a SARIF 2.1.0 document with one `result` per offender,
    // returns exit 2 when offenders exist, and creates parent
    // directories on demand.
    let dir = TempDir::new().unwrap();
    let fixture = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    // Use a nested path to exercise parent-directory creation.
    let out_path = dir.path().join("nested").join("report.sarif.json");

    cli()
        .args([
            "--paths",
            &fixture,
            "check",
            "--threshold",
            "cyclomatic=1",
            "--output-format",
            "sarif",
            "--output",
            out_path.to_str().unwrap(),
        ])
        .assert()
        .code(2);

    let body = fs::read_to_string(&out_path).expect("sarif file readable");
    let doc: serde_json::Value = serde_json::from_str(&body).expect("sarif is valid JSON");
    assert_eq!(doc["version"], "2.1.0");
    let results = doc["runs"][0]["results"]
        .as_array()
        .expect("runs[0].results is array");
    assert!(
        !results.is_empty(),
        "expected at least one SARIF result for branchy fixture; doc was:\n{body}"
    );
}

#[test]
fn check_no_fail_with_sarif_output_exits_zero() {
    // The `--no-fail` flag should keep exit 0 even when offenders
    // exist; the SARIF document is still emitted with the results
    // populated so reporting pipelines see the data.
    let dir = TempDir::new().unwrap();
    let fixture = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let out_path = dir.path().join("report.sarif.json");

    cli()
        .args([
            "--paths",
            &fixture,
            "check",
            "--threshold",
            "cyclomatic=1",
            "--output-format",
            "sarif",
            "--output",
            out_path.to_str().unwrap(),
            "--no-fail",
        ])
        .assert()
        .success();

    let body = fs::read_to_string(&out_path).expect("sarif file readable");
    let doc: serde_json::Value = serde_json::from_str(&body).expect("sarif is valid JSON");
    let results = doc["runs"][0]["results"]
        .as_array()
        .expect("runs[0].results is array");
    assert!(
        !results.is_empty(),
        "--no-fail should still emit offender records; doc was:\n{body}"
    );
}

#[test]
fn check_clean_run_emits_empty_sarif_document() {
    // Acceptance criterion 3: empty offender output should still be
    // a well-formed document (here SARIF runs[].results = []).
    let dir = TempDir::new().unwrap();
    let fixture = write_fixture(&dir, "trivial.rs", TRIVIAL_RUST);
    let out_path = dir.path().join("report.sarif.json");

    cli()
        .args([
            "--paths",
            &fixture,
            "check",
            "--threshold",
            "cyclomatic=10",
            "--output-format",
            "sarif",
            "--output",
            out_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    let body = fs::read_to_string(&out_path).expect("sarif file readable");
    let doc: serde_json::Value = serde_json::from_str(&body).expect("sarif is valid JSON");
    assert_eq!(doc["version"], "2.1.0");
    let results = doc["runs"][0]["results"]
        .as_array()
        .expect("runs[0].results is array");
    assert!(
        results.is_empty(),
        "clean run should emit empty results array; doc was:\n{body}"
    );
}

#[test]
fn check_clang_warning_output_streams_one_line_per_offender() {
    // Clang warning lines stream to stdout when --output is omitted.
    let dir = TempDir::new().unwrap();
    let fixture = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);

    let output = cli()
        .args([
            "--paths",
            &fixture,
            "check",
            "--threshold",
            "cyclomatic=1",
            "--output-format",
            "clang-warning",
            "--no-fail",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).expect("utf-8 stdout");
    // At least one offender line should follow the clang/GCC warning
    // shape `<path>:<line>:<col>: warning: <metric> ...`. Checking
    // both `warning:` and the metric name guards against a routing
    // regression that emits a different format (e.g. MSVC's
    // `warning :` with a space) when clang-warning was requested.
    assert!(
        stdout
            .lines()
            .any(|l| l.contains("warning:") && l.contains("cyclomatic")),
        "expected at least one clang-warning line mentioning cyclomatic; stdout was:\n{stdout}",
    );
}

#[test]
fn check_walks_nested_function_spaces() {
    let dir = TempDir::new().unwrap();
    let body = r"
pub fn outer() -> i32 {
    fn inner(n: i32) -> i32 {
        if n < 0 { -n } else if n == 0 { 0 } else { n }
    }
    inner(5)
}
";
    let path = write_fixture(&dir, "nested.rs", body);

    let assert = cli()
        .args(["--paths", &path, "check", "--threshold", "cyclomatic=1"])
        .assert()
        .code(2);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    // The inner function is a child FuncSpace of `outer`; if the
    // recursion doesn't descend, we'd miss it entirely.
    assert!(
        stderr.contains("inner"),
        "expected nested function to be reported; stderr was:\n{stderr}",
    );
}

// ─── argv shape contract ─────────────────────────────────────────────
//
// `--exclude` is a global variadic option (`num_args(0..)`) on the
// root command. clap collects every positional that follows it,
// including the subcommand token, until it sees the next flag. Without
// a separating flag between the exclude list and the subcommand, the
// subcommand is silently consumed as another exclude glob.
//
// CI relies on `--num-jobs "$(nproc)"` (or any non-variadic flag)
// being interposed between `--exclude …` and `check`. The two tests
// below pin both directions of that contract:
//
//  * `check_subcommand_swallowed_by_variadic_exclude` is the negative
//    pin — if clap or our argv shape ever changes so that the
//    subcommand IS recognised after a bare variadic, this test fails
//    and we can safely simplify the workflow defence.
//  * `check_runs_with_num_jobs_separator` is the positive pin — the
//    exact argv shape `.github/workflows/pages.yml` uses must keep
//    working.

#[test]
fn check_subcommand_swallowed_by_variadic_exclude() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "trivial.rs", TRIVIAL_RUST);

    // No separator between `--exclude` values and `check`. clap eats
    // `check` as one more glob, then errors because no subcommand was
    // ever provided.
    let assert = cli()
        .args([
            "--paths",
            &path,
            "--exclude",
            "./nothing/**",
            "check",
            "--threshold",
            "cyclomatic=10",
        ])
        .assert()
        // Exit 2 is clap's parser-error code (see clap_builder::error).
        .code(2);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("<COMMAND>"),
        "expected clap to complain that <COMMAND> is missing (because \
         `check` was consumed by --exclude); stderr was:\n{stderr}",
    );
}

#[test]
fn check_runs_with_num_jobs_separator() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "trivial.rs", TRIVIAL_RUST);

    // `--num-jobs` is a non-variadic flag; placing it between the
    // exclude list and the subcommand terminates the variadic and lets
    // clap recognise `check`. This is the exact argv shape the Pages
    // workflow uses.
    cli()
        .args([
            "--paths",
            &path,
            "--exclude",
            "./nothing/**",
            "--num-jobs",
            "1",
            "check",
            "--threshold",
            "cyclomatic=10",
        ])
        .assert()
        .success();
}
