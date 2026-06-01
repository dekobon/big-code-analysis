//! Integration tests for the tiered exit codes added by issue #385.
//!
//! `bca check` keeps the stable 0/1/2 contract by default; passing
//! `--strict-exit-codes` (or `[check] exit_codes = "tiered"` in
//! `bca.toml`) splits exit 2 by severity:
//!
//! - `2` — new offenders only.
//! - `3` — baseline regressions only.
//! - `4` — both.
//! - `5` — a `--tier=soft` violation that also breaches the hard limit.
//!
//! Each test drives the real binary against tiny inline fixtures so they
//! don't depend on any submodule.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

mod common;

/// Hermetic `bca` builder: anchors the process cwd at `dir` (a
/// `tempfile::tempdir()` with no `.git` ancestor) so `bca check` cannot
/// auto-discover the repo's own `bca.toml` / `.bca-baseline.toml` and
/// filter the inline fixtures against repo state (#491). The
/// manifest-discovery tests below pass their own fixture dir (which
/// *does* carry a `.git` + `bca.toml`), so they still exercise
/// discovery against fixture state, not repo state.
fn cli(dir: &Path) -> Command {
    common::cli_in(dir)
}

/// Build a Rust source whose sole function `classify` has cyclomatic
/// complexity `decisions + 1`: a base of 1, one `if`, and
/// `decisions - 1` `else if` arms. `decisions` must be >= 1.
fn branchy_source(decisions: usize) -> String {
    assert!(decisions >= 1, "need at least one decision point");
    let mut body = String::from("pub fn classify(n: i32) -> i32 {\n    if n < 0 { 0 }\n");
    for i in 1..decisions {
        let _ = writeln!(body, "    else if n < {i} {{ {i} }}");
    }
    body.push_str("    else { -1 }\n}\n");
    body
}

fn write_file(dir: &TempDir, name: &str, body: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, body).expect("write fixture");
    path
}

/// Write `branchy.rs` at the given cyclomatic complexity and return its
/// path as a UTF-8 string.
fn write_branchy(dir: &TempDir, cyclomatic: usize) -> String {
    let path = write_file(dir, "branchy.rs", &branchy_source(cyclomatic - 1));
    path.to_str().expect("utf8 path").to_owned()
}

// -- Default contract unchanged -------------------------------------------

#[test]
fn default_mode_exits_two_on_violation() {
    let dir = TempDir::new().unwrap();
    let src = write_branchy(&dir, 5);

    cli(dir.path())
        .args(["--paths", &src, "check", "--threshold", "cyclomatic=1"])
        .assert()
        .code(2);
}

#[test]
fn default_mode_collapses_regression_to_two() {
    // A regression exits 3 under `--strict-exit-codes`; in the default
    // contract it must collapse to 2. This is the discriminating control
    // for the default mode — it proves the tiered split does not leak
    // when the flag is absent (a plain violation test cannot, because
    // new-only maps to 2 in both modes).
    let dir = TempDir::new().unwrap();
    let src_path = dir.path().join("branchy.rs");
    fs::write(&src_path, branchy_source(4)).unwrap(); // cyclomatic 5
    let src = src_path.to_str().unwrap().to_owned();
    let baseline = dir.path().join("baseline.toml");

    cli(dir.path())
        .args([
            "--paths",
            &src,
            "check",
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    fs::write(&src_path, branchy_source(6)).unwrap(); // worsen to cyclomatic 7

    cli(dir.path())
        .args([
            "--paths",
            &src,
            "check",
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .code(2);
}

#[test]
fn strict_clean_exits_zero() {
    let dir = TempDir::new().unwrap();
    let src = write_branchy(&dir, 5);

    cli(dir.path())
        .args([
            "--paths",
            &src,
            "check",
            "--threshold",
            "cyclomatic=100",
            "--strict-exit-codes",
        ])
        .assert()
        .success();
}

#[test]
fn no_fail_overrides_strict_exit_codes() {
    let dir = TempDir::new().unwrap();
    let src = write_branchy(&dir, 5);

    cli(dir.path())
        .args([
            "--paths",
            &src,
            "check",
            "--threshold",
            "cyclomatic=1",
            "--strict-exit-codes",
            "--no-fail",
        ])
        .assert()
        .success();
}

// -- Tiered codes 2/3/4 ---------------------------------------------------

#[test]
fn strict_new_only_exits_two() {
    // No baseline: every violation is an unbaselined new offender.
    let dir = TempDir::new().unwrap();
    let src = write_branchy(&dir, 5);

    cli(dir.path())
        .args([
            "--paths",
            &src,
            "check",
            "--threshold",
            "cyclomatic=1",
            "--strict-exit-codes",
        ])
        .assert()
        .code(2);
}

#[test]
fn strict_regression_only_exits_three() {
    let dir = TempDir::new().unwrap();
    let src_path = dir.path().join("branchy.rs");
    fs::write(&src_path, branchy_source(4)).unwrap(); // cyclomatic 5
    let src = src_path.to_str().unwrap().to_owned();
    let baseline = dir.path().join("baseline.toml");

    cli(dir.path())
        .args([
            "--paths",
            &src,
            "check",
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Worsen the same function: cyclomatic 5 -> 7, a regression of the
    // baselined offender.
    fs::write(&src_path, branchy_source(6)).unwrap();

    cli(dir.path())
        .args([
            "--paths",
            &src,
            "check",
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
            "--strict-exit-codes",
        ])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("[regr"));
}

#[test]
fn strict_mixed_exits_four() {
    let dir = TempDir::new().unwrap();
    let a = write_file(&dir, "a.rs", &branchy_source(4));
    let baseline = dir.path().join("baseline.toml");

    // Baseline captures only file a.rs (cyclomatic 5).
    cli(dir.path())
        .args([
            "--paths",
            dir.path().to_str().unwrap(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Worsen a.rs (regression) and add b.rs (new offender).
    fs::write(&a, branchy_source(6)).unwrap(); // cyclomatic 7
    fs::write(dir.path().join("b.rs"), branchy_source(4)).unwrap();

    cli(dir.path())
        .args([
            "--paths",
            dir.path().to_str().unwrap(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
            "--strict-exit-codes",
        ])
        .assert()
        .code(4);
}

// -- Tiered code 5: hard breach under the soft tier -----------------------

#[test]
fn strict_hard_breach_under_soft_tier_exits_five() {
    let dir = TempDir::new().unwrap();
    let src = write_branchy(&dir, 12); // cyclomatic 12, over the hard 10
    let config = dir.path().join("thresholds.toml");
    fs::write(&config, "[thresholds]\ncyclomatic = 10\n").unwrap();

    cli(dir.path())
        .args([
            "--paths",
            &src,
            "check",
            "--config",
            config.to_str().unwrap(),
            "--tier",
            "soft",
            "--headroom",
            "0.5", // soft limit 5; value 12 breaches the hard 10
            "--strict-exit-codes",
        ])
        .assert()
        .code(5);
}

#[test]
fn strict_soft_encroachment_exits_two_not_five() {
    let dir = TempDir::new().unwrap();
    let src = write_branchy(&dir, 7); // cyclomatic 7: over soft 5, under hard 10
    let config = dir.path().join("thresholds.toml");
    fs::write(&config, "[thresholds]\ncyclomatic = 10\n").unwrap();

    cli(dir.path())
        .args([
            "--paths",
            &src,
            "check",
            "--config",
            config.to_str().unwrap(),
            "--tier",
            "soft",
            "--headroom",
            "0.5",
            "--strict-exit-codes",
        ])
        .assert()
        .code(2);
}

// -- Manifest opt-in ------------------------------------------------------

/// Create a fixture repo with a `.git` marker so manifest discovery
/// halts at the fixture root.
fn manifest_fixture(manifest: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join("bca.toml"), manifest).unwrap();
    fs::write(dir.path().join("branchy.rs"), branchy_source(4)).unwrap();
    dir
}

#[test]
fn manifest_exit_codes_tiered_reported_in_effective_config() {
    let dir = manifest_fixture(
        "paths = [\".\"]\n\n[thresholds]\ncyclomatic = 1\n\n[check]\nexit_codes = \"tiered\"\n",
    );

    cli(dir.path())
        .args(["check", "--print-effective-config"])
        .assert()
        .success()
        .stdout(predicate::str::contains("exit_codes = \"tiered\""));
}

#[test]
fn default_effective_config_reports_default_exit_codes() {
    let dir = manifest_fixture("paths = [\".\"]\n\n[thresholds]\ncyclomatic = 1\n");

    cli(dir.path())
        .args(["check", "--print-effective-config"])
        .assert()
        .success()
        .stdout(predicate::str::contains("exit_codes = \"default\""));
}

#[test]
fn manifest_exit_codes_invalid_value_is_tool_error() {
    let dir = manifest_fixture(
        "paths = [\".\"]\n\n[thresholds]\ncyclomatic = 1\n\n[check]\nexit_codes = \"bogus\"\n",
    );

    cli(dir.path())
        .arg("check")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("exit_codes"));
}
