use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn cli() -> Command {
    Command::cargo_bin("big-code-analysis-cli").unwrap()
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
    let (_dir, root) = make_mixed_fixture();

    cli()
        .args(["--paths", root.to_str().unwrap(), "metrics", "-O", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hand.rs"))
        .stdout(predicate::str::contains("generated.rs").not());
}

#[test]
fn no_skip_generated_includes_generated_file() {
    let (_dir, root) = make_mixed_fixture();

    cli()
        .args([
            "--no-skip-generated",
            "--paths",
            root.to_str().unwrap(),
            "metrics",
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
    let (_dir, root) = make_mixed_fixture();

    cli()
        .args([
            "--report-skipped",
            "--paths",
            root.to_str().unwrap(),
            "metrics",
            "-O",
            "json",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("skipped (generated):"))
        .stderr(predicate::str::contains("generated.rs"));
}

#[test]
fn marker_in_body_is_not_skipped() {
    // A file mentioning the phrase deep in its body must NOT be skipped.
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("fix");
    std::fs::create_dir(&root).unwrap();
    let mut content = String::new();
    for i in 0..120 {
        content.push_str(&format!("// line {i}\n"));
    }
    content.push_str("// @generated -- but this is line 120, past the scan window\n");
    content.push_str("fn f() { let _ = 1 + 2; }\n");
    std::fs::write(root.join("late_marker.rs"), content).unwrap();

    cli()
        .args(["--paths", root.to_str().unwrap(), "metrics", "-O", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("late_marker.rs"));
}
