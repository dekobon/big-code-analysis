#![allow(missing_docs)]
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::NamedTempFile;

mod common;

fn cli() -> Command {
    common::bca_command()
}

/// Since #663 an *explicitly-named* file with an unrecognized language
/// warns unconditionally (not gated behind `-w`) and — being the sole
/// input that produced no output — exits 1, mirroring the #596
/// nonexistent-explicit-path rule. The `-w` walk-warning gate now only
/// covers directory-discovered files (see `paths_discovery.rs`).
#[test]
fn explicit_unrecognized_warns_and_exits_one() {
    let tmp = NamedTempFile::with_suffix(".unknownlang123").unwrap();
    std::fs::write(tmp.path(), "some content\n").unwrap();

    cli()
        .args(["metrics", "--paths", tmp.path().to_str().unwrap()])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "warning: skipping explicitly-named file with unrecognized language:",
        ));
}

/// The explicit-file warning fires with or without `-w`: it is a user
/// error (the user named one file and got nothing), not a noisy
/// directory-walk skip.
#[test]
fn explicit_unrecognized_warns_even_with_warning_flag() {
    let tmp = NamedTempFile::with_suffix(".unknownlang123").unwrap();
    std::fs::write(tmp.path(), "some content\n").unwrap();

    cli()
        .args(["metrics", "-w", "--paths", tmp.path().to_str().unwrap()])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "warning: skipping explicitly-named file with unrecognized language:",
        ));
}

#[test]
fn warning_flag_emits_empty_file() {
    let tmp = NamedTempFile::with_suffix(".rs").unwrap();
    // File is already empty by default.

    cli()
        .args(["metrics", "-w", "--paths", tmp.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains("warning: skipping empty file:"));
}
