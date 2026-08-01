#![allow(missing_docs)]
use assert_cmd::Command;
use predicates::prelude::*;

use crate::common;

fn cli() -> Command {
    common::bca_command()
}

// An invalid glob on a non-`check` subcommand must take the `die` path
// (exit 1, #594), never clap's usage exit 2 or the 2-5 band the `check`
// gate owns. `.failure()` accepts any non-zero code and so masks a
// regression to 2 (#907); these pin the exact documented contract. The
// `-X` exclude path has no other exit-1 guard in the suite.
#[test]
fn invalid_include_glob_exits_one_with_error() {
    cli()
        .args(["metrics", "-I", "[", "--paths", "."])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("invalid glob pattern"));
}

#[test]
fn invalid_exclude_glob_exits_one_with_error() {
    cli()
        .args(["metrics", "-X", "[", "--paths", "."])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("invalid glob pattern"));
}

#[test]
fn valid_glob_does_not_error() {
    cli()
        .args(["metrics", "-I", "*.rs", "--paths", "."])
        .assert()
        .success();
}
