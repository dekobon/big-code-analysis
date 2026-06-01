//! Integration tests for `[check.exclude]` / `--check-exclude*`
//! (issue #378): files analysed and reported, but exempt from the
//! `bca check` threshold gate.
//!
//! Each test drives the real binary against a temp directory of tiny
//! inline Rust fixtures so they don't depend on any submodule. The
//! `.git` marker halts `bca.toml` discovery at the fixture root.

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
/// manifest-discovery tests below pass their own fixture dir, which
/// carries a deliberate `.git` + `bca.toml`.
fn cli(dir: &Path) -> Command {
    common::cli_in(dir)
}

/// Cyclomatic == 4 (three decision points plus one). Named per-fixture
/// so a test can assert which offender survived the gate.
fn branchy(fn_name: &str) -> String {
    format!(
        "pub fn {fn_name}(n: i32) -> i32 {{ \
         if n < 0 {{ 1 }} else if n == 0 {{ 2 }} else if n < 10 {{ 3 }} else {{ 4 }} }}\n"
    )
}

/// Temp dir with `excluded.rs` and `kept.rs`, each holding one branchy
/// offender. The function names embed the filename so assertions can
/// distinguish which file's violation was emitted.
fn two_file_fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("excluded.rs"), branchy("excluded_offender")).unwrap();
    fs::write(dir.path().join("kept.rs"), branchy("kept_offender")).unwrap();
    dir
}

/// Acceptance: `bca check` honors `--check-exclude` and does not emit
/// violations from matching files — but a non-matching offender still
/// fails the gate (exit 2). The skipped count is announced on stderr.
#[test]
fn check_exclude_flag_drops_matching_offenders_only() {
    let dir = two_file_fixture();

    cli(dir.path())
        .args([
            "--paths",
            dir.path().to_str().unwrap(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--check-exclude",
            "**/excluded.rs",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("kept_offender"))
        .stderr(predicate::str::contains("excluded_offender").not())
        .stderr(predicate::str::contains(
            "skipped 1 violations via [check.exclude]",
        ));
}

/// When the glob covers the *only* offender, the gate passes clean
/// (exit 0). The file is still walked (so the "no input files matched"
/// tool error does not fire) — its violation is simply dropped.
#[test]
fn check_exclude_covering_sole_offender_exits_zero() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("excluded.rs"), branchy("excluded_offender")).unwrap();

    cli(dir.path())
        .args([
            "--paths",
            dir.path().to_str().unwrap(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--check-exclude",
            "**/excluded.rs",
        ])
        .assert()
        .success()
        // The skipped-count line proves the offender *existed and was
        // dropped* — without it, a clean exit could mean the fixture
        // simply stopped offending (a vacuous pass).
        .stderr(predicate::str::contains(
            "skipped 1 violations via [check.exclude]",
        ))
        .stderr(predicate::str::contains("excluded_offender").not());
}

/// `--check-exclude-from` reads `.gitignore`-style globs from a file;
/// the deny-set behaves identically to inline `--check-exclude`.
#[test]
fn check_exclude_from_file_drops_matching_offenders() {
    let dir = two_file_fixture();
    let ignore = dir.path().join(".bcacheckignore");
    fs::write(&ignore, "# structural exemptions\n\n**/excluded.rs\n").unwrap();

    cli(dir.path())
        .args([
            "--paths",
            dir.path().to_str().unwrap(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--check-exclude-from",
            ignore.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("kept_offender"))
        .stderr(predicate::str::contains("excluded_offender").not());
}

/// Acceptance: `--write-baseline` does NOT record entries for
/// `[check.exclude]` files, keeping the baseline free of structural
/// exemptions. The excluded offender must be absent from the written
/// TOML; the kept one present.
#[test]
fn write_baseline_omits_excluded_files() {
    let dir = two_file_fixture();
    let baseline = dir.path().join("base.toml");

    cli(dir.path())
        .args([
            "--paths",
            dir.path().to_str().unwrap(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--check-exclude",
            "**/excluded.rs",
            "--write-baseline",
            baseline.to_str().unwrap(),
        ])
        .assert()
        .success();

    let written = fs::read_to_string(&baseline).expect("baseline written");
    assert!(
        written.contains("kept_offender"),
        "kept offender must be baselined:\n{written}"
    );
    assert!(
        !written.contains("excluded_offender"),
        "excluded offender must NOT be baselined:\n{written}"
    );
}

/// Acceptance: `bca report` continues to show `[check.exclude]` files —
/// visibility is preserved, only the gate skips them. The report is a
/// separate command that never consults the check-exclude set, so the
/// excluded function appears in the markdown hotspot tables.
#[test]
fn report_markdown_still_shows_excluded_files() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("excluded.rs"), branchy("excluded_offender")).unwrap();

    cli(dir.path())
        .args([
            "--paths",
            dir.path().to_str().unwrap(),
            "report",
            "markdown",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("excluded_offender"));
}

/// The `bca.toml` `[check] exclude` table drives the gate exactly like
/// the flag: a bare `bca check` in the fixture directory drops the
/// excluded file's offenders.
#[test]
fn manifest_check_exclude_table_drops_offenders() {
    let dir = two_file_fixture();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(
        dir.path().join("bca.toml"),
        "paths = [\".\"]\n\n[thresholds]\ncyclomatic = 1\n\n[check]\nexclude = [\"**/excluded.rs\"]\n",
    )
    .unwrap();

    cli(dir.path())
        .arg("check")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("kept_offender"))
        .stderr(predicate::str::contains("excluded_offender").not());
}

/// An explicit `--check-exclude` wins over the manifest `[check]
/// exclude` list (CLI precedence): the flag replaces the manifest's
/// globs, so the manifest's `**/excluded.rs` no longer applies and that
/// offender resurfaces, while the flag's `**/kept.rs` now suppresses the
/// other.
#[test]
fn cli_check_exclude_overrides_manifest_table() {
    let dir = two_file_fixture();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(
        dir.path().join("bca.toml"),
        "paths = [\".\"]\n\n[thresholds]\ncyclomatic = 1\n\n[check]\nexclude = [\"**/excluded.rs\"]\n",
    )
    .unwrap();

    cli(dir.path())
        .args(["check", "--check-exclude", "**/kept.rs"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("excluded_offender"))
        .stderr(predicate::str::contains("kept_offender").not());
}

/// `--print-effective-config` surfaces the resolved `check_exclude`
/// globs (provenance for the gate's filtering inputs).
#[test]
fn print_effective_config_lists_check_exclude() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("kept.rs"), branchy("kept_offender")).unwrap();

    cli(dir.path())
        .args([
            "--paths",
            dir.path().to_str().unwrap(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--check-exclude",
            "tests/**",
            "--print-effective-config",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("check_exclude"))
        .stdout(predicate::str::contains("tests/**"));
}

/// An unreadable `--check-exclude-from` file must attribute the error
/// to the flag the user actually passed, not the walker's
/// `--exclude-from`. Regression for the shared `read_exclude_patterns_from`
/// label, which previously hardcoded `--exclude-from` for both surfaces.
#[test]
fn check_exclude_from_missing_file_names_the_right_flag() {
    let dir = two_file_fixture();
    let missing = dir.path().join("does-not-exist.txt");

    cli(dir.path())
        .args([
            "--paths",
            dir.path().to_str().unwrap(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--check-exclude-from",
            missing.to_str().unwrap(),
        ])
        .assert()
        // Tool error (bad input), not the exit-2 gate failure.
        .code(1)
        .stderr(predicate::str::contains("--check-exclude-from"))
        // The misleading walker-flag attribution must not appear. The
        // double-dash anchor distinguishes it from the correct
        // `--check-exclude-from` substring (which has a single dash
        // before `exclude-from`).
        .stderr(predicate::str::contains(" --exclude-from").not());
}

/// #493: a manifest `[check.exclude]` glob must exempt the same files
/// when `bca check` runs from a subdirectory below the manifest dir.
/// `paths = ["."]` resolves to the manifest dir (an ancestor of the
/// CWD), so the walk root is absolute and above the CWD — pre-fix the
/// `./`-anchored `[check.exclude]` matched the emitted absolute
/// violation path and exempted nothing, failing the gate on the
/// vendored offender. Matching is now anchored to the walk root.
#[test]
fn check_exclude_manifest_glob_applies_from_subdir() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    // `.git` marks the manifest-discovery boundary; the manifest gates
    // cyclomatic and structurally exempts the vendored subtree.
    fs::create_dir(repo.join(".git")).unwrap();
    fs::write(
        repo.join("bca.toml"),
        "paths = [\".\"]\n\n[thresholds]\ncyclomatic = 1\n\n[check]\nexclude = [\"./vendor/**\"]\n",
    )
    .unwrap();
    fs::create_dir(repo.join("vendor")).unwrap();
    fs::write(repo.join("vendor/v.rs"), branchy("vendor_offender")).unwrap();
    fs::create_dir(repo.join("src")).unwrap();
    fs::write(repo.join("src/keep.rs"), branchy("keep_offender")).unwrap();

    // Run from `src/`: the manifest is discovered by climbing to `repo/`,
    // whose `paths=["."]` resolves to `repo/` — above this CWD.
    common::cli_in(&repo.join("src"))
        .arg("check")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("keep_offender"))
        .stderr(predicate::str::contains("vendor_offender").not())
        .stderr(predicate::str::contains(
            "skipped 1 violations via [check.exclude]",
        ));
}

/// Regression for #497: `[check.exclude]` / `--check-exclude` must
/// anchor violations from `--paths-from` seeds, not only `--paths`.
///
/// Before the fix, `apply_check_exclude` was handed only `--paths`, so a
/// violation from a `--paths-from`-sourced *absolute* seed was matched
/// against the deny-set unanchored (in its absolute form). A
/// walk-root-anchored pattern (`./excluded.rs`) therefore never matched
/// it and the exclude silently no-opped — the offender failed the gate
/// (or polluted a baseline). The corpus lives in a *sibling* tempdir so
/// the absolute seed is not at/under the CWD and `reanchor_seed` cannot
/// collapse it: it stays absolute, reproducing the bug condition.
#[test]
fn check_exclude_anchors_paths_from_seeds() {
    // CWD dir carries the `.git` marker that halts bca.toml discovery.
    let cwd = TempDir::new().unwrap();
    fs::create_dir(cwd.path().join(".git")).unwrap();

    // The analyzed corpus is an independent (sibling) absolute path.
    let corpus = TempDir::new().unwrap();
    fs::write(
        corpus.path().join("excluded.rs"),
        branchy("excluded_offender"),
    )
    .unwrap();
    fs::write(corpus.path().join("kept.rs"), branchy("kept_offender")).unwrap();

    let list = cwd.path().join("paths.txt");
    fs::write(&list, format!("{}\n", corpus.path().to_str().unwrap())).unwrap();

    cli(cwd.path())
        .args([
            "--paths-from",
            list.to_str().unwrap(),
            "check",
            "--threshold",
            "cyclomatic=1",
            "--check-exclude",
            "./excluded.rs",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("kept_offender"))
        .stderr(predicate::str::contains("excluded_offender").not())
        .stderr(predicate::str::contains(
            "skipped 1 violations via [check.exclude]",
        ));
}
