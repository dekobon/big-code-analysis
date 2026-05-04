use assert_cmd::Command;
use predicates::prelude::*;

fn cli() -> Command {
    Command::cargo_bin("big-code-analysis-cli").unwrap()
}

/// Without any value, `--list-metrics` prints metric names one per line.
/// The output must include both top-level metric categories and the `loc`
/// sub-metrics that downstream tools (split-minimal-tests.py) grep for.
#[test]
fn list_metrics_prints_names() {
    let assertion = cli().args(["--list-metrics"]).assert().success();
    let stdout = String::from_utf8(assertion.get_output().stdout.clone()).unwrap();
    let names: Vec<&str> = stdout.lines().collect();
    for required in [
        "cognitive",
        "cyclomatic",
        "halstead",
        "sloc",
        "ploc",
        "lloc",
        "cloc",
        "blank",
        "nom",
        "nexits",
        "nargs",
        "mi",
        "abc",
        "wmc",
        "npm",
        "npa",
    ] {
        assert!(
            names.contains(&required),
            "missing {required:?} in --list-metrics output: {stdout}"
        );
    }
}

#[test]
fn list_metrics_descriptions_includes_descriptions() {
    cli()
        .args(["--list-metrics", "descriptions"])
        .assert()
        .success()
        .stdout(predicate::str::contains("cognitive"))
        .stdout(predicate::str::contains("Cognitive complexity"));
}

#[test]
fn list_metrics_invalid_mode_rejected() {
    // Assert clap's "invalid value" message specifically, so the test still
    // fails if `--list-metrics` is removed (a "missing argument" failure
    // would otherwise let this test pass for the wrong reason).
    cli()
        .args(["--list-metrics", "bogus"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}

/// `--list-metrics` should bypass the "no action specified" check and emit
/// the metric catalog with no other flags supplied. The non-empty stdout
/// check guards against a silent early-exit regression.
#[test]
fn list_metrics_does_not_require_action_flag() {
    cli()
        .args(["--list-metrics"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}
