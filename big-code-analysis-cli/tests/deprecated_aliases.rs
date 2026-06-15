#![allow(missing_docs)]
//! End-to-end coverage for the unified one-cycle deprecation warnings
//! (#646): using a deprecated flag spelling still works but draws a
//! `warning: ... is deprecated` line, while the canonical spelling is
//! silent.

use std::io::Write;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

mod common;

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

/// `--num-jobs` is the deprecated spelling of `--jobs`: still honored
/// (exit 0, metrics emitted) but it now warns.
#[test]
fn num_jobs_alias_works_and_warns() {
    let dir = TempDir::new().expect("tempdir");
    let fixture = write_rust_fixture(&dir);

    cli(&dir)
        .args(["metrics", "--paths", &fixture, "--num-jobs", "1"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "`--num-jobs` is deprecated; use `--jobs`",
        ));
}

/// The canonical `--jobs` spelling emits no deprecation chatter.
#[test]
fn jobs_canonical_is_silent() {
    let dir = TempDir::new().expect("tempdir");
    let fixture = write_rust_fixture(&dir);

    cli(&dir)
        .args(["metrics", "--paths", &fixture, "--jobs", "1"])
        .assert()
        .success()
        .stderr(predicate::str::contains("is deprecated").not());
}

/// `--warning` (singular) is the deprecated spelling of `--warnings`.
#[test]
fn warning_singular_alias_warns() {
    let dir = TempDir::new().expect("tempdir");
    let fixture = write_rust_fixture(&dir);

    cli(&dir)
        .args(["metrics", "--paths", &fixture, "--warning"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "`--warning` is deprecated; use `--warnings`",
        ));
}

/// `--language-type` is the deprecated spelling of `--language`.
#[test]
fn language_type_alias_warns() {
    let dir = TempDir::new().expect("tempdir");
    let fixture = write_rust_fixture(&dir);

    cli(&dir)
        .args(["metrics", "--paths", &fixture, "--language-type", "rust"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "`--language-type` is deprecated; use `--language`",
        ));
}

/// `--output-format` warns and — outside `check` — names `--format` as the
/// replacement (issue #513).
#[test]
fn output_format_alias_warns_with_format_replacement() {
    let dir = TempDir::new().expect("tempdir");
    let fixture = write_rust_fixture(&dir);

    cli(&dir)
        .args(["metrics", "--paths", &fixture, "--output-format", "json"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "`--output-format` is deprecated; use `--format`",
        ));
}

/// On `check` the `--output-format` replacement is the report-dialect
/// flag `--report-format`, not the data `--format` (issue #659).
#[test]
fn output_format_alias_on_check_names_report_format() {
    let dir = TempDir::new().expect("tempdir");
    let fixture = write_rust_fixture(&dir);

    cli(&dir)
        .args([
            "check",
            "--paths",
            &fixture,
            "--threshold",
            "cyclomatic=1",
            "--no-fail",
            "--output-format",
            "sarif",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "`--output-format` is deprecated; use `--report-format`",
        ));
}

/// The canonical data `--format` spelling on `metrics` is NOT a
/// deprecated alias and must stay silent — it is the context-sensitive
/// case the global argv scan deliberately excludes.
#[test]
fn format_canonical_on_metrics_is_silent() {
    let dir = TempDir::new().expect("tempdir");
    let fixture = write_rust_fixture(&dir);

    cli(&dir)
        .args(["metrics", "--paths", &fixture, "--format", "json"])
        .assert()
        .success()
        .stderr(predicate::str::contains("is deprecated").not());
}

/// #832: `--ls` / `--le` are the deprecated spellings of `--line-start` /
/// `--line-end` on the `dump` (and `find`) range bounds. Each must draw
/// its own deprecation warning.
#[test]
fn ls_alias_warns() {
    let dir = TempDir::new().expect("tempdir");
    let fixture = write_rust_fixture(&dir);
    cli(&dir)
        .args(["dump", "--paths", &fixture, "--ls", "1"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "`--ls` is deprecated; use `--line-start`",
        ));
}

#[test]
fn le_alias_warns() {
    let dir = TempDir::new().expect("tempdir");
    let fixture = write_rust_fixture(&dir);
    cli(&dir)
        .args(["dump", "--paths", &fixture, "--le", "1"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "`--le` is deprecated; use `--line-end`",
        ));
}

/// #832: `--only-markers` / `--only-excludes` / `--only-baseline` are the
/// deprecated spellings of the `--*-only` exemptions filters. They are
/// mutually exclusive, so each is exercised in its own invocation.
#[test]
fn only_markers_alias_warns() {
    let dir = TempDir::new().expect("tempdir");
    let fixture = write_rust_fixture(&dir);
    cli(&dir)
        .args(["exemptions", "--paths", &fixture, "--only-markers"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "`--only-markers` is deprecated; use `--markers-only`",
        ));
}

#[test]
fn only_excludes_alias_warns() {
    let dir = TempDir::new().expect("tempdir");
    let fixture = write_rust_fixture(&dir);
    cli(&dir)
        .args(["exemptions", "--paths", &fixture, "--only-excludes"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "`--only-excludes` is deprecated; use `--excludes-only`",
        ));
}

#[test]
fn only_baseline_alias_warns() {
    let dir = TempDir::new().expect("tempdir");
    let fixture = write_rust_fixture(&dir);
    cli(&dir)
        .args(["exemptions", "--paths", &fixture, "--only-baseline"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "`--only-baseline` is deprecated; use `--baseline-only`",
        ));
}

/// #836: paths named `vcs` and `jit` after a `--` end-of-options marker
/// are positional values, never a subcommand pair. The subcommand scan
/// must stop at `--` (like the flag scan), so no spurious `jit` is
/// deprecated warning fires.
#[test]
fn vcs_jit_paths_after_double_dash_are_silent() {
    let dir = TempDir::new().expect("tempdir");
    // Real files named exactly `vcs` and `jit` so `metrics` succeeds and
    // the only thing under test is the deprecation scan, not a path error.
    // `cli_in` roots the command at `dir`, so bare relative names appear
    // verbatim in argv — the literal `vcs` / `jit` tokens the scan sees.
    for name in ["vcs", "jit"] {
        let path = dir.path().join(name);
        let mut f = std::fs::File::create(&path).expect("create fixture");
        f.write_all(b"fn f() {}\n").expect("write fixture");
    }

    cli(&dir)
        .args(["metrics", "--language", "rust", "--", "vcs", "jit"])
        .assert()
        .success()
        .stderr(predicate::str::contains("is deprecated").not());
}

/// `--format text` requests the default human-readable output explicitly
/// (a permanent, supported spelling) and must not draw a deprecation
/// warning.
#[test]
fn format_text_value_is_silent() {
    let dir = TempDir::new().expect("tempdir");
    let fixture = write_rust_fixture(&dir);

    cli(&dir)
        .args(["metrics", "--paths", &fixture, "--format", "text"])
        .assert()
        .success()
        .stderr(predicate::str::contains("is deprecated").not());
}
