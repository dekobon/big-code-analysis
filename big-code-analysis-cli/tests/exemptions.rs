//! Integration tests for `bca exemptions` (issue #386): a unified audit
//! of the three gate-skipping tiers — in-source suppression markers,
//! `[check.exclude]` globs, and `.bca-baseline.toml` entries.
//!
//! Each test drives the real binary against a temp directory of tiny
//! inline fixtures so they don't depend on any submodule. The `.git`
//! marker halts `bca.toml` discovery at the fixture root.

use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

mod common;

fn cli() -> Command {
    common::bca_command()
}

/// Fixture with markers across two languages plus a clean file. The
/// `.git` marker stops manifest discovery at the fixture root.
fn marker_fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(
        dir.path().join("foo.rs"),
        "// bca: suppress-file(halstead)\n\
         fn parse_long() {\n    // bca: suppress\n    let x = 1;\n}\n\
         fn helper() {\n    // #lizard forgives\n    let y = 2;\n}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("bar.py"),
        "def thing():\n    # bca: suppress(cyclomatic, cognitive)\n    pass\n",
    )
    .unwrap();
    fs::write(dir.path().join("clean.rs"), "fn clean() { let z = 0; }\n").unwrap();
    dir
}

fn run_json(dir: &TempDir, extra: &[&str]) -> Value {
    let assert = {
        let mut cmd = cli();
        cmd.args([
            "--paths",
            dir.path().to_str().unwrap(),
            "exemptions",
            "--format",
            "json",
        ]);
        cmd.args(extra);
        cmd.assert().success()
    };
    let out = String::from_utf8(assert.get_output().stdout.clone()).expect("UTF-8 stdout");
    serde_json::from_str(&out).expect("valid JSON")
}

/// Acceptance: every supported marker variant found in the tree is
/// listed, with the surrounding function for scope context.
#[test]
fn lists_every_marker_variant_with_enclosing_function() {
    let dir = marker_fixture();
    let v = run_json(&dir, &[]);
    let markers = v["suppressions"]["markers"].as_array().unwrap();
    assert_eq!(markers.len(), 4, "expected 4 markers: {v:#}");

    // Native function-scoped marker attributes its enclosing function.
    let parse_long = markers
        .iter()
        .find(|m| m["function"] == "parse_long")
        .expect("parse_long marker");
    assert_eq!(parse_long["target"], "function");
    assert_eq!(parse_long["dialect"], "native");
    assert_eq!(parse_long["scope"]["kind"], "all");

    // File-scoped marker: whole-file, no enclosing function, metric list.
    let file_marker = markers
        .iter()
        .find(|m| m["target"] == "file")
        .expect("file marker");
    assert!(file_marker["function"].is_null());
    assert_eq!(file_marker["scope"]["metrics"][0], "halstead");

    // Lizard dialect recognized.
    assert!(markers.iter().any(|m| m["dialect"] == "lizard"));
    // Python marker with an explicit metric list.
    assert!(
        markers
            .iter()
            .any(|m| m["function"] == "thing" && m["scope"]["kind"] == "some")
    );
}

/// Acceptance: the audit honors the walker's `--exclude`, so a marker in
/// an excluded file is not reported.
#[test]
fn honors_walker_exclude() {
    let dir = marker_fixture();
    let v = run_json(&dir, &["--exclude", "**/foo.rs"]);
    let markers = v["suppressions"]["markers"].as_array().unwrap();
    // foo.rs held 3 of the 4 markers; only bar.py's remains.
    assert_eq!(markers.len(), 1, "foo.rs markers should be excluded: {v:#}");
    assert_eq!(markers[0]["function"], "thing");
}

/// Acceptance: the `[check.exclude]` globs and `.bca-baseline.toml`
/// entries appear in the same invocation as the markers.
#[test]
fn unifies_excludes_and_baseline_sections() {
    let dir = marker_fixture();
    fs::write(
        dir.path().join(".bca-baseline.toml"),
        "version = 4\n\
         [[entry]]\n\
         path = \"foo.rs\"\n\
         qualified = \"parse_long\"\n\
         metric = \"cognitive\"\n\
         start_line = 2\n\
         value = 29.0\n",
    )
    .unwrap();
    let baseline_path = dir.path().join(".bca-baseline.toml");
    let v = run_json(
        &dir,
        &[
            "--check-exclude",
            "tests/**",
            "--check-exclude",
            "src/gen/*.rs",
            "--baseline",
            baseline_path.to_str().unwrap(),
        ],
    );

    let excludes = v["suppressions"]["excludes"].as_array().unwrap();
    assert_eq!(excludes.len(), 2);
    assert_eq!(excludes[0], "tests/**");

    let baseline = v["suppressions"]["baseline"].as_array().unwrap();
    assert_eq!(baseline.len(), 1);
    assert_eq!(baseline[0]["qualified"], "parse_long");
    assert_eq!(baseline[0]["metric"], "cognitive");
    assert_eq!(baseline[0]["value"], 29.0);
}

/// `bca.toml`'s `[check]` keys (baseline path, exclude globs) drive the
/// audit with no CLI flags — the same sources `bca check` reads. Covers
/// the `Manifest::merge_exemptions` path that the flag-driven tests above
/// bypass.
#[test]
fn manifest_check_keys_drive_audit() {
    let dir = marker_fixture();
    fs::write(
        dir.path().join("custom-baseline.toml"),
        "version = 4\n\
         [[entry]]\n\
         path = \"foo.rs\"\n\
         qualified = \"helper\"\n\
         metric = \"cyclomatic\"\n\
         start_line = 6\n\
         value = 7.0\n",
    )
    .unwrap();
    // `baseline` is a top-level manifest key; `exclude` lives under
    // `[check]` (matching `Manifest`'s schema and `bca check`).
    fs::write(
        dir.path().join("bca.toml"),
        "baseline = \"custom-baseline.toml\"\n\
         [check]\n\
         exclude = [\"vendor/**\", \"generated/*.rs\"]\n",
    )
    .unwrap();
    // Run from inside the fixture so manifest discovery finds bca.toml
    // and its relative baseline path resolves.
    let mut cmd = cli();
    cmd.current_dir(dir.path());
    let assert = cmd
        .args(["--paths", ".", "exemptions", "--format", "json"])
        .assert()
        .success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();

    // Exclude globs came from `[check] exclude`.
    let excludes = v["suppressions"]["excludes"].as_array().unwrap();
    assert_eq!(excludes.len(), 2, "got: {v:#}");
    assert_eq!(excludes[0], "vendor/**");

    // Baseline entry came from the manifest's `[check] baseline` path.
    let baseline = v["suppressions"]["baseline"].as_array().unwrap();
    assert_eq!(baseline.len(), 1, "got: {v:#}");
    assert_eq!(baseline[0]["qualified"], "helper");
    assert_eq!(baseline[0]["metric"], "cyclomatic");
}

/// The default baseline (`.bca-baseline.toml` in the working tree) is
/// audited without an explicit `--baseline`.
#[test]
fn default_baseline_file_is_audited_when_present() {
    let dir = marker_fixture();
    fs::write(
        dir.path().join(".bca-baseline.toml"),
        "version = 4\n\
         [[entry]]\n\
         path = \"bar.py\"\n\
         qualified = \"thing\"\n\
         metric = \"nargs\"\n\
         start_line = 1\n\
         value = 5.0\n",
    )
    .unwrap();
    // Run from inside the fixture so the default relative path resolves.
    let mut cmd = cli();
    cmd.current_dir(dir.path());
    let assert = cmd
        .args([
            "--paths",
            ".",
            "exemptions",
            "--format",
            "json",
            "--only-baseline",
        ])
        .assert()
        .success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["suppressions"]["baseline"].as_array().unwrap().len(), 1);
}

/// `--only-markers` restricts the report to one section; the others
/// serialize to JSON `null` (not requested), distinct from `[]`.
#[test]
fn only_markers_nulls_other_sections() {
    let dir = marker_fixture();
    let v = run_json(&dir, &["--only-markers"]);
    assert!(v["suppressions"]["markers"].is_array());
    assert!(v["suppressions"]["excludes"].is_null());
    assert!(v["suppressions"]["baseline"].is_null());
}

/// `--only-excludes` likewise nulls markers and baseline, and does not
/// walk the tree (markers null even though the fixture has markers).
#[test]
fn only_excludes_nulls_markers_and_baseline() {
    let dir = marker_fixture();
    let v = run_json(&dir, &["--only-excludes", "--check-exclude", "vendor/**"]);
    assert!(v["suppressions"]["markers"].is_null());
    assert_eq!(v["suppressions"]["excludes"][0], "vendor/**");
    assert!(v["suppressions"]["baseline"].is_null());
}

/// The `--only-*` flags are mutually exclusive (clap usage error, exit 2).
#[test]
fn only_flags_are_mutually_exclusive() {
    let dir = marker_fixture();
    cli()
        .args([
            "--paths",
            dir.path().to_str().unwrap(),
            "exemptions",
            "--only-markers",
            "--only-baseline",
        ])
        .assert()
        .failure()
        .code(2);
}

/// The command is informational: it reports markers and still exits 0
/// (unlike `bca check`, which would gate to exit 2 on the same tree).
/// Asserting the report is non-empty rules out a trivial "exited 0
/// because it did nothing" pass.
#[test]
fn always_exits_zero_even_with_markers_present() {
    let dir = marker_fixture();
    cli()
        .args(["--paths", dir.path().to_str().unwrap(), "exemptions"])
        .assert()
        .success()
        .stdout(predicate::str::contains("parse_long"));
}

/// The default (tty) format renders human-readable section headers.
#[test]
fn tty_format_renders_section_headers() {
    let dir = marker_fixture();
    cli()
        .args(["--paths", dir.path().to_str().unwrap(), "exemptions"])
        .assert()
        .success()
        .stdout(predicate::str::contains("# In-source markers ("))
        .stdout(predicate::str::contains("parse_long"));
}

/// A nonexistent explicit `--baseline` is a tool error (exit 1), not a
/// silent empty section — a typo must not look like "no exemptions".
#[test]
fn missing_explicit_baseline_is_an_error() {
    let dir = marker_fixture();
    cli()
        .args([
            "--paths",
            dir.path().to_str().unwrap(),
            "exemptions",
            "--only-baseline",
            "--baseline",
            dir.path().join("does-not-exist.toml").to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        // Fail for the *right* reason: the baseline read, not some other
        // exit-1 path (bad args, panic). A bare `.code(1)` would pass on
        // any of those.
        .stderr(predicate::str::contains("baseline"))
        .stderr(predicate::str::contains("does-not-exist.toml"));
}
