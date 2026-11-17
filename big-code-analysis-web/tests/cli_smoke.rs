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

/// No issue-tracker reference (`#707`) may reach `bca-web --help`, or the
/// man page generated from it: provenance belongs in `//` comments, as
/// `bca`'s own help test holds for that binary.
#[test]
fn help_carries_no_issue_references() {
    let output = bin().arg("--help").output().unwrap();
    let help = String::from_utf8(output.stdout).unwrap();
    let reference = help.match_indices('#').find(|(at, _)| {
        let digits = help[at + 1..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        (2..=4).contains(&digits)
    });
    assert!(
        reference.is_none(),
        "`bca-web --help` leaks an issue reference:\n{help}"
    );
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
