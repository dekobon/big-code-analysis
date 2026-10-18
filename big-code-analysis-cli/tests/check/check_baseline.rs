//! Integration tests for `bca check --baseline` / `--write-baseline`.
//!
//! These tests drive the `bca` binary against tiny inline source
//! fixtures so they don't depend on any submodule. Each test exercises
//! one branch of the baseline contract: write produces a sorted TOML
//! file, re-read filters covered violations, regressions still fail,
//! and the flag composition matrix behaves as documented.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

use crate::common;
use crate::common::fixtures::{BRANCHY_RUST, TRIVIAL_RUST};

/// Hermetic `bca` builder: anchors the process cwd at `dir` (a
/// `tempfile::tempdir()` with no `.git` ancestor) so `bca check` cannot
/// auto-discover the repo's own `bca.toml` / `.bca-baseline.toml` and
/// filter the inline fixtures against repo state (#491).
fn cli(dir: &Path) -> Command {
    common::cli_in(dir)
}

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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("wrote 1 baseline entries"));

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn write_baseline_bare_uses_manifest_baseline() {
    // #496: a bare `--write-baseline` (no path) writes to the `baseline`
    // key from the auto-discovered `bca.toml`, so the path lives in one
    // place. Build a tempdir manifest + `.git` marker so discovery stops
    // there, then run a flagless `bca check --write-baseline`.
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(
        dir.path().join("bca.toml"),
        "paths = [\".\"]\nbaseline = \".bca-baseline.toml\"\n\n[thresholds]\ncyclomatic = 1\n",
    )
    .unwrap();
    fs::write(dir.path().join("branchy.rs"), BRANCHY_RUST).unwrap();
    let manifest_baseline = dir.path().join(".bca-baseline.toml");
    assert!(!manifest_baseline.exists(), "baseline must not exist yet");

    cli(dir.path())
        .args(["check", "--write-baseline"])
        .assert()
        .success();

    // Written to the manifest's `baseline` path, capturing the branchy
    // offender — not to a default or the CWD under a different name.
    assert!(
        manifest_baseline.exists(),
        "bare --write-baseline must write to the manifest `baseline` path"
    );
    let written = fs::read_to_string(&manifest_baseline).unwrap();
    assert!(written.contains("version ="), "wrote a real baseline file");
    assert!(
        written.contains("branchy.rs"),
        "captured the offender: {written}"
    );
}

#[test]
fn write_baseline_bare_without_manifest_baseline_errors() {
    // With no manifest `baseline` to fall back to, a bare
    // `--write-baseline` has no path and must hard-error (not silently
    // skip the write). The tempdir has no `bca.toml`, and a `.git`
    // marker stops manifest discovery at the tempdir so an ancestor
    // `bca.toml` (e.g. a TMPDIR nested under a repo checkout) can't be
    // discovered and silently satisfy the bare write.
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    let src = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--write-baseline needs a path"));
}

#[test]
fn regressed_function_fails_even_when_baselined() {
    let dir = TempDir::new().unwrap();
    let src_path = dir.path().join("branchy.rs");
    fs::write(&src_path, BRANCHY_RUST).unwrap();
    let baseline = dir.path().join("baseline.toml");

    cli(dir.path())
        .args([
            "check",
            "--paths",
            src_path.to_str().unwrap(),
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            src_path.to_str().unwrap(),
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("classify"))
        .stdout(predicate::str::contains("cyclomatic = 7"));
}

#[test]
fn new_offender_fails_even_with_baseline() {
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline = dir.path().join("baseline.toml");

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Add a second branchy file that isn't in the baseline.
    let new_src = write_fixture(&dir, "extra.rs", BRANCHY_RUST);

    cli(dir.path())
        .args([
            "check",
            "--paths",
            new_src.as_str(),
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("classify"));
}

// -- Ratchet semantics ----------------------------------------------------

#[test]
fn improved_function_still_passes() {
    let dir = TempDir::new().unwrap();
    let src_path = write_file(&dir, "branchy.rs", WORSER_RUST);
    let baseline = dir.path().join("baseline.toml");

    cli(dir.path())
        .args([
            "check",
            "--paths",
            src_path.to_str().unwrap(),
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Replace with a less-complex function (cyclomatic drops to 5).
    fs::write(&src_path, BRANCHY_RUST).unwrap();

    cli(dir.path())
        .args([
            "check",
            "--paths",
            src_path.to_str().unwrap(),
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success()
        // The improved function (cyclomatic = 5) is still a live
        // violation against `cyclomatic=1`, but ratchets below the
        // recorded 7, so it must classify `Covered` and be filtered.
        // Assert the filter actually fired — a clean exit alone could
        // mask a parse that produced no violation at all (#894).
        .stderr(predicate::str::contains(
            "filtered 1 violations via baseline",
        ))
        // ...and, since #1465, says so: the entry now records a 7 the
        // tree no longer produces, which is two points of suppression
        // nobody chose. Warning only — the gate stays green.
        .stderr(predicate::str::contains(
            "1 baseline entry improved past the recorded value",
        ))
        .stderr(predicate::str::contains("cyclomatic 7 \u{2192} 5"));
}

#[test]
fn unchanged_function_at_its_recorded_value_warns_nothing() {
    // The silence half of #1465, and the boundary the warning must not
    // cross: re-running against a baseline written from the same tree
    // finds every entry exactly on its record, so a freshly written
    // baseline must not warn about itself.
    let dir = TempDir::new().unwrap();
    let src_path = write_file(&dir, "branchy.rs", WORSER_RUST);
    let baseline = dir.path().join("baseline.toml");

    cli(dir.path())
        .args([
            "check",
            "--paths",
            src_path.to_str().unwrap(),
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    cli(dir.path())
        .args([
            "check",
            "--paths",
            src_path.to_str().unwrap(),
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success()
        // The offender is still covered and filtered — so the run did
        // reach the tally rather than producing no violation at all.
        .stderr(predicate::str::contains(
            "filtered 1 violations via baseline",
        ))
        .stderr(predicate::str::contains("improved past the recorded value").not());
}

// -- Identity & line drift ------------------------------------------------

#[test]
fn moved_function_still_covered_after_line_drift() {
    // Issue #377 acceptance: editing code above a function shifts its
    // start_line, but the baseline keys on the qualified symbol, so the
    // unchanged function stays covered rather than re-surfacing as a
    // `[new]` offender. (Pre-#377 this exact scenario re-keyed as new.)
    let dir = TempDir::new().unwrap();
    let src_path = write_file(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline = dir.path().join("baseline.toml");

    cli(dir.path())
        .args([
            "check",
            "--paths",
            src_path.to_str().unwrap(),
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Prepend a doc comment + blank lines, shifting `classify`'s
    // start_line down. The qualified symbol is unchanged, so the entry
    // still matches and the gate passes clean.
    let shifted = format!("/// Doc comment.\n///\n///\n{BRANCHY_RUST}");
    fs::write(&src_path, shifted).unwrap();

    cli(dir.path())
        .args([
            "check",
            "--paths",
            src_path.to_str().unwrap(),
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("[new]").not())
        // Prove the violation was actually classified and filtered —
        // a clean exit alone could mask a parse that found nothing.
        .stderr(predicate::str::contains("filtered 1 violations"));
}

/// Two methods named `is_valid` on different `impl` blocks. Issue #377
/// acceptance: each must match its own baseline entry independently —
/// here via distinct qualified symbols (`Foo::is_valid` vs
/// `Bar::is_valid`), which is also what the offender line now prints.
const TWO_IMPLS_RUST: &str = r"
pub struct Foo;
pub struct Bar;
impl Foo {
    pub fn is_valid(n: i32) -> bool {
        if n < 0 { false } else if n == 0 { false } else if n < 10 { true } else { true }
    }
}
impl Bar {
    pub fn is_valid(n: i32) -> bool {
        if n < 0 { false } else if n == 0 { false } else if n < 10 { true } else { true }
    }
}
";

#[test]
fn same_named_methods_on_different_impls_match_independently() {
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "impls.rs", TWO_IMPLS_RUST);
    let baseline = dir.path().join("baseline.toml");

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success()
        // Both methods are recorded under their container-qualified
        // symbols, so the baseline carries two distinct entries.
        .stderr(predicate::str::contains("wrote 2 baseline entries"));

    let content = fs::read_to_string(&baseline).unwrap();
    assert!(
        content.contains(r#"qualified = "Foo::is_valid""#),
        "{content}"
    );
    assert!(
        content.contains(r#"qualified = "Bar::is_valid""#),
        "{content}"
    );

    // Re-check unchanged: both are covered, gate passes clean.
    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();
}

/// Two C++ overloads of `f` — same (unqualified) top-level symbol —
/// produce an ambiguous baseline group that the `start_line` tolerance
/// must disambiguate. Doubles as non-Rust per-language coverage of the
/// qualified-symbol walk.
const CPP_OVERLOADS: &str = "
int f(int x) {
    if (x > 0) { return 1; }
    else if (x > 10) { return 2; }
    else if (x > 20) { return 3; }
    else { return 4; }
}

int f(double x) {
    if (x > 0) { return 1; }
    else if (x > 10) { return 2; }
    else if (x > 20) { return 3; }
    else { return 4; }
}
";

#[test]
fn baseline_line_tolerance_flag_is_honored_end_to_end() {
    let dir = TempDir::new().unwrap();
    let src_path = write_file(&dir, "overloads.cpp", CPP_OVERLOADS);
    let baseline = dir.path().join("baseline.toml");

    // Both overloads share the qualified symbol `f`, so the baseline
    // records two entries under one key — the ambiguous case.
    // `--no-config` keeps the test hermetic: the runner's cwd is
    // inside the repo, whose root `bca.toml` would otherwise merge its
    // own thresholds / exclude_from into this fully-explicit run.
    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            src_path.to_str().unwrap(),
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("wrote 2 baseline entries"));

    // Shift both functions down a few lines.
    fs::write(
        &src_path,
        format!("// pad\n// pad\n// pad\n{CPP_OVERLOADS}"),
    )
    .unwrap();

    // Default tolerance (50) absorbs the small drift → both covered.
    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            src_path.to_str().unwrap(),
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("filtered 2 violations"));

    // Tolerance 0 rejects any drift on the ambiguous symbol → both
    // surface as new. Proves the flag value reaches the matcher.
    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            src_path.to_str().unwrap(),
            "--threshold",
            "cyclomatic=1",
            "--baseline-line-tolerance",
            "0",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("[new]"));
}

#[test]
fn fuzzy_match_covers_renamed_function() {
    // Issue #377 rule 3: a function renamed but otherwise unchanged
    // keeps its baseline coverage when `--baseline-fuzzy-match` is on,
    // because the body hash still matches. The hash is populated at
    // write time only when the flag is set, so both runs pass it.
    let dir = TempDir::new().unwrap();
    let src_path = write_file(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline = dir.path().join("baseline.toml");

    cli(dir.path())
        .args([
            "check",
            "--paths",
            src_path.to_str().unwrap(),
            "--threshold",
            "cyclomatic=1",
            "--baseline-fuzzy-match",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    // The written baseline carries a body_hash for fuzzy matching.
    let content = fs::read_to_string(&baseline).unwrap();
    assert!(content.contains("body_hash = "), "{content}");

    // Rename `classify` -> `categorize`; body is byte-identical.
    let renamed = BRANCHY_RUST.replace("fn classify", "fn categorize");
    fs::write(&src_path, renamed).unwrap();

    // Without fuzzy: the qualified symbol changed, so it is a new
    // offender and the gate fails.
    cli(dir.path())
        .args([
            "check",
            "--paths",
            src_path.to_str().unwrap(),
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("[new]"));

    // With fuzzy: the body hash matches, so it stays covered.
    cli(dir.path())
        .args([
            "check",
            "--paths",
            src_path.to_str().unwrap(),
            "--threshold",
            "cyclomatic=1",
            "--baseline-fuzzy-match",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("filtered 1 violations"));
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("wrote 0 baseline entries"));

    let content = fs::read_to_string(&baseline).unwrap();
    assert!(content.contains("version = 6"));
    assert!(content.contains("tier = \"hard\""));
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            src_path.to_str().unwrap(),
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    fs::write(&src_path, WORSER_RUST).unwrap();

    cli(dir.path())
        .args([
            "check",
            "--paths",
            src_path.to_str().unwrap(),
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
            "--no-fail",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("classify"));
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
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
        .stdout(predicate::str::contains("classify"))
        .stdout(predicate::str::contains("cyclomatic = 5"));
}

// -- Determinism & UX -----------------------------------------------------

#[test]
fn write_baseline_byte_equal_across_two_runs() {
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline_a = dir.path().join("a.toml");
    let baseline_b = dir.path().join("b.toml");

    for out in [&baseline_a, &baseline_b] {
        cli(dir.path())
            .args([
                "check",
                "--paths",
                &src,
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
            "--threshold",
            "loc.sloc=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    let content = fs::read_to_string(&baseline).unwrap();
    assert!(
        content.contains(r#"qualified = "<file>""#),
        "expected `<file>` sentinel entry in baseline; got:\n{content}",
    );
    assert!(content.contains(r#"metric = "loc.sloc""#));

    // Re-run with the same threshold and the freshly-written
    // baseline: the file-level violation must be filtered like any
    // other entry.
    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            empty.to_str().unwrap(),
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
            "--threshold",
            "cyclomatic=10",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("wrote 0 baseline entries"));

    let content = fs::read_to_string(&baseline).unwrap();
    assert!(content.contains("version = 6"));
    // No `--tier=soft`, so the write is stamped at the hard tier (#486).
    assert!(content.contains("tier = \"hard\""));
    assert!(!content.contains("headroom"));
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            src_path.to_str().unwrap(),
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    fs::write(&src_path, WORSER_RUST).unwrap();

    cli(dir.path())
        .args([
            "check",
            "--paths",
            src_path.to_str().unwrap(),
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        // (7-5)/5*100 = 40, rounded → +40%
        .stdout(predicate::str::contains("[regr +40%] "));
}

#[test]
fn new_violation_carries_new_tag() {
    // Baseline omits a new file; its violation must be tagged `[new]`.
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline = dir.path().join("baseline.toml");

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    let new_src = write_fixture(&dir, "extra.rs", BRANCHY_RUST);

    cli(dir.path())
        .args([
            "check",
            "--paths",
            new_src.as_str(),
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("[new] "));
}

#[test]
fn no_baseline_emits_unprefixed_lines() {
    // Backward-compatibility invariant: without --baseline the
    // offender line format is byte-identical to today. No `[new]` /
    // `[regr` prefix may appear on the violation line.
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);

    // `--no-config` keeps the test hermetic: the runner's cwd is inside
    // the repo, whose root `bca.toml` declares a `baseline` key that
    // would otherwise tag this run's violations with `[new]`.
    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            &src,
            "--threshold",
            "cyclomatic=1",
        ])
        .assert()
        .code(2)
        // The violation line must contain the function name and metric,
        // and must NOT carry a bracket tag prefix.
        .stdout(predicate::str::contains("classify: cyclomatic ="))
        .stdout(predicate::str::contains("[new]").not())
        .stdout(predicate::str::contains("[regr").not());
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
    cli(dir.path())
        .args([
            "check",
            "--paths",
            ".",
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline_rel.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Form B: --paths /abs/tempdir (absolute).
    cli(dir.path())
        .args([
            "check",
            "--paths",
            &abs_paths,
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
    cli(dir.path())
        .args([
            "check",
            "--paths",
            ".",
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
    cli(dir.path())
        .args([
            "check",
            "--paths",
            &abs_paths,
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

    cli(dir.path())
        .args([
            "check",
            "--paths",
            ".",
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

// -- Provenance directional warning (issue #486) -------------------------

#[test]
fn soft_write_baseline_stamps_tier_and_headroom() {
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline = dir.path().join("baseline.toml");

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
            "--tier=soft=0.95",
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    let content = fs::read_to_string(&baseline).unwrap();
    assert!(content.contains("[provenance]"), "content:\n{content}");
    assert!(content.contains("tier = \"soft\""), "content:\n{content}");
    assert!(content.contains("headroom = 0.95"), "content:\n{content}");
}

#[test]
fn hard_check_reading_soft_baseline_is_silent() {
    // The repo's intended setup: a soft-0.95 baseline read by the hard
    // gate. The hard gate sees a superset of its offenders, so it must
    // emit NO provenance warning.
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline = dir.path().join("baseline.toml");

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
            "--tier=soft=0.95",
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("stricter than the baseline").not());
}

#[test]
fn stricter_check_reading_looser_baseline_warns() {
    // Soft check at 0.90 (stricter) reading a baseline written at 0.95
    // (looser): the baseline may under-cover, so the gate warns.
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "branchy.rs", BRANCHY_RUST);
    let baseline = dir.path().join("baseline.toml");

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
            "--tier=soft=0.95",
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
            "--tier=soft=0.90",
            "--threshold",
            "cyclomatic=1",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("stricter than the baseline"));
}

// -- MI documentation mode provenance (#1597) --------------------------

/// A documented Rust function whose `mi.visual_studio` is well under 100
/// in either mode, so an `mi.visual_studio=100` threshold flags it
/// whichever formula ran and the baseline carries an `mi.*` entry.
const DOCUMENTED_RUST: &str = r"
/// Clamp `n` into `[lo, hi]`.
///
/// The bounds are inclusive; `lo > hi` is not checked and returns
/// `hi` for every input, which callers may rely on.
pub fn clamp(n: i32, lo: i32, hi: i32) -> i32 {
    if n < lo {
        lo
    } else if n > hi {
        hi
    } else {
        n
    }
}
";

fn write_mi_baseline(dir: &TempDir, src: &str, baseline: &Path, mode: &str) {
    cli(dir.path())
        .args([
            "check",
            "--paths",
            src,
            mode,
            "--threshold",
            "mi.visual_studio=100",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn write_baseline_records_the_mi_documentation_mode() {
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "doc.rs", DOCUMENTED_RUST);
    let baseline = dir.path().join("baseline.toml");

    write_mi_baseline(&dir, &src, &baseline, "--mi-charges-docs=false");
    let content = fs::read_to_string(&baseline).unwrap();
    assert!(
        content.contains("mi_charges_docs = false"),
        "content:\n{content}"
    );

    write_mi_baseline(&dir, &src, &baseline, "--mi-charges-docs");
    let content = fs::read_to_string(&baseline).unwrap();
    assert!(
        content.contains("mi_charges_docs = true"),
        "content:\n{content}"
    );
}

#[test]
fn reading_a_baseline_under_the_other_mi_mode_warns_both_ways() {
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "doc.rs", DOCUMENTED_RUST);
    let baseline = dir.path().join("baseline.toml");

    for (written, read, hint) in [
        (
            "--mi-charges-docs=false",
            "--mi-charges-docs",
            "--mi-charges-docs=false",
        ),
        (
            "--mi-charges-docs",
            "--mi-charges-docs=false",
            "--mi-charges-docs=true",
        ),
    ] {
        write_mi_baseline(&dir, &src, &baseline, written);
        cli(dir.path())
            .args([
                "check",
                "--paths",
                &src,
                read,
                "--threshold",
                "mi.visual_studio=100",
                "--baseline",
                baseline.to_str().unwrap(),
            ])
            .assert()
            .stderr(predicate::str::contains("different formula"))
            .stderr(predicate::str::contains(hint));
    }
}

#[test]
fn reading_a_baseline_under_the_same_mi_mode_is_silent() {
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "doc.rs", DOCUMENTED_RUST);
    let baseline = dir.path().join("baseline.toml");

    write_mi_baseline(&dir, &src, &baseline, "--mi-charges-docs");
    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
            "--mi-charges-docs",
            "--threshold",
            "mi.visual_studio=100",
            "--baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("different formula").not());
}

#[test]
fn a_baseline_without_an_mi_mode_reads_as_charging_docs() {
    // A file written before the key existed carries no mode, and every
    // bca that wrote one charged documentation: a run under the new
    // default warns that the baseline predates the stamp, and a run that
    // kept the old formula matches it.
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "doc.rs", DOCUMENTED_RUST);
    let baseline = dir.path().join("baseline.toml");
    write_mi_baseline(&dir, &src, &baseline, "--mi-charges-docs");
    let content = fs::read_to_string(&baseline).unwrap();
    let stripped = content
        .lines()
        .filter(|line| !line.starts_with("mi_charges_docs"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&baseline, stripped).unwrap();

    let run = |mode: &str| {
        cli(dir.path())
            .args([
                "check",
                "--paths",
                &src,
                mode,
                "--threshold",
                "mi.visual_studio=100",
                "--baseline",
                baseline.to_str().unwrap(),
            ])
            .assert()
    };
    run("--mi-charges-docs=false")
        .stderr(predicate::str::contains(
            "predates the mi_charges_docs provenance key",
        ))
        .stderr(predicate::str::contains("--mi-charges-docs=true"));
    run("--mi-charges-docs").stderr(predicate::str::contains("different formula").not());
}

#[test]
fn the_mi_mode_warning_needs_an_mi_value_read_against_the_baseline() {
    // The mode moves only `mi.*`, so a mismatch is worth a warning only
    // when an `mi.*` offender is classified against an `mi.*` entry: a
    // baseline holding none, or a run gating no `mi.*` metric, stays
    // silent although both record the other mode.
    let dir = TempDir::new().unwrap();
    let src = write_fixture(&dir, "doc.rs", DOCUMENTED_RUST);
    let cyclomatic_only = dir.path().join("cyclomatic.toml");
    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            cyclomatic_only.to_str().unwrap(),
        ])
        .assert()
        .success();
    let content = fs::read_to_string(&cyclomatic_only).unwrap();
    assert!(
        content.contains("mi_charges_docs = false") && !content.contains("metric = \"mi."),
        "content:\n{content}"
    );
    let with_mi = dir.path().join("with-mi.toml");
    write_mi_baseline(&dir, &src, &with_mi, "--mi-charges-docs=false");

    for baseline in [&cyclomatic_only, &with_mi] {
        cli(dir.path())
            .args([
                "check",
                "--paths",
                &src,
                "--mi-charges-docs",
                "--threshold",
                "cyclomatic=1",
                "--baseline",
                baseline.to_str().unwrap(),
            ])
            .assert()
            .stderr(predicate::str::contains("different formula").not());
    }
}

/// One Python function of `lines` ternary assignments. Each line adds a
/// decision and a dozen Halstead tokens, so `mi.original` and `mi.sei`
/// fall below zero far sooner than the plain arithmetic of the issue's
/// 3,000-line reproducer: at 200 lines they read about −15 and −77, at
/// 300 about −47 and −112 (#1611).
fn negative_mi_python(lines: usize) -> String {
    use std::fmt::Write as _;
    let mut src = String::from("def big(a, b, c):\n");
    for i in 0..lines {
        writeln!(src, "    x{i} = a * {i} + b if c > {i} else b - c * {i}").unwrap();
    }
    src.push_str("    return a\n");
    src
}

#[test]
fn negative_mi_values_are_baselined_and_ratchet() {
    // Before #1611 `--write-baseline` skipped every negative value with
    // a warning, so a large function's `mi.original` and `mi.sei`
    // reported `[new]` after every refresh and the gate could never be
    // quieted. Both must now be written, cover the unchanged function,
    // and still catch it getting worse.
    let dir = TempDir::new().unwrap();
    let src_path = write_file(&dir, "big.py", &negative_mi_python(200));
    let src = src_path.to_str().unwrap();
    let baseline = dir.path().join("baseline.toml");
    let gate = ["--threshold", "mi.original=20", "--threshold", "mi.sei=20"];

    cli(dir.path())
        .args(["check", "--paths", src])
        .args(gate)
        .args(["--write-baseline", baseline.to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains("wrote 2 baseline entries"))
        .stderr(predicate::str::contains("skipping").not());
    let content = fs::read_to_string(&baseline).unwrap();
    for metric in ["mi.original", "mi.sei"] {
        assert!(
            content.contains(&format!("metric = \"{metric}\"")),
            "{metric} missing:\n{content}"
        );
    }
    assert_eq!(
        content.matches("value = -").count(),
        2,
        "both values must be recorded negative:\n{content}"
    );

    cli(dir.path())
        .args(["check", "--paths", src])
        .args(gate)
        .args(["--baseline", baseline.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("[new]").not());

    // Growing the function drives both values further below their
    // negative records: a regression, tagged in MI points (#1621) rather
    // than the double-signed `+-N%` a signed percentage produced.
    fs::write(&src_path, negative_mi_python(300)).unwrap();
    let out = cli(dir.path())
        .args(["check", "--paths", src])
        .args(gate)
        .args(["--baseline", baseline.to_str().unwrap()])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).unwrap();
    let tags: Vec<(&str, &str)> = stdout
        .lines()
        .filter_map(|line| line.strip_prefix("[regr +"))
        .filter_map(|rest| rest.split_once(" pts] "))
        .collect();
    assert_eq!(tags.len(), 2, "two regressed rows expected:\n{stdout}");
    let mut metrics = Vec::new();
    for (pts, row) in tags {
        let well_formed = pts.split_once('.').is_some_and(|(whole, frac)| {
            !whole.is_empty()
                && whole.bytes().all(|b| b.is_ascii_digit())
                && frac.len() == 1
                && frac.bytes().all(|b| b.is_ascii_digit())
        });
        assert!(well_formed, "malformed regression tag `{pts}`:\n{stdout}");
        // 200 → 300 lines moves both values by roughly 30 points.
        let amount: f64 = pts.parse().unwrap();
        assert!(amount >= 10.0, "implausible drop `{pts}`:\n{stdout}");
        metrics.extend(
            ["mi.original", "mi.sei"]
                .into_iter()
                .filter(|m| row.contains(&format!(": {m} = "))),
        );
    }
    metrics.sort_unstable();
    assert_eq!(metrics, ["mi.original", "mi.sei"], "{stdout}");
}

#[test]
fn summaries_name_the_deepest_negative_mi_breach_as_worst() {
    // #1617: both functions' `mi.original` sit below zero (`big` ≈ −15,
    // `bigger` ≈ −47), where the old breach ratio saturated and tied. The
    // tie kept the larger raw value, the milder breach, so the footer, the
    // step-summary rollup and the top-offenders table all led with `big`.
    // One metric is gated so the result cannot rest on how two MI
    // formulas compare, only on which function falls further; `big` also
    // comes first by line, so no positional tie-break can stand in for
    // severity.
    let dir = TempDir::new().unwrap();
    let source = format!(
        "{}{}",
        negative_mi_python(200),
        negative_mi_python(300).replace("def big(", "def bigger(")
    );
    let src_path = write_file(&dir, "big.py", &source);
    let summary = dir.path().join("step-summary.md");
    let out = cli(dir.path())
        .args(["check", "--paths", src_path.to_str().unwrap()])
        .args(["--threshold", "mi.original=20"])
        .env("GITHUB_STEP_SUMMARY", &summary)
        .assert()
        .code(2);
    let stderr = std::str::from_utf8(&out.get_output().stderr).unwrap();
    let digest = fs::read_to_string(&summary).unwrap();

    assert!(
        digest.contains("### Top 10 offenders (by severity)"),
        "{digest}"
    );
    let top: Vec<&str> = digest
        .lines()
        .skip_while(|line| !line.starts_with("### Top"))
        .filter(|line| line.contains("| mi.original |"))
        .collect();
    assert_eq!(top.len(), 2, "two top-offender rows expected:\n{digest}");
    assert!(top[0].contains("| bigger |"), "deepest first:\n{digest}");
    assert!(top[1].contains("| big |"), "got:\n{digest}");
    // The Value column of the deepest row is what the footer and the
    // rollup must cite.
    let deepest = top[0].split('|').map(str::trim).nth(5).unwrap();
    let shallow = top[1].split('|').map(str::trim).nth(5).unwrap();
    assert!(
        deepest.parse::<f64>().unwrap() < shallow.parse::<f64>().unwrap(),
        "fixture must put `bigger` below `big`: {deepest} vs {shallow}"
    );
    assert!(
        stderr.contains(&format!(
            "2 violations (worst: mi.original = {deepest} vs limit"
        )),
        "footer must name the deeper breach {deepest}:\n{stderr}"
    );
    assert!(
        digest.contains(&format!("| 2 | mi.original | {deepest} |")),
        "rollup must name the deeper breach {deepest}:\n{digest}"
    );
}

#[test]
fn cli_and_code_climate_agree_on_cross_metric_severity() {
    // #1620 and #1621's decided ordering. The 200-line function has
    // cognitive 200 against 15 (13.3x) and mi.original ≈ −14.8 against a
    // floor of 20 — 34.8 points, about two Visual Studio bands
    // (1 + 34.8 / 17.1 ≈ 3.03). The CLI's footer and Code Climate read the
    // same severity key, so the CLI names cognitive worst, Code Climate
    // bands it `blocker` (> 4) and the MI `critical` (2–4). Before #1621
    // the CLI named the MI worst (an infinite ratio) and Code Climate
    // banded it `minor` (a non-positive value fell back to the floor).
    let dir = TempDir::new().unwrap();
    let src_path = write_file(&dir, "big.py", &negative_mi_python(200));
    let src = src_path.to_str().unwrap();
    let gate = [
        "--threshold",
        "mi.original=20",
        "--threshold",
        "cognitive=15",
    ];
    cli(dir.path())
        .args(["check", "--paths", src])
        .args(gate)
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "2 violations (worst: cognitive = 200 vs limit 15",
        ));

    let out = cli(dir.path())
        .args(["check", "--paths", src, "--format", "code-climate"])
        .args(gate)
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let issues: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let band = |metric: &str| {
        issues
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["check_name"] == format!("big-code-analysis/{metric}"))
            .map(|i| i["severity"].as_str().unwrap().to_owned())
    };
    assert_eq!(band("cognitive").as_deref(), Some("blocker"), "{issues}");
    assert_eq!(band("mi.original").as_deref(), Some("critical"), "{issues}");
}

#[test]
fn equidistant_mi_records_tie_toward_the_covering_one() {
    // #1616: two `mi.sei` records for one function, equally far from it,
    // one stricter than the live value and one more lenient. The
    // tie-break kept the higher record whatever the direction; for the
    // lower-is-worse `mi.*` family that is the stricter one, so the
    // function read `[regr …]` against a baseline that also held a record
    // covering it. The records are placed relative to a real write so they
    // bracket whatever the MI formula gives the fixture, and sit one line
    // either side of the function.
    let dir = TempDir::new().unwrap();
    let src_path = write_file(&dir, "big.py", &format!("\n\n{}", negative_mi_python(200)));
    let src = src_path.to_str().unwrap();
    let baseline = dir.path().join("baseline.toml");
    let gate = ["--threshold", "mi.sei=20"];

    cli(dir.path())
        .args(["check", "--paths", src])
        .args(gate)
        .args(["--write-baseline", baseline.to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains("wrote 1 baseline entries"));
    let written = fs::read_to_string(&baseline).unwrap();
    let live: f64 = written
        .lines()
        .find_map(|line| line.strip_prefix("value = "))
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("no recorded value in:\n{written}"));
    // Keep the written header so the version and provenance stay
    // whatever this bca writes; only the entry is replaced. The stricter
    // record comes first so a tie-break that kept the first of two equal
    // candidates would fail too.
    let (header, _) = written.split_once("[[entry]]").unwrap();
    let entry = |line: usize, value: f64| {
        format!(
            "[[entry]]\npath = \"big.py\"\nqualified = \"big\"\n\
             start_line = {line}\nmetric = \"mi.sei\"\nvalue = {value:?}\n\n"
        )
    };
    fs::write(
        &baseline,
        format!("{header}{}{}", entry(2, live + 0.5), entry(4, live - 0.5)),
    )
    .unwrap();

    cli(dir.path())
        .args(["check", "--paths", src])
        .args(gate)
        .args(["--baseline", baseline.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("[regr").not())
        .stderr(predicate::str::contains(
            "filtered 1 violations via baseline",
        ));
}

#[test]
fn a_negative_mi_floor_parses_and_gates() {
    // `mi.original` is unbounded below, so a floor under zero is a real
    // gate (#1621); before it, every negative limit was a parse error.
    // The 200-line function measures ≈ −15, above a floor of −50; the
    // 500-line one ≈ −104, below it.
    let dir = TempDir::new().unwrap();
    let mild = write_file(&dir, "mild.py", &negative_mi_python(200));
    let deep = write_file(&dir, "deep.py", &negative_mi_python(500));
    let gate = ["--threshold", "mi.original=-50"];
    cli(dir.path())
        .args(["check", "--paths", mild.to_str().unwrap()])
        .args(gate)
        .assert()
        .success()
        .stderr(predicate::str::contains("warning").not());
    cli(dir.path())
        .args(["check", "--paths", deep.to_str().unwrap()])
        .args(gate)
        .assert()
        .code(2)
        .stdout(predicate::str::is_match(r"big: mi\.original = -\d+\.\d+ \(limit -50\)").unwrap());
    // A count still rejects a negative limit, with the long-standing
    // message.
    cli(dir.path())
        .args(["check", "--paths", mild.to_str().unwrap()])
        .args(["--threshold", "cyclomatic=-1"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "limit for \"cyclomatic\" must be a finite non-negative number; got -1",
        ));
}

#[test]
fn an_mi_visual_studio_limit_that_cannot_fire_warns() {
    // `mi.visual_studio` is clamped to [0, 100], so a floor of 0 can
    // never be breached: the run passes on a function whose MI is 0. It
    // still parses (rejecting it is a CLI-grammar break reserved for
    // 3.0), but says so once, naming the metric that can gate there.
    let dir = TempDir::new().unwrap();
    let src = write_file(&dir, "big.py", &negative_mi_python(200));
    let out = cli(dir.path())
        .args(["check", "--paths", src.to_str().unwrap()])
        .args(["--threshold", "mi.visual_studio=0"])
        .assert()
        .success();
    let stderr = std::str::from_utf8(&out.get_output().stderr).unwrap();
    let line = "warning: limit mi.visual_studio=0 cannot fire: 0 is the metric's floor; \
                gate mi.original instead";
    assert_eq!(stderr.matches(line).count(), 1, "{stderr}");
}

#[test]
fn an_out_of_domain_mi_visual_studio_limit_warns_and_still_gates() {
    // 101 sits above every value the metric can take, so it flags every
    // function. Repeating the flag is last-wins, and a `--threshold`
    // overrides the config's limit; only the limit that reaches the gate
    // is reported, and only once.
    let dir = TempDir::new().unwrap();
    let src = write_file(&dir, "big.py", &negative_mi_python(200));
    let config = write_file(&dir, "t.toml", "[thresholds]\n\"mi.visual_studio\" = -1\n");
    let out = cli(dir.path())
        .args(["check", "--paths", src.to_str().unwrap()])
        .args(["--config", config.to_str().unwrap()])
        .args(["--threshold", "mi.visual_studio=0"])
        .args(["--threshold", "mi.visual_studio=101"])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("mi.visual_studio = 0 (limit 101)"));
    let stderr = std::str::from_utf8(&out.get_output().stderr).unwrap();
    let line = "warning: limit mi.visual_studio=101 is outside the metric's range [0, 100]; \
                it fires on every value";
    assert_eq!(stderr.matches(line).count(), 1, "{stderr}");
    assert!(
        !stderr.contains("mi.visual_studio=0 cannot fire"),
        "{stderr}"
    );
    assert!(!stderr.contains("mi.visual_studio=-1"), "{stderr}");
}

#[test]
fn a_soft_table_limit_warns_only_on_a_run_that_applies_it() {
    // A hard-tier run never reads `[thresholds.soft]`, so an absolute
    // soft limit of 101 is not a limit it gates on and must not be
    // reported; a soft-tier run applies it and warns.
    let dir = TempDir::new().unwrap();
    let src = write_file(&dir, "big.py", &negative_mi_python(200));
    let config = write_file(
        &dir,
        "t.toml",
        "[thresholds]\n\"mi.visual_studio\" = 20\n\
         [thresholds.soft]\n\"mi.visual_studio\" = 101\n",
    );
    let line = "warning: limit mi.visual_studio=101 is outside the metric's range [0, 100]; \
                it fires on every value";
    for (tier, warnings) in [("--tier=hard", 0), ("--tier=soft", 1)] {
        let out = cli(dir.path())
            .args(["check", "--paths", src.to_str().unwrap()])
            .args(["--config", config.to_str().unwrap(), tier])
            .assert()
            .code(2);
        let stderr = std::str::from_utf8(&out.get_output().stderr).unwrap();
        assert_eq!(stderr.matches(line).count(), warnings, "{tier}:\n{stderr}");
        assert!(
            !stderr.contains("mi.visual_studio=20 "),
            "{tier}:\n{stderr}"
        );
    }
}

#[test]
fn a_soft_tier_advisory_describes_the_raised_floor_not_the_configured_one() {
    // Under `--tier=soft` the gate runs on the floor raised by
    // `(1 - ratio) · unit`, so a configured `mi.visual_studio = 0`
    // gates at 0.5 and does fire. The hard tier applies the floor as
    // written and cannot fire, so only it warns.
    let dir = TempDir::new().unwrap();
    let src = write_file(&dir, "big.py", &negative_mi_python(3000));
    let config = write_file(&dir, "t.toml", "[thresholds]\n\"mi.visual_studio\" = 0\n");
    let line = "warning: limit mi.visual_studio=0 cannot fire";
    for (tier, warnings, code) in [("--tier=hard", 1, 0), ("--tier=soft", 0, 2)] {
        let out = cli(dir.path())
            .args(["check", "--paths", src.to_str().unwrap()])
            .args(["--config", config.to_str().unwrap(), tier])
            .assert()
            .code(code);
        let stderr = std::str::from_utf8(&out.get_output().stderr).unwrap();
        assert_eq!(stderr.matches(line).count(), warnings, "{tier}:\n{stderr}");
    }
}
