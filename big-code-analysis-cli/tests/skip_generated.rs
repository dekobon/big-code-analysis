#![allow(missing_docs)]
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

mod common;

/// Hermetic `bca` builder: anchors the process cwd at `dir` (a `.git`-free
/// fixture tempdir) so `bca metrics` cannot climb to the repo's own
/// `bca.toml` and silently merge `exclude_tests` / `cyclomatic_count_try`
/// / walk-scope globals into the fixture run (#491, #915). The fixture
/// `TempDir` guard must outlive the command spawn.
fn cli(dir: &Path) -> Command {
    common::cli_in(dir)
}

/// Build a fixture directory with one generated and one hand-written file.
/// `TempDir`'s root is hidden (e.g. `/tmp/.tmpXYZ`) and the walker's
/// `is_hidden` filter would skip everything below it, so the fixture
/// places the files inside a non-hidden `fix` subdirectory and the helper
/// returns that subdirectory's path.
fn make_mixed_fixture() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("fix");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join("generated.rs"),
        "// @generated\nfn g() { let _ = 1; }\n",
    )
    .unwrap();
    std::fs::write(root.join("hand.rs"), "fn h() { let _ = 1 + 2; }\n").unwrap();
    (dir, root)
}

#[test]
fn metrics_skips_generated_file_by_default() {
    let (dir, root) = make_mixed_fixture();

    cli(dir.path())
        .args(["metrics", "--paths", root.to_str().unwrap(), "-O", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hand.rs"))
        .stdout(predicate::str::contains("generated.rs").not());
}

#[test]
fn no_skip_generated_includes_generated_file() {
    let (dir, root) = make_mixed_fixture();

    cli(dir.path())
        .args([
            "metrics",
            "--no-skip-generated",
            "--paths",
            root.to_str().unwrap(),
            "-O",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("hand.rs"))
        .stdout(predicate::str::contains("generated.rs"));
}

#[test]
fn report_skipped_logs_each_skipped_file() {
    let (dir, root) = make_mixed_fixture();

    cli(dir.path())
        .args([
            "metrics",
            "--report-skipped",
            "--paths",
            root.to_str().unwrap(),
            "-O",
            "json",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("note: skipped (generated):"))
        .stderr(predicate::str::contains("generated.rs"));
}

#[test]
fn marker_in_body_is_not_skipped() {
    use std::fmt::Write as _;

    // A file mentioning the phrase deep in its body must NOT be skipped.
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("fix");
    std::fs::create_dir(&root).unwrap();
    let mut content = String::new();
    for i in 0..120 {
        let _ = writeln!(content, "// line {i}");
    }
    content.push_str("// @generated -- but this is line 120, past the scan window\n");
    content.push_str("fn f() { let _ = 1 + 2; }\n");
    std::fs::write(root.join("late_marker.rs"), content).unwrap();

    cli(dir.path())
        .args(["metrics", "--paths", root.to_str().unwrap(), "-O", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("late_marker.rs"));
}

/// Hermeticity guard for the `cli()` switch to `cli_in` (#915): a
/// `bca.toml` discoverable from the *inherited* process cwd must not
/// influence a fixture run. The repo's own root `bca.toml` is exactly
/// such a file for the non-hermetic `bca_command()`. To make the leak
/// observable and the test revert-sensitive, this drives the process cwd
/// into a `.git`-rooted dir whose `bca.toml` excludes `hand.rs`:
///
/// - With the hermetic builder (`cli_in` at a clean tempdir), the cwd
///   manifest is never discovered, so `hand.rs` survives.
/// - Reverting `cli()` to `bca_command()` (no cwd anchor) inherits this
///   leaky cwd, discovers the manifest, and drops `hand.rs` — the test
///   then fails, proving it guards the anchor (see `.claude/rules/testing.md`).
///
/// The control arm (raw command explicitly rooted at the leaky dir)
/// proves the exclude actually fires, so the hermetic assertion is not
/// vacuous.
#[test]
fn hermetic_cwd_ignores_discoverable_manifest() {
    let (fixture, root) = make_mixed_fixture();

    // A `.git`-rooted dir whose manifest excludes hand.rs. The `.git`
    // marker stops `bca`'s upward discovery climb here.
    let manifest_dir = TempDir::new().unwrap();
    std::fs::create_dir(manifest_dir.path().join(".git")).unwrap();
    std::fs::write(
        manifest_dir.path().join("bca.toml"),
        "exclude = [\"**/hand.rs\"]\n",
    )
    .unwrap();

    // Control: rooted at the manifest dir, discovery drops hand.rs. If
    // this branch did not fire, the hermetic assertion would be vacuous.
    Command::cargo_bin("bca")
        .unwrap()
        .current_dir(manifest_dir.path())
        .args(["metrics", "--paths", root.to_str().unwrap(), "-O", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hand.rs").not());

    // Drive the *process* cwd into the leaky dir so a non-hermetic
    // `bca_command()` (the pre-#915 helper) would inherit it. The guard
    // restores the prior cwd on drop and serializes against peers.
    let _cwd = common::CwdGuard::enter(manifest_dir.path());

    // Hermetic: `cli_in` anchors at the clean fixture tempdir, overriding
    // the leaky inherited cwd, so the manifest is not discovered and
    // hand.rs survives.
    cli(fixture.path())
        .args(["metrics", "--paths", root.to_str().unwrap(), "-O", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hand.rs"));
}
