//! Integration tests for `bca check --baseline` / `--write-baseline`.
//!
//! These tests drive the `bca` binary against tiny inline source
//! fixtures so they don't depend on any submodule. Each test exercises
//! one branch of the baseline contract: write produces a sorted TOML
//! file, re-read filters covered violations, regressions still fail,
//! and the flag composition matrix behaves as documented.

use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn cli() -> Command {
    Command::cargo_bin("bca").unwrap()
}

/// Rust function with cyclomatic complexity > 1: each branch
/// contributes to the count. Five branches → cyclomatic == 5.
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

/// Heavier-branching variant for "regressed function" cases: seven
/// branches, so cyclomatic > 5 even after baselining at 5.
const WORSER_RUST: &str = r#"
pub fn classify(n: i32) -> &'static str {
    if n < 0 {
        "neg"
    } else if n == 0 {
        "zero"
    } else if n < 10 {
        "small"
    } else if n < 100 {
        "medium"
    } else if n < 1000 {
        "big"
    } else if n < 10000 {
        "huge"
    } else {
        "massive"
    }
}
"#;

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

fn write_file(dir: &TempDir, name: &str, body: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, body).expect("write file");
    path
}

// -- Acceptance criteria --------------------------------------------------

#[test]
fn write_baseline_then_recheck_exits_clean() {
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline = dir.path().join("baseline.toml");

    cli()
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
        .success()
        .stderr(predicate::str::contains("wrote 1 baseline entries"));

    cli()
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
        .success();
}

#[test]
fn regressed_function_fails_even_when_baselined() {
    let dir = TempDir::new().unwrap();
    let src_path = dir.path().join("branchy.rs");
    fs::write(&src_path, BRANCHY_RUST).unwrap();
    let baseline = dir.path().join("baseline.toml");

    cli()
        .args([
            "--paths",
            src_path.to_str().unwrap(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Overwrite the same file with a worse version (same function
    // name and start_line, higher cyclomatic).
    fs::write(&src_path, WORSER_RUST).unwrap();

    cli()
        .args([
            "--paths",
            src_path.to_str().unwrap(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("classify"))
        .stderr(predicate::str::contains("cyclomatic = 7"));
}

#[test]
fn new_offender_fails_even_with_baseline() {
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline = dir.path().join("baseline.toml");

    cli()
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

    // Add a second branchy file that isn't in the baseline.
    let new_src = write_fixture(&dir, "extra.rs", BRANCHY_RUST);

    cli()
        .args([
            "--paths",
            new_src.as_str(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("classify"));
}

// -- Ratchet semantics ----------------------------------------------------

#[test]
fn improved_function_still_passes() {
    let dir = TempDir::new().unwrap();
    let src_path = write_file(&dir, "branchy.rs", WORSER_RUST);
    let baseline = dir.path().join("baseline.toml");

    cli()
        .args([
            "--paths",
            src_path.to_str().unwrap(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Replace with a less-complex function (cyclomatic drops to 5).
    fs::write(&src_path, BRANCHY_RUST).unwrap();

    cli()
        .args([
            "--paths",
            src_path.to_str().unwrap(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();
}

// -- Identity & line drift ------------------------------------------------

#[test]
fn moved_function_treated_as_new_offender() {
    let dir = TempDir::new().unwrap();
    let src_path = write_file(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline = dir.path().join("baseline.toml");

    cli()
        .args([
            "--paths",
            src_path.to_str().unwrap(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Prepend a doc comment + blank line, shifting `classify`'s
    // start_line down. The baseline still has the old start_line, so
    // the entry no longer matches → reported as a new offender.
    let shifted = format!("/// Doc comment.\n///\n///\n{BRANCHY_RUST}");
    fs::write(&src_path, shifted).unwrap();

    cli()
        .args([
            "--paths",
            src_path.to_str().unwrap(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("classify"));
}

// -- Suppression composition ----------------------------------------------

#[test]
fn write_baseline_excludes_suppressed_functions() {
    let dir = TempDir::new().unwrap();
    let suppressed_src = r#"
pub fn classify(n: i32) -> &'static str {
    // bca: suppress(cyclomatic)
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
    let src = write_fixture(&dir, "branchy.rs", suppressed_src);
    let baseline = dir.path().join("baseline.toml");

    cli()
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
        .success()
        .stderr(predicate::str::contains("wrote 0 baseline entries"));

    let content = fs::read_to_string(&baseline).unwrap();
    assert!(content.contains("version = 3"));
    assert!(!content.contains("[[entry]]"));
}

#[test]
fn write_baseline_with_no_suppress_includes_suppressed() {
    let dir = TempDir::new().unwrap();
    let suppressed_src = r#"
pub fn classify(n: i32) -> &'static str {
    // bca: suppress(cyclomatic)
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
    let src = write_fixture(&dir, "branchy.rs", suppressed_src);
    let baseline = dir.path().join("baseline.toml");

    cli()
        .args([
            "--paths",
            &src,
            "check",
            "--threshold",
            "cyclomatic=1",
            "--no-suppress",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("wrote 1 baseline entries"));
}

// -- Clap flag conflicts --------------------------------------------------

#[test]
fn baseline_and_write_baseline_conflict_at_arg_parse() {
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "trivial.rs", TRIVIAL_RUST);

    cli()
        .args([
            "--paths",
            &src,
            "check",
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            "a.toml",
            "--write-baseline",
            "b.toml",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn write_baseline_conflicts_with_output_format() {
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "trivial.rs", TRIVIAL_RUST);

    cli()
        .args([
            "--paths",
            &src,
            "check",
            "--threshold",
            "cyclomatic=1",
            "--output-format",
            "sarif",
            "--write-baseline",
            "b.toml",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn write_baseline_conflicts_with_output() {
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "trivial.rs", TRIVIAL_RUST);

    cli()
        .args([
            "--paths",
            &src,
            "check",
            "--threshold",
            "cyclomatic=1",
            "--output",
            "some.xml",
            "--write-baseline",
            "b.toml",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

// -- Error paths ----------------------------------------------------------

#[test]
fn missing_baseline_file_fails_with_exit_1() {
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "trivial.rs", TRIVIAL_RUST);

    cli()
        .args([
            "--paths",
            &src,
            "check",
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            "/definitely/does/not/exist.toml",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("read baseline"));
}

#[test]
fn malformed_baseline_toml_fails_with_exit_1() {
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "trivial.rs", TRIVIAL_RUST);
    let baseline = dir.path().join("baseline.toml");
    fs::write(&baseline, "this is not = valid toml [[[\n").unwrap();

    cli()
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
        .code(1)
        .stderr(predicate::str::contains("parse baseline"));
}

#[test]
fn higher_version_baseline_fails_with_helpful_message() {
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "trivial.rs", TRIVIAL_RUST);
    let baseline = dir.path().join("baseline.toml");
    fs::write(&baseline, "version = 99\n").unwrap();

    cli()
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
        .code(1)
        .stderr(predicate::str::contains("upgrade bca").or(predicate::str::contains("regenerate")));
}

#[test]
fn empty_baseline_file_fails_with_missing_version() {
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "trivial.rs", TRIVIAL_RUST);
    let baseline = dir.path().join("baseline.toml");
    fs::write(&baseline, "").unwrap();

    cli()
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
        .code(1)
        .stderr(predicate::str::contains("missing version field"));
}

// -- Compose with existing flags ------------------------------------------

#[test]
fn no_fail_overrides_baseline_fail() {
    let dir = TempDir::new().unwrap();
    let src_path = write_file(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline = dir.path().join("baseline.toml");

    cli()
        .args([
            "--paths",
            src_path.to_str().unwrap(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    fs::write(&src_path, WORSER_RUST).unwrap();

    cli()
        .args([
            "--paths",
            src_path.to_str().unwrap(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
            "--no-fail",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("classify"));
}

#[test]
fn stale_baseline_entries_do_not_cover_unrelated_violations() {
    // A baseline entry pointing at a function that does not exist in
    // the analyzed tree must load cleanly AND must not act as a
    // wildcard cover for unrelated live violations. Use BRANCHY_RUST
    // with a tight threshold so the filter has a real violation to
    // process; the stale entry's identity (different path, different
    // function name) must not match it.
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline = dir.path().join("baseline.toml");
    fs::write(
        &baseline,
        "version = 2\n[[entry]]\npath = \"nonexistent.rs\"\nfunction = \"ghost\"\nstart_line = 1\nmetric = \"cyclomatic\"\nvalue = 100.0\n",
    )
    .unwrap();

    cli()
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
        // Live violation must still fail the run — the stale entry
        // does not cover it. A regression that treated stale entries
        // as wildcards would flip this to exit 0.
        .code(2)
        .stderr(predicate::str::contains("classify"))
        .stderr(predicate::str::contains("cyclomatic = 5"));
}

// -- Determinism & UX -----------------------------------------------------

#[test]
fn write_baseline_byte_equal_across_two_runs() {
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline_a = dir.path().join("a.toml");
    let baseline_b = dir.path().join("b.toml");

    for out in [&baseline_a, &baseline_b] {
        cli()
            .args([
                "--paths",
                &src,
                "check",
                "--threshold",
                "cyclomatic=1",
                "--write-baseline",
                out.to_str().unwrap(),
            ])
            .assert()
            .success();
    }

    let a = fs::read(&baseline_a).unwrap();
    let b = fs::read(&baseline_b).unwrap();
    assert_eq!(
        a, b,
        "two --write-baseline runs over an unchanged tree must produce byte-identical output"
    );
}

#[test]
fn filter_emits_summary_when_any_filtered() {
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline = dir.path().join("baseline.toml");

    cli()
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

    cli()
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
        .success()
        .stderr(predicate::str::contains(
            "filtered 1 violations via baseline",
        ));
}

#[test]
fn write_baseline_creates_parent_directory() {
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline = dir.path().join("nested/sub/baseline.toml");

    cli()
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

    assert!(
        baseline.exists(),
        "atomic write must create the parent directory"
    );
}

#[test]
fn top_level_file_metric_baselined() {
    // File-level (Unit-space) violations carry the `<file>` sentinel
    // in the function slot. The baseline must round-trip that
    // sentinel verbatim and use it as a normal identity component
    // when filtering. Trigger a file-level `loc.sloc` violation
    // alongside the function-level cyclomatic one.
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline = dir.path().join("baseline.toml");

    cli()
        .args([
            "--paths",
            &src,
            "check",
            "--threshold",
            "loc.sloc=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    let content = fs::read_to_string(&baseline).unwrap();
    assert!(
        content.contains(r#"function = "<file>""#),
        "expected `<file>` sentinel entry in baseline; got:\n{content}",
    );
    assert!(content.contains(r#"metric = "loc.sloc""#));

    // Re-run with the same threshold and the freshly-written
    // baseline: the file-level violation must be filtered like any
    // other entry.
    cli()
        .args([
            "--paths",
            &src,
            "check",
            "--threshold",
            "loc.sloc=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn write_baseline_with_no_matching_files_fails_with_exit_1() {
    // The `files_dispatched == 0` guard fires before the
    // `--write-baseline` branch, so a `--paths` typo (or an empty
    // tree) is reported as a tool error rather than silently writing
    // an empty baseline. Lock in that ordering so a future refactor
    // doesn't flip the contract.
    let dir = TempDir::new().unwrap();
    let empty = dir.path().join("empty");
    fs::create_dir(&empty).unwrap();
    let baseline = dir.path().join("baseline.toml");

    cli()
        .args([
            "--paths",
            empty.to_str().unwrap(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no input files matched"));

    assert!(
        !baseline.exists(),
        "no baseline file should be created when --paths matches nothing"
    );
}

#[test]
fn clean_tree_write_baseline_produces_empty_versioned_file() {
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "trivial.rs", TRIVIAL_RUST);
    let baseline = dir.path().join("baseline.toml");

    cli()
        .args([
            "--paths",
            &src,
            "check",
            "--threshold",
            "cyclomatic=10",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("wrote 0 baseline entries"));

    let content = fs::read_to_string(&baseline).unwrap();
    assert!(content.contains("version = 3"));
    assert!(!content.contains("[[entry]]"));
}

// -- Coverage tagging (issue #356 sub-deliverable B) ---------------------

#[test]
fn regressed_violation_carries_tag_prefix() {
    // Write a baseline at cyclomatic = 5; replace source with the
    // 7-branch version; the regressed line must carry `[regr +N%]`.
    let dir = TempDir::new().unwrap();
    let src_path = write_file(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline = dir.path().join("baseline.toml");

    cli()
        .args([
            "--paths",
            src_path.to_str().unwrap(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    fs::write(&src_path, WORSER_RUST).unwrap();

    cli()
        .args([
            "--paths",
            src_path.to_str().unwrap(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        // (7-5)/5*100 = 40, rounded → +40%
        .stderr(predicate::str::contains("[regr +40%] "));
}

#[test]
fn new_violation_carries_new_tag() {
    // Baseline omits a new file; its violation must be tagged `[new]`.
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline = dir.path().join("baseline.toml");

    cli()
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

    let new_src = write_fixture(&dir, "extra.rs", BRANCHY_RUST);

    cli()
        .args([
            "--paths",
            new_src.as_str(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("[new] "));
}

#[test]
fn no_baseline_emits_unprefixed_lines() {
    // Backward-compatibility invariant: without --baseline the
    // stderr line format is byte-identical to today. No `[new]` /
    // `[regr` prefix may appear on the violation line.
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);

    cli()
        .args(["--paths", &src, "check", "--threshold", "cyclomatic=1"])
        .assert()
        .code(2)
        // The violation line must contain the function name and metric,
        // and must NOT carry a bracket tag prefix.
        .stderr(predicate::str::contains("classify: cyclomatic ="))
        .stderr(predicate::str::contains("[new]").not())
        .stderr(predicate::str::contains("[regr").not());
}

// -- Path canonicalisation (issue #376) -----------------------------------

/// Run the same `bca check --write-baseline` against the same tree
/// using two different `--paths` forms (`.` vs. the absolute repo
/// path) from the same working directory. The acceptance criterion
/// in #376 is that both invocations produce byte-identical TOML —
/// path keys must be anchor-relative, not dependent on the form the
/// user typed.
#[test]
fn write_baseline_byte_equal_across_paths_forms() {
    let dir = TempDir::new().unwrap();
    write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline_rel = dir.path().join("baseline_rel.toml");
    let baseline_abs = dir.path().join("baseline_abs.toml");
    let abs_paths = dir.path().to_str().unwrap().to_string();

    // Form A: --paths . (relative to CWD = tempdir).
    cli()
        .current_dir(dir.path())
        .args([
            "--paths",
            ".",
            "check",
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline_rel.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Form B: --paths /abs/tempdir (absolute).
    cli()
        .current_dir(dir.path())
        .args([
            "--paths",
            &abs_paths,
            "check",
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline_abs.to_str().unwrap(),
        ])
        .assert()
        .success();

    let a = fs::read_to_string(&baseline_rel).unwrap();
    let b = fs::read_to_string(&baseline_abs).unwrap();
    assert_eq!(
        a, b,
        "baselines written from `.` vs absolute `--paths` must match byte-for-byte"
    );
    // Sanity: the recorded path is anchor-relative ("branchy.rs"),
    // not `./branchy.rs` or `<tempdir>/branchy.rs`.
    assert!(
        a.contains("path = \"branchy.rs\""),
        "key must be relative to the baseline directory, got: {a}"
    );
}

/// Write a baseline with one `--paths` form, then run `--baseline`
/// with a *different* form against the same tree. Every entry must
/// match; the run exits 0. Pre-#376 this would surface every
/// existing offender as `[new]`.
#[test]
fn check_baseline_matches_across_paths_forms() {
    let dir = TempDir::new().unwrap();
    write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline = dir.path().join("baseline.toml");
    let abs_paths = dir.path().to_str().unwrap().to_string();

    // Write with --paths "." (relative).
    cli()
        .current_dir(dir.path())
        .args([
            "--paths",
            ".",
            "check",
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("wrote 1 baseline entries"));

    // Read back with absolute --paths. Pre-#376: every entry would
    // mismatch and the gate would re-fail on every offender.
    cli()
        .current_dir(dir.path())
        .args([
            "--paths",
            &abs_paths,
            "check",
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "filtered 1 violations via baseline",
        ));
}

/// A v2 baseline (pre-#376) keyed with a `./`-prefixed path must
/// still match a violation reported under the new anchor-relative
/// canonical form. The loader runs each legacy entry through the
/// v3 pipeline at parse time, so ASCII-clean v2 baselines migrate
/// transparently. A one-time deprecation warning is emitted on
/// stderr telling the user to refresh.
#[test]
fn legacy_v2_baseline_migrates_dot_prefix() {
    let dir = TempDir::new().unwrap();
    write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline = dir.path().join("baseline.toml");

    // A v2 baseline with the old `./`-prefixed key shape that
    // `--paths .` used to produce.
    fs::write(
        &baseline,
        "version = 2\n[[entry]]\n\
         path = \"./branchy.rs\"\n\
         function = \"classify\"\n\
         start_line = 2\n\
         metric = \"cyclomatic\"\n\
         value = 5.0\n",
    )
    .unwrap();

    cli()
        .current_dir(dir.path())
        .args([
            "--paths",
            ".",
            "check",
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("baseline is v2"))
        .stderr(predicate::str::contains(
            "filtered 1 violations via baseline",
        ));
}
