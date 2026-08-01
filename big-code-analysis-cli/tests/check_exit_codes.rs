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
        .args(["check", "--paths", &src, "--threshold", "cyclomatic=1"])
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

    fs::write(&src_path, branchy_source(6)).unwrap(); // worsen to cyclomatic 7

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
        .code(2);
}

#[test]
fn strict_clean_exits_zero() {
    let dir = TempDir::new().unwrap();
    let src = write_branchy(&dir, 5);

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
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
            "check",
            "--paths",
            &src,
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
            "check",
            "--paths",
            &src,
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

    // Worsen the same function: cyclomatic 5 -> 7, a regression of the
    // baselined offender.
    fs::write(&src_path, branchy_source(6)).unwrap();

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
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
            "check",
            "--paths",
            dir.path().to_str().unwrap(),
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
            "check",
            "--paths",
            dir.path().to_str().unwrap(),
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
            "check",
            "--paths",
            &src,
            "--config",
            config.to_str().unwrap(),
            "--tier=soft=0.5", // soft limit 5; value 12 breaches the hard 10
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
            "check",
            "--paths",
            &src,
            "--config",
            config.to_str().unwrap(),
            "--tier=soft=0.5",
            "--strict-exit-codes",
        ])
        .assert()
        .code(2);
}

/// A `[thresholds.soft]` absolute limit with no `[thresholds]`
/// counterpart gives the metric a soft band and no hard ceiling, so no
/// value can escalate it to a hard breach however far it overshoots.
///
/// Pinned because the ceiling moved from a lookaside map onto the
/// offender record in #1141: "the metric is absent from the hard table"
/// became "the offender carries no ceiling", and fabricating one from
/// the soft limit instead would silently turn every such offender into
/// exit 5. That substitution failed no test before this one.
#[test]
fn strict_soft_only_metric_has_no_hard_ceiling_to_breach() {
    let dir = TempDir::new().unwrap();
    let src = write_branchy(&dir, 12);
    let config = dir.path().join("thresholds.toml");
    // `cognitive` is gated only at the soft tier; `cyclomatic` supplies
    // the hard table so the run has a hard tier at all.
    fs::write(
        &config,
        "[thresholds]\ncyclomatic = 100\n[thresholds.soft]\ncognitive = 2\n",
    )
    .unwrap();

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
            "--config",
            config.to_str().unwrap(),
            "--tier=soft",
            "--exit-codes=tiered",
        ])
        .assert()
        .code(2);
}

/// A limit written with the bare `diff --metric` alias (`sloc` for
/// `loc.sloc`) still escalates to a hard breach under the soft tier.
///
/// Until #1141 the escalation compared the offender's *canonical* metric
/// name against a map keyed by the spelling the user wrote, so
/// `[thresholds] sloc = N` never matched and no value, however large,
/// could reach exit 5. Carrying the ceiling on the offender resolves
/// both sides from the same key and fixes it.
#[test]
fn strict_alias_spelled_limit_still_escalates_to_hard_breach() {
    let dir = TempDir::new().unwrap();
    let src = write_branchy(&dir, 3);
    let config = dir.path().join("thresholds.toml");
    // `sloc = 2` against a file well over it: the soft band is 1, the
    // hard ceiling 2, and the file breaches both.
    fs::write(&config, "[thresholds]\nsloc = 2\n").unwrap();

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &src,
            "--config",
            config.to_str().unwrap(),
            "--tier=soft=0.5",
            "--exit-codes=tiered",
        ])
        .assert()
        .code(5);
}

// -- Workspace-wide convention (#561) -------------------------------------
//
// The 0/1 split is documented as a cross-subcommand contract (top-level
// `bca --help` "Exit codes" section + book `commands/README.md`): 0 on
// success, 1 on a tool error, and 1 is *never* a metric signal — codes
// 2-5 are reserved for the `check` gate above. These two tests pin the
// non-`check` side of that contract so a future regression that, say,
// made `metrics` exit non-zero on success or collapsed a tool error to
// 0 would fail here.

#[test]
fn metrics_clean_run_exits_zero() {
    let dir = TempDir::new().unwrap();
    let src = write_branchy(&dir, 5);

    cli(dir.path())
        .args(["metrics", "--paths", &src])
        .assert()
        .code(0);
}

#[test]
fn tool_error_exits_one_not_metric_signal() {
    // An invalid include glob is a tool error on a non-`check`
    // subcommand: it must take the `die` path (exit 1), never 0 and
    // never the 2-5 band the `check` gate owns.
    let (dir, mut cmd) = common::cli_hermetic();

    cmd.args([
        "metrics",
        "-I",
        "[",
        "--paths",
        dir.path().to_str().unwrap(),
    ])
    .assert()
    .code(1)
    .stderr(predicate::str::contains("invalid glob pattern"));
}

// -- clap usage / argv errors exit 1, not the gate band (#594) ------------
//
// clap's built-in `Error::exit` terminates usage errors with exit 2,
// which collides with the metric-gate band (`check` thresholds / `vcs
// jit --fail-over`). #594 remaps every argv/usage/value-parse failure to
// exit 1 (tool error) while preserving exit 0 for `--help` / `--version`.
// These tests pin both halves so the gate band stays free of typo'd
// flags and malformed `--threshold` values.

#[test]
fn unknown_flag_exits_one_with_usage() {
    let (dir, mut cmd) = common::cli_hermetic();
    let _ = &dir;

    cmd.arg("--bogus-flag")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Usage"));
}

#[test]
fn unknown_subcommand_exits_one() {
    let (dir, mut cmd) = common::cli_hermetic();
    let _ = &dir;

    // Assert the clap usage message too, so the exit 1 is provably the
    // usage-error remap and not a coincidental tool-error exit from some
    // unrelated path.
    cmd.arg("bogus-subcommand")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

#[test]
fn malformed_threshold_value_exits_one_not_gate() {
    // A `value_parser`-rejected `--threshold` is the exact "bad
    // threshold spec" the help text calls a tool error. It must exit 1,
    // never the gate's 2.
    let dir = TempDir::new().unwrap();
    let src = write_branchy(&dir, 5);

    // Assert the clap value-parse message too: exit 1 here must come from
    // the rejected `--threshold` value, not from a gate violation that
    // happens to share the tool-error code. `write_branchy(5)` is well
    // under any default limit, so a silently-ignored bad value would exit
    // 0 — pinning the stderr makes the wrong-reason path observable.
    cli(dir.path())
        .args(["check", "--paths", &src, "--threshold", "cyclomatic=abc"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("invalid value 'cyclomatic=abc'"));
}

#[test]
fn help_exits_zero() {
    let (dir, mut cmd) = common::cli_hermetic();
    let _ = &dir;

    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage"));
}

#[test]
fn version_exits_zero() {
    let (dir, mut cmd) = common::cli_hermetic();
    let _ = &dir;

    // Assert the version banner on stdout so the exit 0 is provably the
    // `--version` short-circuit (clap prints `bca <version>`), not a bare
    // success from some other path.
    cmd.arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("bca "));
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
