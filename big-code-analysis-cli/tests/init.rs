//! Integration tests for the `bca init` subcommand.
//!
//! These drive the `bca` binary against tempdirs and verify the
//! adoption-scaffolding contract: write the consolidated `bca.toml`
//! manifest plus the `.bcaignore` and `.bca-baseline.toml` files it
//! references, refuse to overwrite without `--force`, and produce a
//! manifest that `bca check` auto-discovers and gates against
//! zero-config.

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
fn init_writes_canonical_files() {
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
        .stderr(predicate::str::contains("bca.toml"))
        .stderr(predicate::str::contains(".bcaignore"))
        .stderr(predicate::str::contains(".bca-baseline.toml"));

    assert!(dir.path().join("bca.toml").exists());
    assert!(dir.path().join(".bcaignore").exists());
    assert!(dir.path().join(".bca-baseline.toml").exists());

    // The manifest must carry the manifest keys plus the canonical
    // [thresholds] table — the whole point of the scaffold.
    let body = fs::read_to_string(dir.path().join("bca.toml")).unwrap();
    assert!(
        body.contains("paths = [\".\"]"),
        "manifest template missing `paths`: {body}"
    );
    assert!(
        body.contains("exclude_from = \".bcaignore\""),
        "manifest template missing `exclude_from`: {body}"
    );
    assert!(
        body.contains("baseline = \".bca-baseline.toml\""),
        "manifest template missing `baseline`: {body}"
    );
    assert!(
        body.contains("[thresholds]"),
        "manifest template missing [thresholds] table: {body}"
    );
    assert!(
        body.contains("cyclomatic"),
        "manifest template missing canonical metric keys: {body}"
    );
    // `cyclomatic_count_try` is a Rust-specific policy, so the generic
    // scaffold ships it commented out for discovery, not enabled.
    assert!(
        body.contains("# cyclomatic_count_try = false"),
        "manifest template must ship cyclomatic_count_try commented out: {body}"
    );
}

#[test]
fn init_refuses_to_overwrite_existing_files_without_force() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("lib.rs"), TRIVIAL_RUST).unwrap();
    // Pre-create one of the canonical files with sentinel content.
    fs::write(dir.path().join("bca.toml"), "# user's existing file\n").unwrap();

    cli()
        .current_dir(dir.path())
        .args(["init"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("refusing to overwrite"))
        .stderr(predicate::str::contains("bca.toml"));

    // The pre-existing file must be untouched.
    let body = fs::read_to_string(dir.path().join("bca.toml")).unwrap();
    assert_eq!(body, "# user's existing file\n");
}

#[test]
fn init_force_overwrites_existing_files() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("lib.rs"), TRIVIAL_RUST).unwrap();
    fs::write(dir.path().join("bca.toml"), "# user's existing file\n").unwrap();

    cli()
        .current_dir(dir.path())
        .args(["init", "--force"])
        .assert()
        .success();

    let body = fs::read_to_string(dir.path().join("bca.toml")).unwrap();
    assert!(
        body.contains("[thresholds]"),
        "--force must overwrite with canonical template: {body}"
    );
}

#[test]
fn init_scaffold_is_discovered_by_zero_config_check() {
    // Round-trip: `init` writes `bca.toml`, then a bare `bca check`
    // (no `--config`, no `--paths`, no `--threshold`) run from the
    // scaffolded directory must auto-discover the manifest (#374, #483,
    // #488) and gate against it. This proves the scaffold is actually
    // usable zero-config, not merely syntactically valid TOML.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("lib.rs"), TRIVIAL_RUST).unwrap();

    cli()
        .current_dir(dir.path())
        .args(["init", "--no-baseline"])
        .assert()
        .success();

    // The trivial source has cyclomatic=1, well under any of the
    // manifest's limits, so a bare `bca check` exits 0 cleanly. Crucially
    // there is NO `--config` / `--threshold` flag here: success proves
    // the gate ran against the auto-discovered manifest's [thresholds],
    // not that it silently no-op'd. A missing/ignored manifest would
    // instead die with "no thresholds configured" (exit 1).
    cli()
        .current_dir(dir.path())
        .args(["check"])
        .assert()
        .success();
}

#[test]
fn init_zero_config_check_reports_missing_thresholds_without_manifest() {
    // Negative control for `init_scaffold_is_discovered_by_zero_config_check`:
    // without a scaffolded manifest, a bare `bca check` has no thresholds
    // to gate against and must die with the "no thresholds configured"
    // diagnostic. This pins that the prior test's success is attributable
    // to the manifest the scaffold wrote, not to `check` succeeding on an
    // empty threshold set.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("lib.rs"), TRIVIAL_RUST).unwrap();

    cli()
        .current_dir(dir.path())
        .args(["check"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no thresholds configured"));
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
