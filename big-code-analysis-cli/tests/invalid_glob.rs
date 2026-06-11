#![allow(missing_docs)]
use assert_cmd::Command;
use predicates::prelude::*;

mod common;

fn cli() -> Command {
    common::bca_command()
}

#[test]
fn invalid_include_glob_exits_nonzero_with_error() {
    cli()
        .args(["metrics", "-I", "[", "--paths", "."])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid glob pattern"));
}

#[test]
fn invalid_exclude_glob_exits_nonzero_with_error() {
    cli()
        .args(["metrics", "-X", "[", "--paths", "."])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid glob pattern"));
}

#[test]
fn valid_glob_does_not_error() {
    cli()
        .args(["metrics", "-I", "*.rs", "--paths", "."])
        .assert()
        .success();
}
