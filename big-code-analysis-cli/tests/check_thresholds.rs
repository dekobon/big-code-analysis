//! Integration tests for the `bca check` threshold engine.
//!
//! These tests drive the binary against tiny inline source fixtures so
//! they don't depend on any submodule. Each test exercises one branch of
//! the exit-code contract: 0 clean / 0 with --no-fail / 2 violations
//! / 1 tool error.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

mod common;

/// Hermetic `bca` builder: anchors the process cwd at `dir` (a
/// `tempfile::tempdir()` with no `.git` ancestor) so `bca check` cannot
/// auto-discover the repo's own `bca.toml` / `.bca-baseline.toml` and
/// filter or scale the inline fixtures against repo state (#491).
fn cli(dir: &Path) -> Command {
    common::cli_in(dir)
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

    cli(dir.path())
        .args(["check", "--paths", &path, "--threshold", "cyclomatic=10"])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

#[test]
fn check_violation_exits_two_with_stable_stderr() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);

    cli(dir.path())
        .args(["check", "--paths", &path, "--threshold", "cyclomatic=1"])
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

/// The violation report is buffered before it reaches stderr (#1115), so
/// it must be drained before anything else writes to either stream —
/// otherwise the very run that has offenders reports them out of order,
/// or (once `run_check` reaches `process::exit`, which runs no
/// destructors) not at all.
///
/// `--summary-file` pointed at a path under a missing directory makes
/// `write_step_summary` fail, which `emit_check_results` reports with a
/// bare `eprintln!` — an unbuffered write straight to fd 2, emitted after
/// the buffered block. Pinning the two in order is what makes the flush
/// observable: delete both the explicit `stderr.flush()` and the
/// `drop(stderr)` that precede the step-summary call and the diagnostic
/// overtakes the offenders, failing here.
#[test]
fn check_violations_are_flushed_before_later_stderr_writes() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let unwritable = dir.path().join("absent").join("summary.md");

    let output = cli(dir.path())
        .args([
            "check",
            "--paths",
            &path,
            "--threshold",
            "cyclomatic=1",
            "--summary-file",
            unwritable.to_str().expect("utf8 path"),
        ])
        .assert()
        .code(2)
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(output).expect("utf8 stderr");

    let offender = stderr
        .find("classify")
        .unwrap_or_else(|| panic!("offender line missing from stderr:\n{stderr}"));
    let diagnostic = stderr
        .find("failed to append step summary")
        .unwrap_or_else(|| panic!("step-summary diagnostic missing from stderr:\n{stderr}"));

    assert!(
        offender < diagnostic,
        "the buffered offender report must be flushed before the unbuffered \
         step-summary diagnostic; got:\n{stderr}"
    );
}

/// The remediation block's "Detailed reports" line reads
/// `GITHUB_REPOSITORY` / `GITHUB_RUN_ID` and points at the CI artifact
/// URL when both are set, falling back to `run bca report …` locally
/// (`commands.rs::artifact_link`). `scrub_ci_env` must `env_remove` both
/// so a helper-built `bca check` does not *inherit* them from the parent
/// (a GitHub Actions runner exports both), always emitting the local
/// fallback regardless of where the suite runs (#900).
///
/// `env_remove` blocks *inheritance*, not an explicit per-command
/// `.env`, so the test exercises the real path: it sets both vars on the
/// parent process for the duration of the test (restored on drop) and
/// then proves a raw child inherits them (CI URL) while a helper-built
/// child does not (local fallback). Without the parent set the scrub
/// would have nothing to remove and the assertion would be vacuous.
#[test]
fn helper_scrubs_artifact_link_env_to_local_fallback() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let args = [
        "check",
        "--paths",
        path.as_str(),
        "--threshold",
        "cyclomatic=1",
    ];

    let _guard = ArtifactEnvGuard::set("octo/repo", "424242");

    // Control: a raw command inherits the parent's artifact-link vars
    // and emits the CI artifact URL. If this branch never fired, the
    // scrub assertion below would be vacuous.
    Command::cargo_bin("bca")
        .unwrap()
        .current_dir(dir.path())
        .args(args)
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "https://github.com/octo/repo/actions/runs/424242",
        ));

    // The helper's `env_remove` blocks inheritance, so the same parent
    // env yields the local fallback, never the URL.
    cli(dir.path())
        .args(args)
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "run `bca report` to see them locally",
        ))
        .stderr(predicate::str::contains("https://github.com/octo/repo").not());
}

/// RAII guard that sets `GITHUB_REPOSITORY` / `GITHUB_RUN_ID` on the
/// parent process and restores their prior values on drop. `set_var` /
/// `remove_var` are `unsafe` in Rust 2024 (concurrent-mutation footgun).
///
/// The lock is the binary-wide [`common::process_env_lock`], not a
/// guard-private one: child-spawn snapshots the *whole* environment, so
/// this guard must serialize against every other env-mutating-and-
/// spawning test in the binary — notably
/// `cli_helper_does_not_leak_to_github_step_summary`, which mutates a
/// *different* variable. A private lock let the two run concurrently and
/// the step-summary child captured a torn env carrying this guard's CI
/// artifact vars, failing that test intermittently.
struct ArtifactEnvGuard {
    repo: Option<String>,
    run_id: Option<String>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl ArtifactEnvGuard {
    fn set(repo: &str, run_id: &str) -> Self {
        let lock = common::process_env_lock();
        let prior = Self {
            repo: std::env::var("GITHUB_REPOSITORY").ok(),
            run_id: std::env::var("GITHUB_RUN_ID").ok(),
            _lock: lock,
        };
        // SAFETY: serialized by ENV_LOCK; restored on drop.
        unsafe {
            std::env::set_var("GITHUB_REPOSITORY", repo);
            std::env::set_var("GITHUB_RUN_ID", run_id);
        }
        prior
    }
}

impl Drop for ArtifactEnvGuard {
    fn drop(&mut self) {
        // SAFETY: still holding ENV_LOCK via `_lock`.
        unsafe {
            match &self.repo {
                Some(v) => std::env::set_var("GITHUB_REPOSITORY", v),
                None => std::env::remove_var("GITHUB_REPOSITORY"),
            }
            match &self.run_id {
                Some(v) => std::env::set_var("GITHUB_RUN_ID", v),
                None => std::env::remove_var("GITHUB_RUN_ID"),
            }
        }
    }
}

#[test]
fn check_no_fail_keeps_exit_zero_but_still_reports() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &path,
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

    cli(dir.path())
        .args(["check", "--paths", &path, "--threshold", "not_a_metric=1"])
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

    // `cli()` already anchors the cwd at a `.git`-free tempdir, so
    // discovery never reaches the repo's root `bca.toml` (whose
    // `[thresholds]` table would otherwise satisfy the "at least one
    // threshold" check). `--no-config` is an explicit belt-and-suspenders
    // guard on top of that cwd anchor.
    cli(dir.path())
        .args(["check", "--no-config", "--paths", &path])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no thresholds configured"));
}

/// A 1-edit typo on `--threshold` must surface a "did you mean ..."
/// hint pointing at the canonical name. Regression for #381.
#[test]
fn check_unknown_metric_close_typo_suggests_correction() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "trivial.rs", TRIVIAL_RUST);

    cli(dir.path())
        .args(["check", "--paths", &path, "--threshold", "cyclic=15"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("did you mean"))
        .stderr(predicate::str::contains("cyclomatic"));
}

/// A typo inside a dotted/compound metric name must still resolve
/// to the right neighbour. Verifies the suggester treats the whole
/// name as one string rather than splitting on `.`.
#[test]
fn check_unknown_metric_dotted_typo_suggests_correction() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "trivial.rs", TRIVIAL_RUST);

    cli(dir.path())
        .args(["check", "--paths", &path, "--threshold", "halstead.efort=1"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("did you mean"))
        .stderr(predicate::str::contains("halstead.effort"));
}

/// Garbage input with no close match must keep the original
/// "unknown metric" error and not invent a suggestion. Without this,
/// short or unrelated inputs would point users at unrelated metrics.
#[test]
fn check_unknown_metric_unrelated_input_omits_suggestion() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "trivial.rs", TRIVIAL_RUST);

    cli(dir.path())
        .args(["check", "--paths", &path, "--threshold", "xyznonexistent=1"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("unknown threshold metric"))
        .stderr(predicate::str::contains("did you mean").not());
}

/// `[thresholds]` TOML config keys flow through the same validator,
/// so a typo there must produce the same suggestion as a CLI typo.
#[test]
fn check_unknown_metric_in_toml_config_suggests_correction() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "trivial.rs", TRIVIAL_RUST);
    let config_path = dir.path().join("bca.toml");
    fs::write(&config_path, "[thresholds]\ncyclomatc = 15\n").expect("write config");
    let config_str = config_path.to_str().expect("utf8 config path");

    cli(dir.path())
        .args(["check", "--paths", &path, "--config", config_str])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("did you mean"))
        .stderr(predicate::str::contains("cyclomatic"));
}

#[test]
fn check_with_no_matching_files_exits_one() {
    // A directory that exists but contains no source files should produce
    // a tool error (exit 1), not a vacuous clean pass (exit 0). Otherwise
    // a typo in `--paths` silently green-lights CI.
    let dir = TempDir::new().unwrap();

    cli(dir.path())
        .args([
            "check",
            "--paths",
            dir.path().to_str().unwrap(),
            "--threshold",
            "cyclomatic=10",
        ])
        .assert()
        .code(1)
        // #609: fatal tool errors carry the unified bare lowercase
        // `error:` prefix (matching clap's own usage errors), never the
        // old capitalized `Error:`.
        .stderr(predicate::str::contains("error: no input files matched"))
        .stderr(predicate::str::contains("Error:").not());
}

#[test]
fn check_reads_thresholds_from_toml_config() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let cfg_path = dir.path().join("thresholds.toml");
    fs::write(&cfg_path, "[thresholds]\ncyclomatic = 1\n").unwrap();

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &path,
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &path,
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
    let assert = cli(dir.path())
        .args([
            "check",
            "--paths",
            &path,
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

    // `cli()` already anchors the cwd at a `.git`-free tempdir, so
    // discovery never reaches the repo's root `bca.toml` (whose
    // `baseline` key would otherwise prefix the violation line with a
    // `[new]` tag and break the `starts_with(path)` assertion below).
    // `--no-config` is an explicit belt-and-suspenders guard on top of
    // that cwd anchor.
    let assert = cli(dir.path())
        // loc.sloc aggregates source lines at the file level, so a
        // threshold of 1 is guaranteed to fire there for any
        // non-trivial fixture.
        .args([
            "check",
            "--no-config",
            "--paths",
            &path,
            "--threshold",
            "loc.sloc=1",
        ])
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &fixture,
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &fixture,
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &fixture,
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
fn check_output_sarif_extension_infers_format_without_flag() {
    // Issue #600: `--output report.sarif` with no `--format` must infer
    // SARIF from the extension and write a real document, not silently
    // do nothing on exit 0 (the pre-2.0 bug).
    let dir = TempDir::new().unwrap();
    let fixture = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let out_path = dir.path().join("report.sarif");

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &fixture,
            "--threshold",
            "cyclomatic=1",
            "--output",
            out_path.to_str().unwrap(),
            "--no-fail",
        ])
        .assert()
        .success();

    let body = fs::read_to_string(&out_path).expect("inferred sarif file readable");
    let doc: serde_json::Value = serde_json::from_str(&body).expect("sarif is valid JSON");
    assert_eq!(doc["version"], "2.1.0", "inferred format should be SARIF");
    let results = doc["runs"][0]["results"]
        .as_array()
        .expect("runs[0].results is array");
    assert!(
        !results.is_empty(),
        "inferred SARIF should carry the offenders; doc was:\n{body}"
    );
}

#[test]
fn check_output_xml_extension_infers_checkstyle_without_flag() {
    // Issue #600: `.xml` infers Checkstyle, the only XML writer.
    let dir = TempDir::new().unwrap();
    let fixture = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let out_path = dir.path().join("report.xml");

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &fixture,
            "--threshold",
            "cyclomatic=1",
            "--output",
            out_path.to_str().unwrap(),
            "--no-fail",
        ])
        .assert()
        .success();

    let body = fs::read_to_string(&out_path).expect("inferred checkstyle file readable");
    assert!(
        body.contains("<checkstyle"),
        "inferred format should be Checkstyle XML; body was:\n{body}"
    );
}

#[test]
fn check_output_unknown_extension_without_format_exits_one() {
    // Issue #600: an extension with no unique writer (`.json` is shared
    // by SARIF and Code Climate) must be a usage error naming --format,
    // never a silent no-op.
    let dir = TempDir::new().unwrap();
    let fixture = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let out_path = dir.path().join("report.json");

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &fixture,
            "--threshold",
            "cyclomatic=1",
            "--output",
            out_path.to_str().unwrap(),
            "--no-fail",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--format"));
    assert!(
        !out_path.exists(),
        "no document should be written when the format cannot be inferred"
    );
}

#[test]
fn check_output_no_extension_without_format_exits_one() {
    // Issue #600: an extensionless `--output` path cannot be inferred
    // and must error rather than silently write nothing.
    let dir = TempDir::new().unwrap();
    let fixture = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let out_path = dir.path().join("report");

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &fixture,
            "--threshold",
            "cyclomatic=1",
            "--output",
            out_path.to_str().unwrap(),
            "--no-fail",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--format"));
    assert!(!out_path.exists(), "no document on un-inferable extension");
}

#[test]
fn check_explicit_format_overrides_output_extension() {
    // Issue #600: an explicit `--format` always wins; the extension is
    // only consulted when `--format` is absent. Here `.xml` would infer
    // Checkstyle, but `--format sarif` must produce a SARIF document.
    let dir = TempDir::new().unwrap();
    let fixture = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let out_path = dir.path().join("report.xml");

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &fixture,
            "--threshold",
            "cyclomatic=1",
            "--format",
            "sarif",
            "--output",
            out_path.to_str().unwrap(),
            "--no-fail",
        ])
        .assert()
        .success();

    let body = fs::read_to_string(&out_path).expect("output file readable");
    let doc: serde_json::Value =
        serde_json::from_str(&body).expect("explicit --format wins → SARIF");
    assert_eq!(doc["version"], "2.1.0");
}

#[test]
fn check_clang_warning_output_streams_one_line_per_offender() {
    // Clang warning lines stream to stdout when --output is omitted.
    let dir = TempDir::new().unwrap();
    let fixture = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);

    let output = cli(dir.path())
        .args([
            "check",
            "--paths",
            &fixture,
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

    let assert = cli(dir.path())
        .args(["check", "--paths", &path, "--threshold", "cyclomatic=1"])
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
// As of #601, `--exclude` (and `--include`) take exactly one value per
// occurrence (`num_args(1)`, repeatable via `ArgAction::Append`). A
// positional that follows — including a subcommand token — is no longer
// swallowed as an extra glob, so no separator flag is required between
// the exclude list and the subcommand.
//
// The two tests below pin both directions of that contract:
//
//  * `check_recognised_after_single_value_exclude` is the regression
//    pin for #601 — `--exclude GLOB check` must recognise `check` as
//    the subcommand without an interposed flag. Before #601 the
//    variadic `num_args(0..)` consumed `check` as another glob and clap
//    errored on a missing <COMMAND>.
//  * `check_runs_with_num_jobs_separator` is the historical positive
//    pin — the argv shape `.github/workflows/pages.yml` uses (with the
//    `--num-jobs` separator) must keep working; the separator is now
//    redundant but harmless.

#[test]
fn check_recognised_after_single_value_exclude() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "trivial.rs", TRIVIAL_RUST);

    // No separator between the single `--exclude` value and `check`.
    // Post-#601 `--exclude` binds exactly one glob, so `check` is parsed
    // as the subcommand. Pre-#601 this errored with a missing <COMMAND>.
    cli(dir.path())
        .args([
            "check",
            "--paths",
            &path,
            "--exclude",
            "./nothing/**",
            "--threshold",
            "cyclomatic=10",
        ])
        .assert()
        .success();
}

#[test]
fn check_runs_with_num_jobs_separator() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "trivial.rs", TRIVIAL_RUST);

    // The historical argv shape with `--num-jobs` interposed between the
    // exclude value and the subcommand. The separator is redundant
    // post-#601 but must remain harmless — this is the exact shape the
    // Pages workflow uses.
    cli(dir.path())
        .args([
            "check",
            "--paths",
            &path,
            "--exclude",
            "./nothing/**",
            "--num-jobs",
            "1",
            "--threshold",
            "cyclomatic=10",
        ])
        .assert()
        .success();
}

// -- Per-file summary footer (issue #356 sub-deliverable A) -------------

/// Function with cyclomatic == 2 (one branch). Lower-ratio violation
/// when threshold = 1: ratio = 2.
const ONE_BRANCH_RUST: &str = r#"
pub fn pick(n: i32) -> &'static str {
    if n > 0 { "pos" } else { "non-pos" }
}
"#;

#[test]
fn summary_footer_emitted_by_default() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);

    cli(dir.path())
        .args(["check", "--paths", &path, "--threshold", "cyclomatic=1"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--- summary ---"))
        .stderr(predicate::str::contains("1 violation (worst:"));
}

#[test]
fn summary_footer_suppressed_by_no_summary() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &path,
            "--threshold",
            "cyclomatic=1",
            "--no-summary",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--- summary ---").not());
}

#[test]
fn summary_skipped_for_clean_run() {
    // No violations → no footer. Clean stderr stays empty so CI
    // tooling that asserts on "no output ⇒ clean" keeps working.
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "trivial.rs", TRIVIAL_RUST);

    cli(dir.path())
        .args(["check", "--paths", &path, "--threshold", "cyclomatic=10"])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

#[test]
fn cli_helper_does_not_leak_to_github_step_summary() {
    const ENV_KEY: &str = "GITHUB_STEP_SUMMARY";

    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let summary = dir.path().join("step-summary.md");
    fs::write(&summary, "").unwrap();

    // Pre-set GITHUB_STEP_SUMMARY on the parent's env so a freshly
    // built Command inherits it — this mimics the GHA runner case
    // exactly (the runner exports the var before invoking
    // `cargo test`). `assert_cmd::Command::cargo_bin` snapshots
    // the parent env at spawn time, so any tests in this binary
    // that spawn `bca` while the var is set would also leak unless
    // `cli()` removes it — which is the helper's contract.
    //
    // SAFETY (test-only): Rust 2024 marks `env::set_var` /
    // `env::remove_var` as `unsafe` because of concurrent-thread
    // hazards. The binary-wide `common::process_env_lock` held for the
    // whole set/spawn/restore window serializes this test against every
    // other env-mutating-and-spawning test in the binary (e.g.
    // `helper_scrubs_artifact_link_env_to_local_fallback`, which mutates
    // `GITHUB_REPOSITORY`/`GITHUB_RUN_ID`). Without it, that test's
    // concurrent `set_var` tore the environment this test's child
    // snapshots at spawn, leaking the CI artifact vars into the step
    // summary and failing the assertion intermittently. The remaining
    // concurrent reader is the child `bca` process — which we explicitly
    // want to either see the var (control) or not (regression check).
    let _env_lock = common::process_env_lock();
    let prior = std::env::var_os(ENV_KEY);
    unsafe { std::env::set_var(ENV_KEY, &summary) };

    let result = std::panic::catch_unwind(|| {
        // Regression: `cli()` strips the inherited env var, so
        // the child never sees a step-summary target.
        cli(dir.path())
            .args(["check", "--paths", &path, "--threshold", "cyclomatic=1"])
            .assert()
            .code(2);

        let leaked = fs::read_to_string(&summary).unwrap();
        assert!(
            leaked.is_empty(),
            "cli() leaked to GITHUB_STEP_SUMMARY: {leaked:?}"
        );
    });

    // Always restore the parent's env before propagating any
    // panic, so a failed assertion doesn't pollute later tests in
    // the same binary.
    unsafe {
        match &prior {
            Some(v) => std::env::set_var(ENV_KEY, v),
            None => std::env::remove_var(ENV_KEY),
        }
    }
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
}

#[test]
fn summary_pluralizes_count() {
    // Three files with one violation each → each row reads
    // "1 violation" (singular).
    let dir = TempDir::new().unwrap();
    let p1 = write_fixture(&dir, "a.rs", BRANCHY_RUST);
    let p2 = write_fixture(&dir, "b.rs", BRANCHY_RUST);
    let p3 = write_fixture(&dir, "c.rs", BRANCHY_RUST);

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &p1,
            "--paths",
            &p2,
            "--paths",
            &p3,
            "--threshold",
            "cyclomatic=1",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("1 violation (worst:"))
        // Anchor with the leading `": "` from the format
        // `<path>: <N> violations` so a future fixture emitting
        // "11 violations" cannot false-match the substring "1 violations".
        .stderr(predicate::str::contains(": 1 violations").not());
}

#[test]
fn summary_worst_metric_uses_max_ratio() {
    // One file violating two thresholds:
    //   cyclomatic = 5 vs limit 4    → ratio 1.25
    //   cyclomatic = 5 vs limit 1    → ratio 5     (worst)
    // We can't impose two limits on the same metric, so use two
    // distinct metrics: cyclomatic (ratio 5/1=5) vs nargs (ratio
    // unsatisfied because the function has 1 arg). Use loc.sloc
    // instead — BRANCHY_RUST is ~12 lines so loc.sloc=12 vs limit 100
    // wouldn't trigger. Pick limits that cross both:
    //   cyclomatic = 5 vs 1  → ratio 5     (worst, max ratio)
    //   loc.sloc   = ~12 vs 11 → ratio ~1.09
    // The footer must cite cyclomatic as worst.
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &path,
            "--threshold",
            "cyclomatic=1",
            "--threshold",
            "loc.sloc=11",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("(worst: cyclomatic ="));
}

#[test]
fn summary_sorts_by_count_desc() {
    // File `a.rs` violates two thresholds (count=2); file `b.rs`
    // violates one (count=1). The footer must list a.rs before b.rs.
    let dir = TempDir::new().unwrap();
    let a = write_fixture(&dir, "a.rs", BRANCHY_RUST); // 1 fn, 2 metrics violated
    let b = write_fixture(&dir, "b.rs", ONE_BRANCH_RUST); // 1 fn, 1 metric violated

    let output = cli(dir.path())
        .args([
            "check",
            "--paths",
            &a,
            "--paths",
            &b,
            "--threshold",
            "cyclomatic=1",
            "--threshold",
            "loc.sloc=8",
        ])
        .assert()
        .code(2)
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(output).unwrap();
    let summary_start = stderr
        .find("--- summary ---")
        .expect("summary banner present");
    let summary = &stderr[summary_start..];
    let a_pos = summary.find(&a).expect("a.rs in summary");
    let b_pos = summary.find(&b).expect("b.rs in summary");
    assert!(
        a_pos < b_pos,
        "a.rs (2 violations) must precede b.rs (1 violation); got:\n{summary}",
    );
}

/// `--print-effective-config` (default TOML) must emit the resolved
/// `[thresholds]` table with `--threshold` CLI overrides applied,
/// then exit 0 without walking any files. Validates the
/// debuggability contract for issue #380.
#[test]
fn check_print_effective_config_toml_emits_resolved_thresholds_and_exits_zero() {
    let dir = TempDir::new().unwrap();
    cli(dir.path())
        .args([
            "check",
            "--threshold",
            "cyclomatic=22",
            "--print-effective-config",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("[thresholds]"))
        .stdout(predicate::str::contains("cyclomatic = 22"))
        .stdout(predicate::str::contains("[check]"));
}

/// `--print-effective-config=json` must emit valid JSON with the same
/// shape as the TOML form (thresholds + check tables).
#[test]
fn check_print_effective_config_json_emits_valid_json() {
    let dir = TempDir::new().unwrap();
    let assert = cli(dir.path())
        .args([
            "check",
            "--threshold",
            "cyclomatic=22",
            "--threshold",
            "loc.sloc=300",
            "--print-effective-config=json",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("stdout is UTF-8");
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout must be valid JSON");
    assert_eq!(
        parsed["thresholds"]["cyclomatic"], 22.0,
        "JSON output must contain resolved cyclomatic threshold"
    );
    assert_eq!(
        parsed["thresholds"]["loc.sloc"], 300.0,
        "JSON output must contain resolved loc.sloc threshold"
    );
    assert!(
        parsed["check"].is_object(),
        "JSON must contain check table: {stdout}"
    );
}

/// CLI must reject the combination `--print-effective-config` +
/// `--write-baseline` at clap parse time. The flag is a read-only
/// debug aid; pairing it with a side-effecting operation would be a
/// silent footgun.
#[test]
fn check_print_effective_config_conflicts_with_write_baseline() {
    let dir = TempDir::new().unwrap();
    let baseline_path = dir.path().join("baseline.toml");

    cli(dir.path())
        .args([
            "check",
            "--threshold",
            "cyclomatic=22",
            "--print-effective-config",
            "--write-baseline",
            baseline_path.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

/// Round-trip: feeding the TOML output of `--print-effective-config`
/// back through `--config` must produce the same resolved thresholds.
/// This is the core debuggability promise — operators can capture
/// the effective view, tweak it, and feed it back without manual
/// translation.
#[test]
fn check_print_effective_config_toml_roundtrips_through_config() {
    let dir = TempDir::new().unwrap();
    // First pass: capture the effective config to a file.
    let assert = cli(dir.path())
        .args([
            "check",
            "--threshold",
            "cyclomatic=22",
            "--threshold",
            "loc.sloc=300",
            "--print-effective-config",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("stdout is UTF-8");
    let cfg_path = dir.path().join("captured.toml");
    fs::write(&cfg_path, &stdout).unwrap();

    // Second pass: feed it back via --config and re-print. The
    // re-emitted [thresholds] table must match the original.
    let assert2 = cli(dir.path())
        .args([
            "check",
            "--config",
            cfg_path.to_str().unwrap(),
            "--print-effective-config",
        ])
        .assert()
        .success();
    let stdout2 = String::from_utf8(assert2.get_output().stdout.clone()).expect("stdout is UTF-8");
    assert!(
        stdout2.contains("cyclomatic = 22"),
        "roundtripped TOML must keep cyclomatic: {stdout2}"
    );
    assert!(
        stdout2.contains("\"loc.sloc\" = 300"),
        "roundtripped TOML must keep loc.sloc: {stdout2}"
    );
}

// ─── --tier=soft scaling via --headroom (#373/#375) ───────────────────
//
// `classify` in BRANCHY_RUST has cyclomatic == 5. These tests pin the
// documented resolution order (config → tier resolution → --threshold
// overrides absolute) and the exit-code contract. `--headroom` is a
// soft-tier dial: it only takes effect under `--tier=soft`. Config
// fixtures reuse `write_fixture` with a `thresholds.toml` name.

/// A config limit that is clean at full scale must become an offender
/// once `--tier=soft --headroom` shrinks it below the function's value.
/// With `cyclomatic = 100` scaled by `0.01` the limit is `1.0`, so
/// `classify` (cyclomatic 5) trips the gate.
#[test]
fn check_headroom_scales_config_limit_into_offender() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let cfg = write_fixture(&dir, "thresholds.toml", "[thresholds]\ncyclomatic = 100\n");

    // Sanity: clean at full scale (hard tier).
    cli(dir.path())
        .args(["check", "--paths", &path, "--config", &cfg])
        .assert()
        .success();

    // Scaled to 1.0 at the soft tier → offender.
    cli(dir.path())
        .args([
            "check",
            "--paths",
            &path,
            "--config",
            &cfg,
            "--tier=soft=0.01",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("classify"))
        .stderr(predicate::str::contains("cyclomatic"))
        .stderr(predicate::str::contains("(limit 1)"));
}

/// The deprecated `--headroom <R>` alias now promotes the gate to
/// `--tier=soft=<R>` (issue #688): headroom IS the soft tier's ratio,
/// not a separate hard-tier dial. A value that trips the soft gate now
/// trips, and a one-cycle deprecation warning points at the new form.
#[test]
fn check_headroom_alias_promotes_to_soft_tier() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let cfg = write_fixture(&dir, "thresholds.toml", "[thresholds]\ncyclomatic = 100\n");

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &path,
            "--config",
            &cfg,
            "--headroom",
            "0.01",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "`--headroom <R>` is deprecated; use `--tier=soft=<R>`",
        ))
        .stderr(predicate::str::contains("cyclomatic"))
        .stderr(predicate::str::contains("(limit 1)"));
}

/// `--tier=soft --headroom 1.0` is the documented no-op. The limit is
/// pinned at the strict boundary (`cyclomatic = 5`, exactly `classify`'s
/// value, and the offender test is `value > limit`), so any erroneous
/// downward scaling at ratio `1.0` would flip the run to an offender
/// (exit 2) and fail this test — a looser limit like `100` would mask
/// that.
#[test]
fn check_headroom_one_is_noop() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let cfg = write_fixture(&dir, "thresholds.toml", "[thresholds]\ncyclomatic = 5\n");

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &path,
            "--config",
            &cfg,
            "--tier=soft=1.0",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

/// Out-of-range ratios are a usage error regardless of tier: exit 1
/// (tool error, not the exit-2 threshold-exceeded code) with a clear
/// stderr message. Covers both bounds of the half-open `(0, 1]`
/// interval.
#[test]
fn check_headroom_out_of_range_exits_one() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let cfg = write_fixture(&dir, "thresholds.toml", "[thresholds]\ncyclomatic = 100\n");

    // `--headroom=<v>` (joined form) so clap forwards a leading-`-`
    // value to our validator instead of treating it as a flag.
    for ratio in ["--headroom=1.5", "--headroom=0", "--headroom=-0.5"] {
        cli(dir.path())
            .args(["check", "--paths", &path, "--config", &cfg, ratio])
            .assert()
            .code(1)
            .stderr(predicate::str::contains("--headroom must be in (0, 1]"));
    }
}

/// `--threshold` overrides are absolute: they land *after* scaling and
/// are not themselves scaled. Config `cyclomatic = 2` would scale to
/// `1.0` (offender), but the explicit `--threshold cyclomatic=8`
/// replaces it with an un-scaled `8` (clean for `classify`'s 5). If the
/// override were scaled to `4`, `classify` would trip — so a clean exit
/// proves the override stayed absolute.
#[test]
fn check_headroom_does_not_scale_cli_threshold_override() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let cfg = write_fixture(&dir, "thresholds.toml", "[thresholds]\ncyclomatic = 2\n");

    // `cli(dir.path())` runs from the temp dir so no auto-discovered
    // repo `bca.toml` baseline leaks in: at `--headroom 0.5` this run is
    // stricter than the repo's soft-0.95 baseline and would otherwise
    // draw the (here irrelevant) provenance warning (#486).
    cli(dir.path())
        .args([
            "check",
            "--paths",
            &path,
            "--config",
            &cfg,
            "--tier=soft=0.5",
            "--threshold",
            "cyclomatic=8",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

/// `--tier=soft` with no config to scale is a no-op (because
/// `--threshold` limits are absolute), so it emits a one-line note
/// rather than silently appearing to take effect.
#[test]
fn check_soft_tier_without_config_warns_and_noops() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);

    // `cli()` already anchors the cwd at a `.git`-free tempdir, so
    // discovery never reaches the repo's root `bca.toml` (whose
    // `[thresholds]` table would otherwise give the soft tier something
    // to scale, suppressing the "no effect" note). `--no-config` is an
    // explicit belt-and-suspenders guard on top of that cwd anchor.
    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            &path,
            "--tier=soft=0.5",
            "--threshold",
            "cyclomatic=100",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "--tier=soft has no effect without configured thresholds",
        ));
}

/// `--tier=soft --headroom` stacks with `--write-baseline`: the baseline
/// captures offenders at the *scaled* limits. A subsequent `--baseline`
/// run at the same tier then suppresses them.
#[test]
fn check_headroom_write_baseline_captures_scaled_offenders() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let cfg = write_fixture(&dir, "thresholds.toml", "[thresholds]\ncyclomatic = 100\n");
    let baseline = dir.path().join("baseline.toml");
    let baseline_str = baseline.to_str().unwrap();

    // Write a baseline at the scaled (1.0) limit: classify is captured.
    cli(dir.path())
        .args([
            "check",
            "--paths",
            &path,
            "--config",
            &cfg,
            "--tier=soft=0.01",
            "--write-baseline",
            baseline_str,
        ])
        .assert()
        .success();
    let body = fs::read_to_string(&baseline).expect("baseline readable");
    assert!(
        body.contains("classify") && body.contains("cyclomatic"),
        "baseline must capture the scaled-tier offender; was:\n{body}"
    );

    // Re-run filtered by that baseline at the same tier: suppressed.
    cli(dir.path())
        .args([
            "check",
            "--paths",
            &path,
            "--config",
            &cfg,
            "--tier=soft=0.01",
            "--baseline",
            baseline_str,
        ])
        .assert()
        .success();
}

/// `--print-effective-config` must show the post-scaling `[thresholds]`
/// values, record the applied `--headroom` ratio, and report the
/// resolved tier for provenance.
#[test]
fn check_headroom_print_effective_config_shows_scaled_values_and_ratio() {
    let dir = TempDir::new().unwrap();
    let cfg = write_fixture(&dir, "thresholds.toml", "[thresholds]\ncyclomatic = 100\n");

    cli(dir.path())
        .args([
            "check",
            "--config",
            &cfg,
            "--tier=soft=0.5",
            "--print-effective-config",
        ])
        .assert()
        .success()
        // 100 * 0.5 = 50 (rendered as a float by the TOML serializer).
        .stdout(predicate::str::contains("cyclomatic = 50.0"))
        .stdout(predicate::str::contains("headroom = 0.5"))
        .stdout(predicate::str::contains("tier = \"soft\""));
}

// ─── [thresholds.soft] per-metric soft tier (#375) ────────────────────
//
// `classify` in BRANCHY_RUST has cyclomatic == 5. A `[thresholds.soft]`
// table overrides specific keys at the soft tier; unspecified keys
// inherit the hard limit (no soft band).

/// `--tier=soft` honors an absolute `[thresholds.soft]` override: a hard
/// limit clean for `classify` becomes an offender at the tighter soft
/// limit. The default hard tier still passes the same config.
#[test]
fn check_soft_table_absolute_override_trips_at_soft_tier() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let cfg = write_fixture(
        &dir,
        "thresholds.toml",
        "[thresholds]\ncyclomatic = 10\n[thresholds.soft]\ncyclomatic = 3\n",
    );

    // Hard tier: limit 10, classify (5) is clean; the soft table is ignored.
    cli(dir.path())
        .args(["check", "--paths", &path, "--config", &cfg])
        .assert()
        .success();

    // Soft tier: limit 3, classify (5) trips.
    cli(dir.path())
        .args(["check", "--paths", &path, "--config", &cfg, "--tier=soft"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cyclomatic = 5 (limit 3)"));
}

/// `"NNx"` scale syntax resolves against the metric's hard limit:
/// `cyclomatic = 10` with `"0.4x"` yields a soft limit of `4`, which
/// `classify` (5) exceeds.
#[test]
fn check_soft_table_scale_relative_resolves_against_hard() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let cfg = write_fixture(
        &dir,
        "thresholds.toml",
        "[thresholds]\ncyclomatic = 10\n[thresholds.soft]\ncyclomatic = \"0.4x\"\n",
    );

    cli(dir.path())
        .args(["check", "--paths", &path, "--config", &cfg, "--tier=soft"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cyclomatic = 5 (limit 4)"));
}

/// A metric absent from `[thresholds.soft]` inherits its hard limit at
/// the soft tier (no soft band). `nargs` here has no soft override, so a
/// soft run gates `nargs` at the hard `7` while tightening `cyclomatic`.
#[test]
fn check_soft_table_unspecified_metric_inherits_hard_limit() {
    let dir = TempDir::new().unwrap();
    let cfg = write_fixture(
        &dir,
        "thresholds.toml",
        "[thresholds]\ncyclomatic = 100\nnargs = 7\n[thresholds.soft]\ncyclomatic = 3\n",
    );

    cli(dir.path())
        .args([
            "check",
            "--config",
            &cfg,
            "--tier=soft",
            "--print-effective-config",
        ])
        .assert()
        .success()
        // cyclomatic dropped to the soft override; nargs inherits hard 7.
        // Anchor the `.0` so a buggy `30.0` / `70.0` can't substring-match.
        .stdout(predicate::str::contains("cyclomatic = 3.0"))
        .stdout(predicate::str::contains("nargs = 7.0"));
}

/// When a `[thresholds.soft]` table is present, the blanket
/// `--tier=soft=<R>` ratio does not apply — per-metric intent wins over
/// the scalar (issue #688: the soft table takes precedence silently).
#[test]
fn check_soft_table_overrides_blanket_ratio() {
    let dir = TempDir::new().unwrap();
    let cfg = write_fixture(
        &dir,
        "thresholds.toml",
        "[thresholds]\ncyclomatic = 100\n[thresholds.soft]\ncyclomatic = 50\n",
    );

    cli(dir.path())
        .args([
            "check",
            "--config",
            &cfg,
            "--tier=soft=0.5",
            "--print-effective-config",
        ])
        .assert()
        .success()
        // 50.0 (soft override), NOT 25.0 (50 * 0.5 ratio). Anchor the
        // `.0` so a buggy `500.0` can't substring-match.
        .stdout(predicate::str::contains("cyclomatic = 50.0"));
}

/// A `"NNx"` soft override with no hard limit to scale is a config error
/// (exit 1) — a scale factor relative to nothing is meaningless.
#[test]
fn check_soft_table_scale_without_hard_base_errors() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let cfg = write_fixture(
        &dir,
        "thresholds.toml",
        "[thresholds]\nnargs = 7\n[thresholds.soft]\ncyclomatic = \"0.9x\"\n",
    );

    // `cli()` already anchors the cwd at a `.git`-free tempdir, so
    // discovery never reaches the repo's root `bca.toml` (whose hard
    // `cyclomatic` limit would otherwise merge in and give the `"0.9x"`
    // soft scale a base to multiply, masking the intended error).
    // `--no-config` is an explicit belt-and-suspenders guard on top of
    // that cwd anchor.
    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            &path,
            "--config",
            &cfg,
            "--tier=soft",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no hard"))
        .stderr(predicate::str::contains("cyclomatic"));
}

// ─── #683: tri-state CI flags ─────────────────────────────────────────

/// `--github-annotations=never` suppresses the `::error` annotations
/// even when `$GITHUB_ACTIONS == "true"` — the opt-out that the bare
/// on-only flag could not express (#683).
#[test]
fn github_annotations_never_suppresses_under_gha() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);

    cli(dir.path())
        .env("GITHUB_ACTIONS", "true")
        .args([
            "check",
            "--paths",
            &path,
            "--threshold",
            "cyclomatic=1",
            "--github-annotations=never",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("::error file=").not());
}

/// `--github-annotations=always` forces the annotations on even outside
/// a GHA step (no `$GITHUB_ACTIONS`).
#[test]
fn github_annotations_always_forces_on_outside_gha() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &path,
            "--threshold",
            "cyclomatic=1",
            "--github-annotations=always",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("::error file="));
}

/// `--summary-file never` skips the step-summary append even when
/// `$GITHUB_STEP_SUMMARY` is set (#683).
#[test]
fn summary_file_never_skips_append_under_gha() {
    const ENV_KEY: &str = "GITHUB_STEP_SUMMARY";
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let summary = dir.path().join("step-summary.md");
    fs::write(&summary, "").unwrap();

    cli(dir.path())
        .env(ENV_KEY, &summary)
        .args([
            "check",
            "--paths",
            &path,
            "--threshold",
            "cyclomatic=1",
            "--summary-file",
            "never",
        ])
        .assert()
        .code(2);

    let body = fs::read_to_string(&summary).unwrap();
    assert!(
        body.is_empty(),
        "--summary-file never must not append: {body:?}"
    );
}

// ─── #666/#688: deprecated-alias warn-but-honor ───────────────────────

/// `--strict-exit-codes` still works for one cycle but warns, pointing
/// at the canonical `--exit-codes=tiered` (#666).
#[test]
fn strict_exit_codes_alias_warns_but_honors() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);

    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            &path,
            "--threshold",
            "cyclomatic=1",
            "--strict-exit-codes",
        ])
        .assert()
        // New offenders, no baseline → tiered exit 2 (same as default
        // here, but the alias is honored, not rejected).
        .code(2)
        .stderr(predicate::str::contains(
            "`--strict-exit-codes` is deprecated; use `--exit-codes=tiered`",
        ));
}

/// `--no-cyclomatic-try` still works for one cycle but warns, pointing
/// at the canonical `--cyclomatic-count-try=false` (#666).
#[test]
fn no_cyclomatic_try_alias_warns_but_honors() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);

    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            &path,
            "--threshold",
            "cyclomatic=100",
            "--no-cyclomatic-try",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "`--no-cyclomatic-try` is deprecated; use `--cyclomatic-count-try=false`",
        ));
}

/// Write `body` to `name` and strip every permission bit so reading it
/// fails with `EACCES`. Returns `None` when the caller is privileged
/// enough to read it anyway (root ignores mode bits), because then the
/// scenario under test cannot be staged at all.
#[cfg(unix)]
fn unreadable_fixture(dir: &TempDir, name: &str, body: &str) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.path().join(name);
    fs::write(&path, body).expect("write fixture");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).expect("chmod 000");
    // Probe the actual capability rather than the uid: `fs::read`
    // succeeding here means mode bits do not deny this process.
    if fs::read(&path).is_ok() {
        return None;
    }
    Some(path.to_str().expect("utf8 fixture path").to_string())
}

/// #1060: every input file failing to read must exit 1, not 0. The
/// per-file read error already reached stderr, but the counter that
/// backs the "no input files matched" guard was bumped before the read
/// was attempted, so the gate reported success on a run that analysed
/// nothing.
#[cfg(unix)]
#[test]
fn check_exits_one_when_every_input_file_is_unreadable() {
    let dir = TempDir::new().unwrap();
    let Some(path) = unreadable_fixture(&dir, "branchy.rs", BRANCHY_RUST) else {
        eprintln!("skipping: this process can read a mode-000 file");
        return;
    };

    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            &path,
            "--threshold",
            "cyclomatic=1",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Permission denied"))
        .stderr(predicate::str::contains("1 input file could not be read"));
}

/// A partially-read gate is not a passing gate (#1060): the readable
/// half being clean does not license exit 0 when the other half was
/// never analysed. Two unreadable files, so this also pins the plural
/// wording of the summary line.
#[cfg(unix)]
#[test]
fn check_exits_one_when_some_input_files_are_unreadable() {
    let dir = TempDir::new().unwrap();
    write_fixture(&dir, "clean.rs", TRIVIAL_RUST);
    if unreadable_fixture(&dir, "locked.rs", BRANCHY_RUST).is_none()
        || unreadable_fixture(&dir, "locked_too.rs", BRANCHY_RUST).is_none()
    {
        eprintln!("skipping: this process can read a mode-000 file");
        return;
    }

    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            dir.path().to_str().unwrap(),
            "--threshold",
            "cyclomatic=100",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("2 input files could not be read"));
}

/// #1060, the size class the other tests miss: `read_file_with_eol`
/// short-circuits files of three bytes or fewer, and `stat` succeeds
/// without read permission — so a tiny unreadable file used to be
/// counted as an empty one and the run exited 0 with no diagnostic at
/// all, not even the per-file `error processing` line.
#[cfg(unix)]
#[test]
fn check_exits_one_when_a_tiny_input_file_is_unreadable() {
    let dir = TempDir::new().unwrap();
    let Some(path) = unreadable_fixture(&dir, "tiny.c", "a;\n") else {
        eprintln!("skipping: this process can read a mode-000 file");
        return;
    };

    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            &path,
            "--threshold",
            "cyclomatic=1",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Permission denied"))
        .stderr(predicate::str::contains("1 input file could not be read"));
}

/// `--no-fail` suppresses threshold failures, not broken input: an
/// unreadable file is a tool error and still exits 1 (#1060).
#[cfg(unix)]
#[test]
fn check_no_fail_does_not_mask_an_unreadable_input() {
    let dir = TempDir::new().unwrap();
    let Some(path) = unreadable_fixture(&dir, "branchy.rs", BRANCHY_RUST) else {
        eprintln!("skipping: this process can read a mode-000 file");
        return;
    };

    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            &path,
            "--threshold",
            "cyclomatic=1",
            "--no-fail",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("1 input file could not be read"));
}
