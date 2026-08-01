//! `bca check`'s stdout/stderr split (issue #1167).
//!
//! The offender rows are the command's *product* and belong on stdout,
//! where `| wc -l`, `| head`, `| rg -c` and `2>/dev/null` can reach
//! them. Everything else the run says about itself — the summary footer,
//! the `skipped` / `filtered` counts, warnings, the remediation block —
//! is commentary and belongs on stderr. Before #1167 the rows went to
//! stderr too, so all four of those idioms reported an empty offender
//! set: a *plausible* "this tree is clean" rather than an error.
//!
//! Every test here asserts **both** halves. A one-sided assertion would
//! stay green against a later change that swept everything back onto one
//! stream, which is the regression this file exists to prevent.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use tempfile::TempDir;

use crate::common;

/// `classify` measures `cyclomatic = 5`, so any of these fixtures
/// offends at `cyclomatic=1`. The function name is parameterised so a
/// two-file corpus can attribute each row to its file by name alone.
fn branchy(name: &str) -> String {
    format!(
        "pub fn {name}(n: i32) -> i32 {{
    if n < 0 {{ return -1; }}
    if n == 0 {{ return 0; }}
    if n < 10 {{ return 1; }}
    if n < 100 {{ return 2; }}
    3
}}
"
    )
}

/// A hermetic two-file corpus: `a.rs` holds `a_offender`, `b.rs` holds
/// `b_offender`. Both offend at `cyclomatic=1`, so a run over the
/// directory produces exactly two offender rows.
fn corpus() -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("a.rs"), branchy("a_offender")).expect("write a.rs");
    fs::write(dir.path().join("b.rs"), branchy("b_offender")).expect("write b.rs");
    dir
}

fn cli(dir: &Path) -> Command {
    common::cli_in(dir)
}

/// A `bca check` over `dir` at `cyclomatic=1`, plus `extra` flags.
fn check(dir: &TempDir, extra: &[&str]) -> (String, String, Option<i32>) {
    let out = cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            dir.path().to_str().expect("utf8 dir"),
            "--threshold",
            "cyclomatic=1",
        ])
        .args(extra)
        .output()
        .expect("bca runs");
    (
        String::from_utf8(out.stdout).expect("utf8 stdout"),
        String::from_utf8(out.stderr).expect("utf8 stderr"),
        out.status.code(),
    )
}

/// The offender rows in `text`, selected by the trailing `(limit N)`
/// that only a row carries.
///
/// The selector matters more than it looks. The summary footer renders
/// `a.rs: 1 violation (worst: cyclomatic = 5 vs limit 1 at L1)`, so the
/// obvious `contains(": cyclomatic = ")` matches the *footer* and every
/// "no rows on stderr" assertion below would fail for the wrong reason —
/// or, with the polarity flipped, pass for the wrong one. ` (limit ` is
/// the one substring the footer does not contain.
fn rows(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| line.contains(" (limit "))
        .collect()
}

/// Both halves of the contract for the default invocation: the rows are
/// on stdout, and stderr carries the footer without a single row.
#[test]
fn plain_run_puts_rows_on_stdout_and_the_footer_on_stderr() {
    let dir = corpus();
    let (stdout, stderr, code) = check(&dir, &[]);

    let stdout_rows = rows(&stdout);
    assert_eq!(
        stdout_rows.len(),
        2,
        "one row per offending function, on stdout; stdout was:\n{stdout}"
    );
    assert!(
        stdout_rows.iter().any(|r| r.contains("a_offender"))
            && stdout_rows.iter().any(|r| r.contains("b_offender")),
        "both offenders must be named on stdout; stdout was:\n{stdout}"
    );
    assert!(
        rows(&stderr).is_empty(),
        "no offender row may reach stderr; stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("--- summary ---"),
        "the footer is commentary and stays on stderr; stderr was:\n{stderr}"
    );
    assert_eq!(code, Some(2), "the gate verdict is unchanged");
}

/// `--baseline`: the covered offender is dropped and the count that says
/// so stays on stderr, while the surviving row is on stdout.
#[test]
fn baseline_filtered_run_keeps_its_diagnostic_on_stderr() {
    let dir = corpus();
    let baseline = dir.path().join("baseline.toml");

    // Record only `a.rs`, so the later two-file run has exactly one
    // covered offender and one uncovered one.
    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            dir.path().join("a.rs").to_str().expect("utf8 path"),
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().expect("utf8 path"),
        ])
        .assert()
        .success();

    let (stdout, stderr, code) =
        check(&dir, &["--baseline", baseline.to_str().expect("utf8 path")]);

    let stdout_rows = rows(&stdout);
    assert_eq!(
        stdout_rows.len(),
        1,
        "only the uncovered offender survives; stdout was:\n{stdout}"
    );
    assert!(
        stdout_rows[0].contains("b_offender"),
        "the surviving row is `b.rs`'s; stdout was:\n{stdout}"
    );
    assert!(
        rows(&stderr).is_empty(),
        "no offender row may reach stderr; stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("bca: filtered 1 violations via baseline"),
        "the filter count is a diagnostic and stays on stderr; stderr was:\n{stderr}"
    );
    assert_eq!(code, Some(2), "the gate verdict is unchanged");
}

/// `[check.exclude]`: same shape, different diagnostic.
#[test]
fn check_exclude_run_keeps_its_diagnostic_on_stderr() {
    let dir = corpus();
    let (stdout, stderr, code) = check(&dir, &["--check-exclude", "**/a.rs"]);

    let stdout_rows = rows(&stdout);
    assert_eq!(
        stdout_rows.len(),
        1,
        "only the unexcluded offender survives; stdout was:\n{stdout}"
    );
    assert!(
        stdout_rows[0].contains("b_offender"),
        "the surviving row is `b.rs`'s; stdout was:\n{stdout}"
    );
    assert!(
        rows(&stderr).is_empty(),
        "no offender row may reach stderr; stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("bca: skipped 1 violations via [check.exclude]"),
        "the skip count is a diagnostic and stays on stderr; stderr was:\n{stderr}"
    );
    assert_eq!(code, Some(2), "the gate verdict is unchanged");
}

/// #1167 is a stream change and nothing else. Anyone scripting on `$?`
/// must see no difference, in the default contract and the tiered one.
///
/// The tiered leg is the discriminating one: it exercises the
/// `--exit-codes=tiered` mapping (`NewOnly` → 2, `RegressionOnly` → 3),
/// which shares no code with the default collapse-to-2.
#[test]
fn exit_codes_are_unchanged_by_the_stream_split() {
    let dir = corpus();
    let baseline = dir.path().join("baseline.toml");

    assert_eq!(
        check(&dir, &["--threshold", "cyclomatic=100"]).2,
        Some(0),
        "a clean run exits 0"
    );
    assert_eq!(check(&dir, &[]).2, Some(2), "a breach exits 2");
    assert_eq!(
        check(&dir, &["--no-fail"]).2,
        Some(0),
        "--no-fail forces 0 while still reporting"
    );
    assert_eq!(
        check(&dir, &["--exit-codes=tiered"]).2,
        Some(2),
        "unbaselined offenders are NewOnly, which is 2 in either contract"
    );

    // Baseline both offenders, then worsen one so the tiered contract
    // has a category of its own to report.
    cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            dir.path().to_str().expect("utf8 dir"),
            "--threshold",
            "cyclomatic=1",
            "--write-baseline",
            baseline.to_str().expect("utf8 path"),
        ])
        .assert()
        .success();
    fs::write(
        dir.path().join("a.rs"),
        branchy("a_offender").replace("    3\n", "    if n < 1000 { return 3; }\n    4\n"),
    )
    .expect("worsen a.rs");

    let (stdout, stderr, code) = check(
        &dir,
        &[
            "--exit-codes=tiered",
            "--baseline",
            baseline.to_str().expect("utf8 path"),
        ],
    );
    assert_eq!(
        code,
        Some(3),
        "a lone baseline regression is RegressionOnly; stdout:\n{stdout}stderr:\n{stderr}"
    );
    assert_eq!(
        rows(&stdout).len(),
        1,
        "the regressed row is on stdout; stdout was:\n{stdout}"
    );
    assert!(
        rows(&stderr).is_empty(),
        "no offender row may reach stderr; stderr was:\n{stderr}"
    );
}

/// `--summary-file` writes a separate artifact. It is not a stream, so
/// #1167 must not move it — and its digest must stay out of *both*
/// streams.
#[test]
fn summary_file_digest_is_a_separate_artifact() {
    let dir = corpus();
    let digest = dir.path().join("summary.md");

    let (stdout, stderr, code) = check(
        &dir,
        &["--summary-file", digest.to_str().expect("utf8 path")],
    );
    assert_eq!(code, Some(2));

    let written = fs::read_to_string(&digest).expect("digest written");
    assert!(
        written.contains("bca-step-summary"),
        "the digest keeps its replace markers; got:\n{written}"
    );
    assert!(
        written.contains("a_offender") && written.contains("b_offender"),
        "the digest still names every offender; got:\n{written}"
    );
    assert_eq!(
        rows(&stdout).len(),
        2,
        "the rows still go to stdout; stdout was:\n{stdout}"
    );
    assert!(
        rows(&stderr).is_empty(),
        "no offender row may reach stderr; stderr was:\n{stderr}"
    );
    for (name, stream) in [("stdout", &stdout), ("stderr", &stderr)] {
        assert!(
            !stream.contains("bca-step-summary"),
            "the digest is a file, not a stream; {name} was:\n{stream}"
        );
    }
}

/// The one exception to the split, and the reason it exists: with
/// `--report-format` and no `--output`, the aggregated document owns
/// stdout. Mixing human rows into it would corrupt a SARIF payload, so
/// they stay on stderr for that combination alone.
#[test]
fn report_format_on_stdout_keeps_the_document_parseable() {
    let dir = corpus();
    let (stdout, stderr, code) = check(&dir, &["--report-format", "sarif"]);
    assert_eq!(code, Some(2));

    let doc: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout is an uncorrupted SARIF document");
    assert_eq!(
        doc["runs"][0]["results"]
            .as_array()
            .expect("SARIF results array")
            .len(),
        2,
        "the document still carries both offenders"
    );
    assert!(
        rows(&stdout).is_empty(),
        "no human row may be interleaved into the document; stdout was:\n{stdout}"
    );
    assert_eq!(
        rows(&stderr).len(),
        2,
        "the human rows fall back to stderr here; stderr was:\n{stderr}"
    );
}

/// `--output <file>` moves the document off stdout, so the rows go back
/// to it. Without this the exception above would read as "any
/// `--report-format` keeps the old behaviour", which is not the rule.
#[test]
fn report_format_with_output_file_puts_rows_back_on_stdout() {
    let dir = corpus();
    let out = dir.path().join("report.sarif");

    let (stdout, stderr, code) = check(
        &dir,
        &[
            "--report-format",
            "sarif",
            "--output",
            out.to_str().expect("utf8 path"),
        ],
    );
    assert_eq!(code, Some(2));

    let doc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out).expect("document written"))
            .expect("the file is a SARIF document");
    assert_eq!(
        doc["runs"][0]["results"]
            .as_array()
            .expect("SARIF results array")
            .len(),
        2,
        "the document is unaffected by the stream split"
    );
    assert_eq!(
        rows(&stdout).len(),
        2,
        "stdout is free, so the rows take it; stdout was:\n{stdout}"
    );
    assert!(
        rows(&stderr).is_empty(),
        "no offender row may reach stderr; stderr was:\n{stderr}"
    );
}
