#![allow(missing_docs)]
use assert_cmd::Command;
use predicates::prelude::*;

fn bin() -> Command {
    Command::cargo_bin("bca-web").unwrap()
}

#[test]
fn help_prints_usage_with_renamed_binary() {
    bin()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: bca-web"))
        .stdout(predicate::str::contains("--host"))
        .stdout(predicate::str::contains("--port"));
}

#[test]
fn version_prints() {
    bin()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("bca-web"));
}

#[test]
fn unknown_flag_is_rejected() {
    bin()
        .arg("--definitely-not-a-flag")
        .assert()
        .failure()
        .stderr(predicate::str::contains("unexpected argument"));
}
