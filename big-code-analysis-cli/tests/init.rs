//! Integration tests for the `bca init` subcommand.
//!
//! These drive the `bca` binary against tempdirs and verify the
//! pre-#374 adoption-scaffolding contract: write three canonical
//! files (`bca-thresholds.toml`, `.bcaignore`, `.bca-baseline.toml`),
//! refuse to overwrite without `--force`, and produce a thresholds
//! file that round-trips through `bca check --config`.

use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

mod common;

fn cli() -> Command {
    common::bca_command()
}

/// Trivial Rust source — non-empty so the baseline walk has something
/// to traverse. The body is small enough that no threshold is
/// exceeded, so the baseline file ends up empty (just the version
/// preamble). That is fine — the test cares about file existence /
/// shape, not entry count.
const TRIVIAL_RUST: &str = "
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
";

#[test]
fn init_writes_three_canonical_files() {
    let dir = TempDir::new().unwrap();
    // Seed a source file so the implicit `--write-baseline` walk has
    // input. Without this, `run_check_walk` would die with "no input
    // files matched".
    fs::write(dir.path().join("lib.rs"), TRIVIAL_RUST).unwrap();

    cli()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .success()
        .stderr(predicate::str::contains("wrote"))
        .stderr(predicate::str::contains("bca-thresholds.toml"))
        .stderr(predicate::str::contains(".bcaignore"))
        .stderr(predicate::str::contains(".bca-baseline.toml"));

    assert!(dir.path().join("bca-thresholds.toml").exists());
    assert!(dir.path().join(".bcaignore").exists());
    assert!(dir.path().join(".bca-baseline.toml").exists());

    // The thresholds file must contain the canonical [thresholds]
    // table — the whole point of the scaffold.
    let body = fs::read_to_string(dir.path().join("bca-thresholds.toml")).unwrap();
    assert!(
        body.contains("[thresholds]"),
        "thresholds template missing [thresholds] table: {body}"
    );
    assert!(
        body.contains("cyclomatic"),
        "thresholds template missing canonical metric keys: {body}"
    );
}

#[test]
fn init_refuses_to_overwrite_existing_files_without_force() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("lib.rs"), TRIVIAL_RUST).unwrap();
    // Pre-create one of the canonical files with sentinel content.
    fs::write(
        dir.path().join("bca-thresholds.toml"),
        "# user's existing file\n",
    )
    .unwrap();

    cli()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("refusing to overwrite"))
        .stderr(predicate::str::contains("bca-thresholds.toml"));

    // The pre-existing file must be untouched.
    let body = fs::read_to_string(dir.path().join("bca-thresholds.toml")).unwrap();
    assert_eq!(body, "# user's existing file\n");
}

#[test]
fn init_force_overwrites_existing_files() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("lib.rs"), TRIVIAL_RUST).unwrap();
    fs::write(
        dir.path().join("bca-thresholds.toml"),
        "# user's existing file\n",
    )
    .unwrap();

    cli()
        .current_dir(dir.path())
        .args(["init", "--force"])
        .assert()
        .success();

    let body = fs::read_to_string(dir.path().join("bca-thresholds.toml")).unwrap();
    assert!(
        body.contains("[thresholds]"),
        "--force must overwrite with canonical template: {body}"
    );
}

#[test]
fn init_written_thresholds_file_validates_via_bca_check() {
    // Round-trip: `init` writes `bca-thresholds.toml`, then
    // `bca check --config <that file>` must accept it without error.
    // This catches a class of bug where the embedded template would
    // be syntactically valid TOML but reject every metric name or
    // contain unparseable limits.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("lib.rs"), TRIVIAL_RUST).unwrap();

    cli()
        .current_dir(dir.path())
        .args(["init", "--no-baseline"])
        .assert()
        .success();

    // The trivial source has cyclomatic=1, well under any of the
    // template's limits, so check exits 0 cleanly.
    cli()
        .current_dir(dir.path())
        .args(["--paths", ".", "check", "--config", "bca-thresholds.toml"])
        .assert()
        .success();
}

#[test]
fn init_no_baseline_writes_empty_placeholder() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("lib.rs"), TRIVIAL_RUST).unwrap();

    cli()
        .current_dir(dir.path())
        .args(["init", "--no-baseline"])
        .assert()
        .success();

    let baseline = fs::read_to_string(dir.path().join(".bca-baseline.toml")).unwrap();
    // Empty placeholder still carries the version key the loader
    // requires; without it `--baseline` would reject the file.
    assert!(
        baseline.contains("version = 4"),
        "empty baseline placeholder missing version key: {baseline}"
    );
    // No actual entries.
    assert!(
        !baseline.contains("[[entry]]"),
        "--no-baseline must produce empty file with no entries: {baseline}"
    );
}
