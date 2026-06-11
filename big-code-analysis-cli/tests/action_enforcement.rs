#![allow(missing_docs)]
use assert_cmd::Command;
use predicates::prelude::*;

mod common;

fn cli() -> Command {
    common::bca_command()
}

/// Running with no subcommand should print help (or fail with a help-style
/// message) rather than silently succeed.
#[test]
fn no_subcommand_rejected() {
    cli()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage").or(predicate::str::contains("help")));
}

/// Subcommands are mutually exclusive by construction. Since #651 freed
/// the positional slot on walking subcommands for input `[PATHS]`, a
/// second subcommand token after `dump` is consumed as a path rather
/// than rejected at parse time. It must still fail loudly: the
/// nonexistent `metrics` path is reported (exit 1, #596 fail-on-bad-path
/// semantics), not silently swallowed into a "no files to analyze"
/// no-op. Asserting the offending token names the path ensures we catch
/// the *correct* failure.
#[test]
fn two_subcommands_rejected() {
    cli().args(["dump", "metrics"]).assert().failure().stderr(
        predicate::str::contains("path does not exist").and(predicate::str::contains("metrics")),
    );
}

/// `--top` lives only on `report`. Passing it to `metrics` is a parse-time
/// error.
#[test]
fn top_rejected_on_metrics() {
    cli()
        .args(["metrics", "--top", "5"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--top"));
}

/// Markdown was a metrics format pre-restructure; now `bca metrics -O
/// markdown` is rejected at parse time because `MetricsFormat` does not
/// include it.
#[test]
fn markdown_rejected_as_metrics_format() {
    cli()
        .args(["metrics", "-O", "markdown"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}

/// Legacy invocations should fail with a migration hint pointing at the new
/// command, so CI breakage on upgrade is actionable.
#[test]
fn legacy_metrics_flag_emits_migration_hint() {
    cli()
        .args(["--metrics", "-O", "markdown"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("report markdown"))
        .stderr(predicate::str::contains("subcommands"));
}

/// `find` requires at least one node-type argument. The node kinds now
/// live on the repeatable `-t`/`--type` flag (#651), whose value metavar
/// is `<NODE_TYPE>`; asserting on it catches the *required-argument*
/// failure specifically. Without this, a regression that made the flag
/// optional could pass for the wrong reason — the program would fail
/// later with "no files to analyze".
#[test]
fn find_without_nodes_rejected() {
    cli()
        .args(["find"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("<NODE_TYPE>"));
}

/// `count` requires at least one node-type argument. See
/// `find_without_nodes_rejected` for why the metavar check matters.
#[test]
fn count_without_nodes_rejected() {
    cli()
        .args(["count"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("<NODE_TYPE>"));
}

/// Issue #235 moved the offender formats off `metrics` / `ops` and
/// onto `bca check --output-format`. `MetricsFormat` no longer
/// enumerates them, so clap rejects the values at parse time on both
/// commands. The hint we install in `legacy_hint` then points the
/// user at the new home.
#[test]
fn metrics_rejects_offender_formats_with_migration_hint() {
    for fmt in ["checkstyle", "sarif", "clang-warning", "msvc-warning"] {
        cli()
            .args(["metrics", "-O", fmt])
            .assert()
            .failure()
            .stderr(predicate::str::contains("bca check"))
            .stderr(predicate::str::contains(fmt));
    }
}

#[test]
fn ops_rejects_offender_formats_with_migration_hint() {
    for fmt in ["checkstyle", "sarif", "clang-warning", "msvc-warning"] {
        cli()
            .args(["ops", "-O", fmt])
            .assert()
            .failure()
            .stderr(predicate::str::contains("bca check"))
            .stderr(predicate::str::contains(fmt));
    }
}

/// CSV has a metric-shaped row schema and is not meaningful for the
/// operands/operators output of `ops`. Same runtime-rejection pattern as
/// the aggregated formats above.
#[test]
fn ops_rejects_csv_format_at_runtime() {
    cli()
        .args(["ops", "-O", "csv"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not supported by `ops`"))
        .stderr(predicate::str::contains("metric-shaped"));
}

/// Under the unified #669 semantics `bca metrics -o <file>` is a single
/// aggregate file, so pointing `--output` at an existing regular file is
/// no longer an error: it is overwritten with the aggregate document. The
/// per-file directory-tree mode that *did* require a directory target
/// moved to `--output-dir` (see `metrics_output_dir_rejects_regular_file`).
#[test]
fn metrics_output_file_overwrites_regular_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("a.rs");
    std::fs::write(&src, b"fn main() {}\n").expect("seed source");
    let file_path = dir.path().join("out.json");
    std::fs::write(&file_path, b"stale").expect("seed file");

    cli()
        .args([
            "metrics",
            "-O",
            "json",
            "-o",
            file_path.to_str().expect("utf8"),
            src.to_str().expect("utf8"),
        ])
        .assert()
        .success();
    let written = std::fs::read_to_string(&file_path).expect("read aggregate");
    assert!(
        written.trim_start().starts_with('['),
        "aggregate --output must be a top-level JSON array, got: {written}"
    );
}

/// `--output-dir <existing-file>` must die: the per-file-tree mode needs a
/// directory target so each input file gets its own sibling document.
/// Pointing it at a regular file is a configuration error (#669).
#[test]
fn metrics_output_dir_rejects_regular_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file_path = dir.path().join("not_a_dir.json");
    std::fs::write(&file_path, b"").expect("seed file");

    cli()
        .args([
            "metrics",
            "-O",
            "json",
            "--paths",
            ".",
            "--output-dir",
            file_path.to_str().expect("utf8"),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--output-dir must be a directory for `metrics`",
        ));
}

/// Symmetric `--output-dir` directory-target check for `ops`, pinning the
/// per-command error message so a refactor that swaps the wrong command
/// name into the shared helper's `format_args!` fails loudly (#669).
#[test]
fn ops_output_dir_rejects_regular_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file_path = dir.path().join("not_a_dir.json");
    std::fs::write(&file_path, b"").expect("seed file");

    cli()
        .args([
            "ops",
            "-O",
            "json",
            "--paths",
            ".",
            "--output-dir",
            file_path.to_str().expect("utf8"),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--output-dir must be a directory for `ops`",
        ));
}
