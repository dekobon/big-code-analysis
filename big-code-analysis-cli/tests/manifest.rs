//! Integration tests for `bca.toml` manifest discovery and precedence
//! (issue #374).
//!
//! Each test drives the real binary against a temp directory containing
//! a `bca.toml` (and a `.git` marker so manifest discovery stops at the
//! fixture root rather than climbing into the host filesystem). The
//! fixtures are tiny inline Rust so they don't depend on any submodule.

use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

mod common;

fn cli() -> Command {
    common::bca_command()
}

/// `classify` has cyclomatic == 4 (three `else if`/`if` decision points
/// plus one). Used by tests that need a guaranteed offender at a tight
/// limit and a clean run at a loose one.
const BRANCHY_RUST: &str = r#"
pub fn classify(n: i32) -> &'static str {
    if n < 0 { "neg" } else if n == 0 { "zero" } else if n < 10 { "small" } else { "big" }
}
"#;

/// Create a fixture repo: a temp dir with a `.git` marker (so discovery
/// halts here), a `bca.toml` with the given body, and `branchy.rs`.
fn fixture(manifest: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join("bca.toml"), manifest).unwrap();
    fs::write(dir.path().join("branchy.rs"), BRANCHY_RUST).unwrap();
    dir
}

/// Acceptance criterion 1: `bca check` with no flags, run in a directory
/// containing `bca.toml`, picks up `paths` and `[thresholds]` and gates
/// accordingly. A tight `cyclomatic = 1` fires on `classify`.
#[test]
fn bare_check_uses_manifest_paths_and_thresholds() {
    let dir = fixture("paths = [\".\"]\n\n[thresholds]\ncyclomatic = 1\n");

    cli()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("classify"))
        .stderr(predicate::str::contains("cyclomatic"))
        .stderr(predicate::str::contains("(limit 1)"));
}

/// A loose manifest limit produces a clean run — proving the manifest
/// threshold (not some default) is what's being applied.
#[test]
fn bare_check_clean_under_loose_manifest_threshold() {
    let dir = fixture("paths = [\".\"]\n\n[thresholds]\ncyclomatic = 100\n");

    cli()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

/// Acceptance criterion 3: `--no-config` skips manifest discovery
/// entirely, so a directory whose only thresholds live in `bca.toml`
/// errors out with "no thresholds configured" (exit 1, not a vacuous
/// clean pass).
#[test]
fn no_config_skips_discovery() {
    let dir = fixture("paths = [\".\"]\n\n[thresholds]\ncyclomatic = 1\n");

    cli()
        .current_dir(dir.path())
        .args(["check", "--no-config"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no thresholds configured"));
}

/// Acceptance criterion 2: an explicit `--threshold` overrides the
/// manifest value. Manifest says 100 (clean), CLI says 1 (offender);
/// the CLI must win.
#[test]
fn cli_threshold_overrides_manifest() {
    let dir = fixture("paths = [\".\"]\n\n[thresholds]\ncyclomatic = 100\n");

    cli()
        .current_dir(dir.path())
        .args(["check", "--threshold", "cyclomatic=1"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("(limit 1)"));
}

/// `--paths` overrides the manifest `paths`. Pointed at an empty
/// subdirectory, the run finds no files and errors (exit 1) instead of
/// analysing the manifest's `.`.
#[test]
fn cli_paths_overrides_manifest() {
    let dir = fixture("paths = [\".\"]\n\n[thresholds]\ncyclomatic = 1\n");
    let empty = dir.path().join("empty");
    fs::create_dir(&empty).unwrap();

    cli()
        .current_dir(dir.path())
        .args(["check", "--paths"])
        .arg(&empty)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no input files matched"));
}

/// `--config <file>` merges on top of the manifest `[thresholds]`,
/// winning on key collision. Manifest says 100 (clean); the config file
/// says 1 (offender); the config value must win.
#[test]
fn config_file_merges_over_manifest_thresholds() {
    let dir = fixture("paths = [\".\"]\n\n[thresholds]\ncyclomatic = 100\n");
    let cfg = dir.path().join("override.toml");
    fs::write(&cfg, "[thresholds]\ncyclomatic = 1\n").unwrap();

    cli()
        .current_dir(dir.path())
        .args(["check", "--config"])
        .arg(&cfg)
        .assert()
        .code(2)
        .stderr(predicate::str::contains("(limit 1)"));
}

/// The manifest `headroom` key scales the manifest `[thresholds]` at the
/// soft tier, exactly like `--headroom`. 100 × 0.01 = 1.0, so `classify`
/// (cyclomatic 4) trips the gate. `--headroom` is a soft-tier dial, so
/// `--tier=soft` is required for it to take effect.
#[test]
fn manifest_headroom_scales_thresholds_at_soft_tier() {
    let dir = fixture("paths = [\".\"]\nheadroom = 0.01\n\n[thresholds]\ncyclomatic = 100\n");

    cli()
        .current_dir(dir.path())
        .args(["check", "--tier", "soft"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cyclomatic"))
        // Pin the *scaled* limit: 100 × 0.01 = 1. A bare "cyclomatic"
        // match would pass even if the scale were wrong (e.g. headroom
        // ignored and the limit left at 100, or applied twice).
        .stderr(predicate::str::contains("(limit 1)"));
}

/// The manifest `baseline` key is honored: a baseline that already
/// records `classify`'s violation suppresses it, yielding a clean run.
#[test]
fn manifest_baseline_is_honored() {
    let dir = fixture(
        "paths = [\".\"]\nbaseline = \".bca-baseline.toml\"\n\n[thresholds]\ncyclomatic = 1\n",
    );

    // Write the baseline at the same thresholds, then confirm the next
    // bare run is suppressed by it.
    cli()
        .current_dir(dir.path())
        .args(["check", "--write-baseline", ".bca-baseline.toml"])
        .assert()
        .success();
    assert!(
        dir.path().join(".bca-baseline.toml").is_file(),
        "write-baseline should have created the file"
    );

    cli()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .success()
        .stderr(predicate::str::contains("filtered"));
}

/// The manifest `baseline_fuzzy_match` key enables the body-hash
/// fallback (issue #377): after renaming `classify` to `categorize`
/// with the body unchanged, the manifest's fuzzy flag keeps the entry
/// covered on a bare `bca check` with no extra CLI flags.
#[test]
fn manifest_baseline_fuzzy_match_is_honored() {
    let dir = fixture(
        "paths = [\".\"]\nbaseline = \".bca-baseline.toml\"\nbaseline_fuzzy_match = true\n\n[thresholds]\ncyclomatic = 1\n",
    );

    // Seed a fuzzy baseline (body_hash populated) for `classify`.
    cli()
        .current_dir(dir.path())
        .args([
            "check",
            "--baseline-fuzzy-match",
            "--write-baseline",
            ".bca-baseline.toml",
        ])
        .assert()
        .success();

    // Rename the function; the body is byte-identical.
    fs::write(
        dir.path().join("branchy.rs"),
        BRANCHY_RUST.replace("fn classify", "fn categorize"),
    )
    .unwrap();

    // A bare run picks up both `baseline` and `baseline_fuzzy_match`
    // from the manifest, so the body hash still covers the renamed fn.
    cli()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .success()
        .stderr(predicate::str::contains("[new]").not())
        // Confirm the renamed function was actually covered via the
        // fuzzy fallback, not silently dropped by an empty parse.
        .stderr(predicate::str::contains("filtered 1 violations"));
}

/// The manifest `baseline_line_tolerance` key reaches the matcher
/// (issue #377): two C++ overloads of `f` form an ambiguous symbol
/// group, and a manifest tolerance of 0 rejects any line drift, so a
/// bare `bca check` surfaces them as new after a shift.
#[test]
fn manifest_baseline_line_tolerance_is_honored() {
    const CPP: &str = "
int f(int x) {
    if (x > 0) { return 1; } else if (x > 9) { return 2; } else { return 3; }
}
int f(double x) {
    if (x > 0) { return 1; } else if (x > 9) { return 2; } else { return 3; }
}
";
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(
        dir.path().join("bca.toml"),
        "paths = [\".\"]\nbaseline = \".bca-baseline.toml\"\nbaseline_line_tolerance = 0\n\n[thresholds]\ncyclomatic = 1\n",
    )
    .unwrap();
    let src = dir.path().join("overloads.cpp");
    fs::write(&src, CPP).unwrap();

    // The key must not be rejected as unknown.
    cli()
        .current_dir(dir.path())
        .args(["check", "--write-baseline", ".bca-baseline.toml"])
        .assert()
        .success()
        .stderr(predicate::str::contains("unrecognized key").not())
        .stderr(predicate::str::contains("wrote 2 baseline entries"));

    // Shift both functions down; with manifest tolerance 0 the ambiguous
    // entries no longer match and resurface as new.
    fs::write(&src, format!("// pad\n// pad\n{CPP}")).unwrap();
    cli()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("[new]"));
}

/// Manifest discovery climbs from the working directory to the repo
/// root (the dir holding `.git`). Running from a nested subdirectory
/// must still find the root `bca.toml`, and its relative `paths`
/// resolve against the manifest's own directory.
#[test]
fn discovery_climbs_from_subdirectory() {
    let dir = fixture("paths = [\".\"]\n\n[thresholds]\ncyclomatic = 1\n");
    let sub = dir.path().join("nested").join("deeper");
    fs::create_dir_all(&sub).unwrap();

    cli()
        .current_dir(&sub)
        .arg("check")
        .assert()
        .code(2)
        // `paths = ["."]` resolved against the manifest dir, so the
        // root `branchy.rs` is analysed even though cwd is two levels down.
        .stderr(predicate::str::contains("classify"));
}

/// Unrecognized top-level keys (forthcoming features such as
/// `exit_codes`, #385) are ignored with a one-line warning rather than
/// rejected, so projects can pre-adopt the schema.
#[test]
fn unknown_top_level_key_warns_but_runs() {
    let dir = fixture(
        "paths = [\".\"]\n\n\
         [thresholds]\ncyclomatic = 100\n\n\
         [exit_codes]\nviolations = 3\n",
    );

    cli()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "ignoring unrecognized key `exit_codes`",
        ));
}

/// A manifest `[thresholds.soft]` sub-table (#375) is ignored at the
/// default hard tier — only the hard `[thresholds]` scalars gate — but
/// is honored under `--tier=soft`.
#[test]
fn manifest_soft_threshold_subtable_applies_only_at_soft_tier() {
    let dir = fixture(
        "paths = [\".\"]\n\n\
         [thresholds]\ncyclomatic = 100\n\n\
         [thresholds.soft]\ncyclomatic = 1\n",
    );

    // Hard tier (default): the soft override is ignored; limit 100 is
    // clean for `classify` (cyclomatic 5).
    cli()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .success();

    // Soft tier: the per-metric override drops the limit to 1, so
    // `classify` trips.
    cli()
        .current_dir(dir.path())
        .args(["check", "--tier", "soft"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("(limit 1)"));
}

/// `bca init` must NOT consume an existing manifest: it scaffolds
/// config, so merging repo-level manifest globals into its
/// baseline-generation walk would pin the wrong tree. A manifest with
/// an unknown key is the tell — had `init` discovered it, the
/// "ignoring unrecognized key" warning would fire. It must stay silent.
#[test]
fn init_ignores_existing_manifest() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    // The `[check]` table is an unrecognized key today; discovery would
    // warn about it, so its absence proves init skipped discovery.
    fs::write(
        dir.path().join("bca.toml"),
        "paths = [\".\"]\n\n[check]\nexclude = [\"x\"]\n",
    )
    .unwrap();
    let target = dir.path().join("proj");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("x.rs"), "pub fn f() {}\n").unwrap();

    cli()
        .current_dir(dir.path())
        .args(["init", "--dir"])
        .arg(&target)
        .assert()
        .success()
        .stderr(predicate::str::contains("ignoring unrecognized key").not());
}

/// A malformed `bca.toml` is a hard error (exit 1), never silently
/// ignored — a typo in config must not quietly disable the gate.
#[test]
fn malformed_manifest_is_a_hard_error() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join("bca.toml"), "paths = [unterminated\n").unwrap();

    cli()
        .current_dir(dir.path())
        .args(["check", "--threshold", "cyclomatic=1"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("parse bca.toml"));
}

/// An out-of-range `headroom` in the manifest is a hard error (exit 1)
/// with a `bca.toml`-attributed message — not silently accepted, and
/// not borrowing the `--headroom` flag's wording.
#[test]
fn manifest_headroom_out_of_range_is_rejected() {
    let dir = fixture("paths = [\".\"]\nheadroom = 1.5\n\n[thresholds]\ncyclomatic = 1\n");

    cli()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "bca.toml: headroom must be in (0, 1]",
        ));
}

/// An explicit `--num-jobs` overrides the manifest `num_jobs`. The
/// manifest here carries an invalid `0`; if the CLI flag were not
/// detected as overriding, that `0` would be read and rejected
/// (exit 1). A clean run proves `-j 1` won — exercising the
/// value-source detection that distinguishes a CLI flag from the
/// `auto` default (a `global` arg supplied after the subcommand
/// surfaces only in the subcommand's matches).
#[test]
fn cli_num_jobs_overrides_manifest() {
    let dir = fixture("paths = [\".\"]\nnum_jobs = 0\n\n[thresholds]\ncyclomatic = 100\n");

    cli()
        .current_dir(dir.path())
        .args(["check", "-j", "1"])
        .assert()
        .success();
}

/// An out-of-range `num_jobs` in the manifest is a hard error with a
/// clear message, reusing the `--num-jobs` validator's diagnostics.
#[test]
fn manifest_num_jobs_zero_is_rejected() {
    let dir = fixture("paths = [\".\"]\nnum_jobs = 0\n\n[thresholds]\ncyclomatic = 1\n");

    cli()
        .current_dir(dir.path())
        .arg("check")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("num_jobs"));
}
