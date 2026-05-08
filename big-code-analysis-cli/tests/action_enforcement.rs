use assert_cmd::Command;
use predicates::prelude::*;

fn cli() -> Command {
    Command::cargo_bin("big-code-analysis-cli").unwrap()
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

/// Subcommands are mutually exclusive by construction; clap rejects a
/// second subcommand token as an unexpected positional argument at parse
/// time. Asserting the offending token appears in stderr ensures we
/// catch the *correct* failure — not, e.g., "no files to analyze" if
/// `metrics` were ever silently swallowed as a path.
#[test]
fn two_subcommands_rejected() {
    cli()
        .args(["dump", "metrics"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unexpected argument 'metrics'"));
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

/// `find` requires at least one node-type argument. Asserting on the
/// `<NODES>` metavar (which we control via the `nodes:` field name in
/// `NodesArgs`) catches the *required-argument* failure specifically.
/// Without this, a regression that made `nodes` optional could pass for
/// the wrong reason — the program would fail later with "no files to
/// analyze".
#[test]
fn find_without_nodes_rejected() {
    cli()
        .args(["find"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("<NODES>"));
}

/// `count` requires at least one node-type argument. See
/// `find_without_nodes_rejected` for why the metavar check matters.
#[test]
fn count_without_nodes_rejected() {
    cli()
        .args(["count"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("<NODES>"));
}

/// Aggregated CI/IDE formats (Checkstyle / SARIF / Clang / MSVC warning) are
/// shared via `MetricsFormat` so clap parses them on `ops`, but the dispatch
/// rejects them at runtime because `ops` emits operands/operators, not
/// offender records. Asserting both the failure exit code AND a substring of
/// the rejection message guards against the runtime guard being silently
/// removed (which would otherwise let `ops -O checkstyle` succeed with no
/// output and exit 0).
#[test]
fn ops_rejects_aggregated_formats_at_runtime() {
    for fmt in ["checkstyle", "sarif", "clang-warning", "msvc-warning"] {
        cli()
            .args(["ops", "-O", fmt])
            .assert()
            .failure()
            .stderr(predicate::str::contains("aggregated formats"))
            .stderr(predicate::str::contains("not supported by `ops`"));
    }
}

/// CSV / HTML have a metric-shaped row schema and are not meaningful for
/// the operands/operators output of `ops`. Same runtime-rejection pattern as
/// the aggregated formats above.
#[test]
fn ops_rejects_funcspace_formats_at_runtime() {
    for fmt in ["csv", "html"] {
        cli()
            .args(["ops", "-O", fmt])
            .assert()
            .failure()
            .stderr(predicate::str::contains("not supported by `ops`"))
            .stderr(predicate::str::contains("metric-shaped"));
    }
}
