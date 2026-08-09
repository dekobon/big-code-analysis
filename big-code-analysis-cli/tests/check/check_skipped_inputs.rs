//! Integration tests for the `bca check` skipped-input summary and the
//! `--strict` gate profile (issue #1055).
//!
//! Two inputs the branch under test controls — a generated-code marker
//! in a file's head and a committed `.gitignore` — used to remove files
//! from the gate with nothing on stderr. The gate now reports what it
//! declined to look at by default (loud, not strict), and `--strict` /
//! `[check] strict` flips both skip defaults for PR-gate use.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

use crate::common;

/// Hermetic `bca` anchored at `dir`, with the user-level global
/// gitignore isolated so an ignore rule on the developer's machine
/// cannot leak into the measured file set (the ignored-file tally is
/// exactly what these tests assert on).
fn cli(dir: &Path) -> Command {
    let mut cmd = common::cli_in(dir);
    cmd.env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null");
    cmd
}

/// Cyclomatic == 4 (three decision points plus one), against the
/// `cyclomatic=1` gate every test here runs.
fn branchy(fn_name: &str) -> String {
    format!(
        "pub fn {fn_name}(n: i32) -> i32 {{ \
         if n < 0 {{ 1 }} else if n == 0 {{ 2 }} else if n < 10 {{ 3 }} else {{ 4 }} }}\n"
    )
}

/// Cyclomatic == 1: passes the `cyclomatic=1` gate. Present in most
/// fixtures so the walk keeps at least one analysable file — the
/// sole-input case is its own test.
const CLEAN: &str = "pub fn ok() -> i32 { 1 }\n";

/// Fixture tree under a `proj/` subdir (the tempdir root itself is a
/// hidden `.tmp*` directory, which the walker would prune as a hidden
/// entry if passed through a parent). Returns the tempdir guard plus
/// the project path.
fn fixture(files: &[(&str, &str)]) -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let proj = dir.path().join("proj");
    fs::create_dir(&proj).unwrap();
    for (name, contents) in files {
        fs::write(proj.join(name), contents).unwrap();
    }
    (dir, proj)
}

/// Run `bca check --no-config --threshold cyclomatic=1` over `proj`
/// with `extra` flags appended.
fn check(dir: &TempDir, proj: &Path, extra: &[&str]) -> assert_cmd::assert::Assert {
    let mut args = vec![
        "check",
        "--no-config",
        "--paths",
        proj.to_str().unwrap(),
        "--threshold",
        "cyclomatic=1",
    ];
    args.extend_from_slice(extra);
    cli(dir.path()).args(&args).assert()
}

/// Bypass A as filed: a `@generated` marker on line 1 removes the
/// violator from the gate (exit 0) — but the summary now says so.
#[test]
fn generated_marker_bypasses_the_gate_but_the_summary_says_so() {
    let marked = format!("// @generated\n{}", branchy("offender"));
    let (dir, proj) = fixture(&[("violator.rs", &marked), ("ok.rs", CLEAN)]);

    check(&dir, &proj, &[])
        .success()
        .stderr(predicate::str::contains(
            "bca: 1 file not checked (1 generated) — pass --report-skipped to list them",
        ));
}

/// The detector scans a 50-line / 5 KiB head window, not line 1, so a
/// marker buried in a license header bypasses too. Pinned so a future
/// narrowing of the window cannot pass unnoticed on the line-1 case
/// alone.
#[test]
fn marker_buried_in_a_license_header_still_bypasses() {
    let marked = format!(
        "/* Copyright 2026 Example Corp.\n\
         \x20* Licensed under Apache-2.0.\n\
         \x20* Regenerate with: make protos\n\
         \x20* @generated\n\
         \x20*/\n{}",
        branchy("offender")
    );
    let (dir, proj) = fixture(&[("violator.rs", &marked), ("ok.rs", CLEAN)]);

    check(&dir, &proj, &[])
        .success()
        .stderr(predicate::str::contains("1 file not checked (1 generated)"));
}

/// All three marker phrases are live, including the
/// plausible-deniability `DO NOT EDIT` form that reads as ordinary
/// housekeeping.
#[test]
fn do_not_edit_and_generated_code_markers_are_equally_live() {
    let housekeeping = format!(
        "// DO NOT EDIT — see docs/schema.md before changing\n{}",
        branchy("offender_a")
    );
    let shouty = format!("// GENERATED CODE\n{}", branchy("offender_b"));
    let (dir, proj) = fixture(&[
        ("do_not_edit.rs", &housekeeping),
        ("generated_code.rs", &shouty),
        ("ok.rs", CLEAN),
    ]);

    check(&dir, &proj, &[])
        .success()
        .stderr(predicate::str::contains(
            "2 files not checked (2 generated)",
        ));
}

/// Bypass B as filed: a `.gitignore` committed in the tree under test
/// drops the violator from the walk (exit 0) — but the summary now
/// says so.
#[test]
fn committed_gitignore_drops_the_violator_with_a_summary() {
    let (dir, proj) = fixture(&[
        ("violator.rs", &branchy("offender")),
        ("ok.rs", CLEAN),
        (".gitignore", "violator.rs\n"),
    ]);

    check(&dir, &proj, &[])
        .success()
        .stderr(predicate::str::contains(
            "bca: 1 file not checked (1 ignored) — pass --report-skipped to list them",
        ));
}

/// The nuance the issue records: when the ignored file was the *only*
/// analysable input, the run is a tool error (exit 1), not a pass —
/// and the summary still lands above the error so the cause is named.
#[test]
fn sole_ignored_input_is_a_tool_error_not_a_pass() {
    let (dir, proj) = fixture(&[
        ("violator.rs", &branchy("offender")),
        (".gitignore", "violator.rs\n"),
    ]);

    check(&dir, &proj, &[])
        .code(1)
        .stderr(predicate::str::contains("1 file not checked (1 ignored)"))
        .stderr(predicate::str::contains("no input files matched"));
}

/// A run that skipped nothing stays byte-for-byte silent about it.
#[test]
fn a_clean_run_emits_no_summary() {
    let (dir, proj) = fixture(&[("ok.rs", CLEAN)]);

    check(&dir, &proj, &[])
        .success()
        .stderr(predicate::str::contains("not checked").not());
}

/// Both causes in one run are counted apart, generated first.
#[test]
fn generated_and_ignored_files_are_counted_apart() {
    let marked = format!("// @generated\n{}", branchy("offender_a"));
    let (dir, proj) = fixture(&[
        ("generated.rs", &marked),
        ("ignored.rs", &branchy("offender_b")),
        ("ok.rs", CLEAN),
        (".gitignore", "ignored.rs\n"),
    ]);

    check(&dir, &proj, &[])
        .success()
        .stderr(predicate::str::contains(
            "2 files not checked (1 generated, 1 ignored)",
        ));
}

/// `--strict` restores generated files to the gate: the marker no
/// longer bypasses (exit 2), and with nothing skipped there is no
/// summary either.
#[test]
fn strict_flag_restores_generated_files_to_the_gate() {
    let marked = format!("// @generated\n{}", branchy("offender"));
    let (dir, proj) = fixture(&[("violator.rs", &marked), ("ok.rs", CLEAN)]);

    check(&dir, &proj, &["--strict"])
        .code(2)
        .stdout(predicate::str::contains("offender"))
        .stderr(predicate::str::contains("not checked").not());
}

/// `--strict` restores ignore-dropped files to the gate.
#[test]
fn strict_flag_restores_ignored_files_to_the_gate() {
    let (dir, proj) = fixture(&[
        ("violator.rs", &branchy("offender")),
        ("ok.rs", CLEAN),
        (".gitignore", "violator.rs\n"),
    ]);

    check(&dir, &proj, &["--strict"])
        .code(2)
        .stdout(predicate::str::contains("offender"))
        .stderr(predicate::str::contains("not checked").not());
}

/// The constituent flag `--no-skip-generated` restores generated files
/// on its own — `--strict` is a profile over it, not a replacement.
#[test]
fn no_skip_generated_alone_restores_generated_files() {
    let marked = format!("// @generated\n{}", branchy("offender"));
    let (dir, proj) = fixture(&[("violator.rs", &marked), ("ok.rs", CLEAN)]);

    check(&dir, &proj, &["--no-skip-generated"])
        .code(2)
        .stdout(predicate::str::contains("offender"));
}

/// The constituent flag `--no-ignore` restores ignore-dropped files on
/// its own.
#[test]
fn no_ignore_alone_restores_ignored_files() {
    let (dir, proj) = fixture(&[
        ("violator.rs", &branchy("offender")),
        ("ok.rs", CLEAN),
        (".gitignore", "violator.rs\n"),
    ]);

    check(&dir, &proj, &["--no-ignore"])
        .code(2)
        .stdout(predicate::str::contains("offender"));
}

/// `[check] strict = true` in a discovered `bca.toml` enables the
/// profile without any CLI flag, the key is recognized (no
/// unknown-key warning), and pairing it with an explicit `--strict`
/// changes nothing.
#[test]
fn manifest_strict_key_enables_the_profile() {
    let marked = format!("// @generated\n{}", branchy("offender"));
    // `_dir` keeps the tempdir guard alive; the command anchors at
    // `proj` for manifest discovery instead.
    let (_dir, proj) = fixture(&[("violator.rs", &marked), ("ok.rs", CLEAN)]);
    // A `.git` marker halts manifest discovery at the fixture root, so
    // the run picks up this `bca.toml` and nothing above it.
    fs::create_dir(proj.join(".git")).unwrap();
    fs::write(
        proj.join("bca.toml"),
        "[check]\nstrict = true\n\n[thresholds]\ncyclomatic = 1\n",
    )
    .unwrap();

    for extra in [&[][..], &["--strict"][..]] {
        let mut args = vec!["check", "--paths", "."];
        args.extend_from_slice(extra);
        cli(&proj)
            .args(&args)
            .assert()
            .code(2)
            .stdout(predicate::str::contains("offender"))
            .stderr(predicate::str::contains("ignoring unrecognized key").not());
    }
}

/// `--report-skipped` names each ignore-dropped file (the generated
/// listing already existed) and the summary drops the hint that would
/// point at a flag the user just passed.
#[test]
fn report_skipped_lists_ignored_files_and_drops_the_hint() {
    let (dir, proj) = fixture(&[
        ("violator.rs", &branchy("offender")),
        ("ok.rs", CLEAN),
        (".gitignore", "violator.rs\n"),
    ]);

    check(&dir, &proj, &["--report-skipped"])
        .success()
        .stderr(predicate::str::contains("note: skipped (ignored):"))
        .stderr(predicate::str::contains("violator.rs"))
        .stderr(predicate::str::contains(
            "bca: 1 file not checked (1 ignored)",
        ))
        .stderr(predicate::str::contains("pass --report-skipped").not());
}

/// `--paths-from -` (stdin) is read exactly once: the ignored-file
/// measurement resolves the seed list twice, and a second read of an
/// exhausted stdin would silently empty one side of the diff and
/// suppress the summary.
#[test]
fn paths_from_stdin_is_read_once() {
    let (dir, proj) = fixture(&[
        ("violator.rs", &branchy("offender")),
        ("ok.rs", CLEAN),
        (".gitignore", "violator.rs\n"),
    ]);

    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths-from",
            "-",
            "--threshold",
            "cyclomatic=1",
        ])
        .write_stdin(format!("{}\n", proj.display()))
        .assert()
        .success()
        .stderr(predicate::str::contains("1 file not checked (1 ignored)"));
}

/// The on-disk `--paths-from <file>` form keeps working through the
/// same materialize-once path.
#[test]
fn paths_from_file_is_honoured() {
    let (dir, proj) = fixture(&[
        ("violator.rs", &branchy("offender")),
        ("ok.rs", CLEAN),
        (".gitignore", "violator.rs\n"),
    ]);
    let list = dir.path().join("list.txt");
    fs::write(&list, format!("{}\n", proj.display())).unwrap();

    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths-from",
            list.to_str().unwrap(),
            "--threshold",
            "cyclomatic=1",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("1 file not checked (1 ignored)"));
}

/// `--print-effective-config` records the profile: the `strict` field
/// plus the two flipped skip defaults it implies.
#[test]
fn effective_config_records_the_strict_profile() {
    let (dir, proj) = fixture(&[("ok.rs", CLEAN)]);

    let output = check(&dir, &proj, &["--strict", "--print-effective-config=json"])
        .success()
        .get_output()
        .stdout
        .clone();
    let config: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(config["check"]["strict"], serde_json::Value::Bool(true));
    assert_eq!(config["check"]["no_ignore"], serde_json::Value::Bool(true));
    assert_eq!(
        config["check"]["no_skip_generated"],
        serde_json::Value::Bool(true)
    );
}
