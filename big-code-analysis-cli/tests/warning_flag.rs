use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::NamedTempFile;

fn cli() -> Command {
    Command::cargo_bin("big-code-analysis-cli").unwrap()
}

#[test]
fn warning_flag_emits_unrecognized_language() {
    let tmp = NamedTempFile::with_suffix(".unknownlang123").unwrap();
    std::fs::write(tmp.path(), "some content\n").unwrap();

    cli()
        .args(["--metrics", "-w", "--paths", tmp.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "warning: skipping file with unrecognized language:",
        ));
}

#[test]
fn no_warning_without_flag() {
    let tmp = NamedTempFile::with_suffix(".unknownlang123").unwrap();
    std::fs::write(tmp.path(), "some content\n").unwrap();

    cli()
        .args(["--metrics", "--paths", tmp.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

#[test]
fn warning_flag_emits_empty_file() {
    let tmp = NamedTempFile::with_suffix(".rs").unwrap();
    // File is already empty by default.

    cli()
        .args(["--metrics", "-w", "--paths", tmp.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains("warning: skipping empty file:"));
}
