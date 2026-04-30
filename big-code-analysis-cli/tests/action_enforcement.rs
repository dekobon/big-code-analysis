use assert_cmd::Command;
use predicates::prelude::*;

fn cli() -> Command {
    Command::cargo_bin("big-code-analysis-cli").unwrap()
}

/// Running the CLI without any action flag should fail loudly rather than
/// silently succeed with no output.
#[test]
fn no_action_flag_rejected_at_runtime() {
    cli()
        .assert()
        .failure()
        .stderr(predicate::str::contains("no action specified"));
}

/// Two action flags should be rejected by clap's `action` ArgGroup at parse
/// time. Previously the `act_on_file` if/else chain silently picked one and
/// dropped the other.
#[test]
fn dump_and_function_rejected_at_parse() {
    cli()
        .args(["-d", "-F"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

/// `--metrics` plus `--find` was the canonical issue example: the user
/// expects both, gets only metrics. Now clap rejects the combination
/// upfront.
#[test]
fn metrics_and_find_rejected_at_parse() {
    cli()
        .args(["-m", "-f", "identifier"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

/// `--dump` paired with `-O` is rejected because `-O` requires the
/// `format_action` group (only `--metrics` and `--ops` honour
/// `--output-format`).
#[test]
fn dump_with_output_format_rejected_at_parse() {
    cli()
        .args(["-d", "-O", "json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--metrics|--ops"));
}
