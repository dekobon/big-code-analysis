use assert_cmd::Command;
use predicates::prelude::*;

fn cli() -> Command {
    Command::cargo_bin("big-code-analysis-cli").unwrap()
}

#[test]
fn invalid_include_glob_exits_nonzero_with_error() {
    cli()
        .args(["-I", "[", "--paths", ".", "metrics"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid glob pattern"));
}

#[test]
fn invalid_exclude_glob_exits_nonzero_with_error() {
    cli()
        .args(["-X", "[", "--paths", ".", "metrics"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid glob pattern"));
}

#[test]
fn valid_glob_does_not_error() {
    cli()
        .args(["-I", "*.rs", "--paths", ".", "metrics"])
        .assert()
        .success();
}
