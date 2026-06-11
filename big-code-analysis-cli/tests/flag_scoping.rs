#![allow(missing_docs)]
//! Regression tests for the 2.0 flag-scoping (#597) and positional-path
//! (#651) CLI restructure.
//!
//! #597 scoped every walk- / tuning- / preproc- / output-specific flag
//! off `global = true` into per-subcommand `#[command(flatten)]` groups,
//! so a flag passed to a subcommand that never consumed it is now a hard
//! clap usage error (exit 1) instead of a silent no-op. #651 added a
//! trailing `[PATHS]...` positional to the walking subcommands and moved
//! `find`/`count` node kinds onto a repeatable `-t`/`--type` flag.

use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;

mod common;

fn cli() -> Command {
    common::bca_command()
}

/// Write a trivial Rust source file into a fresh tempdir and return both
/// so the caller keeps the dir alive.
fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("a.rs");
    fs::write(&file, "fn main() { let _ = 1; }\n").expect("write fixture");
    (dir, file)
}

// ----- #597: inert flags are now hard usage errors -----

/// `list-metrics` walks nothing, so it must not advertise `--paths` in
/// its help. Before #597 every `global = true` flag leaked onto every
/// subcommand's help.
#[test]
fn list_metrics_help_does_not_list_paths() {
    cli()
        .args(["list-metrics", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--paths").not())
        .stdout(predicate::str::contains("--jobs").not())
        .stdout(predicate::str::contains("--preproc-data").not());
}

/// `list-metrics` never consumed `--paths`; passing it is now a usage
/// error (exit 1, the tool-error code — never the 2-5 metric-gate band).
#[test]
fn list_metrics_rejects_paths_flag() {
    cli()
        .args(["list-metrics", "--paths", "src"])
        .assert()
        .failure()
        .code(1);
}

/// `vcs commit` scores a single commit, not a walked tree, so it gets
/// neither the walk nor the preproc group. `--preproc-data` is now a
/// usage error there.
#[test]
fn vcs_commit_rejects_preproc_data() {
    cli()
        .args(["vcs", "commit", "--preproc-data", "x.json"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("--preproc-data"));
}

/// `vcs commit --exclude-tests` is the headline silent-no-op the issue
/// calls out: before #597 it was ignored, now it is a hard usage error.
#[test]
fn vcs_commit_rejects_exclude_tests() {
    cli()
        .args(["vcs", "commit", "--exclude-tests"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("--exclude-tests"));
}

/// The old `jit` alias of `vcs commit` rejects the inert flag too.
#[test]
fn vcs_jit_rejects_exclude_tests() {
    cli()
        .args(["vcs", "jit", "--exclude-tests"])
        .assert()
        .failure()
        .code(1);
}

/// `vcs trend` samples a time series; it takes no walk flags either.
#[test]
fn vcs_trend_rejects_paths_flag() {
    cli()
        .args(["vcs", "trend", "--paths", "src"])
        .assert()
        .failure()
        .code(1);
}

/// A scoped flag passed *before* the subcommand (the pre-2.0 global
/// form) no longer parses — the deliberate break.
#[test]
fn paths_before_subcommand_is_rejected() {
    cli()
        .args(["--paths", "src", "metrics"])
        .assert()
        .failure()
        .code(1);
}

/// `--exclude-tests` still works where it is consumed: on `metrics`,
/// after the subcommand. This guards against over-scoping the flag away.
#[test]
fn metrics_still_accepts_exclude_tests() {
    let (dir, file) = fixture();
    cli()
        .args(["metrics", "--exclude-tests", "--paths"])
        .arg(&file)
        .assert()
        .success();
    drop(dir);
}

/// The walk-selection group renders under an "Input selection" help
/// heading on a consuming subcommand (#597 grouping).
#[test]
fn metrics_help_groups_input_selection() {
    cli()
        .args(["metrics", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Input selection"))
        .stdout(predicate::str::contains("--paths"));
}

// ----- #651: positional paths + -t/--type -----

/// `bca metrics <PATH>` walks the positional path — the headline #651
/// ergonomic fix.
#[test]
fn metrics_accepts_positional_path() {
    let (dir, file) = fixture();
    cli()
        .arg("metrics")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("a.rs"));
    drop(dir);
}

/// Positional `[PATHS]` and `--paths` union: both seeds are walked.
#[test]
fn metrics_unions_positional_and_paths_flag() {
    let dir = tempfile::tempdir().expect("tempdir");
    let a = dir.path().join("a.rs");
    let b = dir.path().join("b.rs");
    fs::write(&a, "fn a() {}\n").expect("write a");
    fs::write(&b, "fn b() {}\n").expect("write b");
    cli()
        .arg("metrics")
        .arg(&a)
        .arg("--paths")
        .arg(&b)
        .assert()
        .success()
        .stdout(predicate::str::contains("a.rs"))
        .stdout(predicate::str::contains("b.rs"));
    drop(dir);
}

/// `find` takes node kinds via a repeatable `-t`/`--type` and the path
/// positionally (#651).
#[test]
fn find_uses_type_flag_and_positional_path() {
    let (dir, file) = fixture();
    cli()
        .arg("find")
        .arg("-t")
        .arg("function_item")
        .arg(&file)
        .assert()
        .success()
        // Assert the matched node is actually reported: a bare `.success()`
        // would pass even if `-t` were silently ignored and nothing found.
        .stdout(predicate::str::contains("function_item"));
    drop(dir);
}

/// `-t` is repeatable: several node kinds in one `find`.
#[test]
fn find_accepts_repeated_type_flags() {
    let (dir, file) = fixture();
    cli()
        .arg("find")
        .arg("-t")
        .arg("function_item")
        .arg("--type")
        .arg("let_declaration")
        .arg(&file)
        .assert()
        .success();
    drop(dir);
}

/// `find` with no `-t` is a usage error (at least one node kind is
/// required).
#[test]
fn find_without_type_is_rejected() {
    let (dir, file) = fixture();
    cli().arg("find").arg(&file).assert().failure().code(1);
    drop(dir);
}

/// `count` likewise uses `-t` and prints the tally; the positional is a
/// path, not a node kind.
#[test]
fn count_uses_type_flag_and_positional_path() {
    let (dir, file) = fixture();
    cli()
        .arg("count")
        .arg("-t")
        .arg("function_item")
        .arg(&file)
        .assert()
        .success()
        // The fixture has exactly one `function_item`; assert the tally so a
        // silently-ignored `-t` (count 0) would fail rather than pass.
        .stdout(predicate::str::contains("Found nodes: 1"));
    drop(dir);
}

/// `count` with no `-t` is a usage error.
#[test]
fn count_without_type_is_rejected() {
    let (dir, file) = fixture();
    cli().arg("count").arg(&file).assert().failure().code(1);
    drop(dir);
}

/// #601 interaction: a following positional path is not swallowed by a
/// preceding `-I`/`--include` glob (each takes exactly one value). The
/// run succeeds and walks the positional path.
#[test]
fn include_does_not_swallow_following_positional_path() {
    let (dir, file) = fixture();
    cli()
        .arg("metrics")
        .arg("-I")
        .arg("*.rs")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("a.rs"));
    drop(dir);
}

/// #651/#669: an optional-value flag (`num_args(0..=1)`) must not swallow
/// a following positional `[PATHS]`. Before `require_equals = true`,
/// `bca metrics --cyclomatic-count-try src` parsed `src` as the flag's
/// bool value and errored with "invalid value 'src'". The bare flag now
/// uses its default-missing value and the path is the positional.
#[test]
fn bare_optional_value_flag_does_not_swallow_positional_path() {
    let (dir, file) = fixture();
    cli()
        .arg("metrics")
        .arg("--cyclomatic-count-try")
        .arg(&file)
        .assert()
        .success()
        .stdout(predicate::str::contains("a.rs"));
    drop(dir);
}

/// #688/#669: `--tier` takes its value with `=` (`--tier=soft`) so a
/// following positional path is walked rather than parsed as the tier.
#[test]
fn tier_equals_form_walks_following_positional_path() {
    let (dir, file) = fixture();
    cli()
        .arg("check")
        .arg("--tier=soft")
        .arg(&file)
        .assert()
        // The trivial fixture has no offenders, so the gate passes (exit
        // 0); the point is that `--tier=soft` did not consume the path.
        .success();
    drop(dir);
}

/// #688/#669: the space form `--tier soft <path>` no longer takes `soft`
/// as the value — with `require_equals` it is left as a positional, so
/// `soft` is treated as an input path and the run fails because that
/// path does not exist.
#[test]
fn tier_space_form_treats_value_as_positional() {
    cli()
        .arg("check")
        .arg("--tier")
        .arg("soft")
        .assert()
        .failure()
        .stderr(predicate::str::contains("path does not exist: soft"));
}
