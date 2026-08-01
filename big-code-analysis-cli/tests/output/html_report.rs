//! End-to-end integration tests for `bca report html`.
//!
//! Drives the CLI binary against a real fixture and validates the
//! emitted HTML with the same `quick-xml` walker the per-file metrics
//! HTML output uses (see `tests/common/validators.rs`).

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

use crate::common;
use common::validators::assert_html_well_formed;

fn cli() -> Command {
    common::bca_command()
}

fn fixture_path() -> String {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let workspace = std::path::Path::new(manifest)
        .parent()
        .expect("manifest dir has parent");
    workspace
        .join("tests/repositories/DeepSpeech/stats.py")
        .to_str()
        .expect("path is utf-8")
        .to_string()
}

#[test]
fn report_html_help_lists_format_top_and_strip_prefix() {
    cli()
        .args(["report", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("html"))
        .stdout(predicate::str::contains("markdown"))
        .stdout(predicate::str::contains("--top"))
        .stdout(predicate::str::contains("--strip-prefix"));
}

#[test]
fn report_html_to_stdout_is_well_formed() {
    let output = cli()
        .args(["report", "--paths", &fixture_path(), "html"])
        .output()
        .expect("invocation succeeds");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body = String::from_utf8(output.stdout).expect("output is UTF-8");
    assert!(body.contains("<!doctype html>"));
    assert!(body.contains("<h1>Code Quality Metrics Summary</h1>"));
    // Headings now carry a slug `id=` (issue #622); match text+close.
    assert!(body.contains(">Python</h2>"));
    assert!(
        body.contains("<nav class=\"toc\""),
        "report should carry a table-of-contents nav"
    );
    assert_html_well_formed(&body);
}

#[test]
fn report_html_to_file_is_well_formed() {
    let dir = TempDir::new().expect("tempdir");
    let out = dir.path().join("report.html");
    cli()
        .args([
            "report",
            "--paths",
            &fixture_path(),
            "html",
            "--output",
            out.to_str().expect("utf-8"),
        ])
        .assert()
        .success();
    let body = std::fs::read_to_string(&out).expect("read report");
    assert!(body.contains("<!doctype html>"));
    assert!(body.contains("class=\"hotspot\""));
    assert_html_well_formed(&body);
}

#[test]
fn report_html_top_zero_shows_all() {
    // Issue #602 unified `0 = all` across `vcs`/`report`/`trend`; `--top 0`
    // is now a valid "show every row" request, and the MI title says "all".
    let output = cli()
        .args(["report", "--paths", &fixture_path(), "html", "--top", "0"])
        .output()
        .expect("invocation succeeds");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body = String::from_utf8(output.stdout).expect("output is UTF-8");
    assert!(
        body.contains("Maintainability Index hotspots (all, by MI)"),
        "uncapped MI title says 'all', not 'top-0' (issue #677): {body}"
    );
    assert_html_well_formed(&body);
}

#[test]
fn report_html_with_no_matching_files_is_well_formed() {
    // Point at an empty directory so the walk resolves zero files: the
    // report must still render a well-formed shell with no hotspot
    // tables. `--no-config` keeps it hermetic from the repo's root
    // `bca.toml`. Since #596 a bare (pathless) `report` defaults to `.`
    // and would analyze the runner's cwd, so the empty-input case is now
    // expressed by an explicit empty `--paths` target rather than the
    // absence of one.
    let empty = TempDir::new().expect("tempdir");
    let output = cli()
        .args([
            "report",
            "--no-config",
            "--paths",
            empty.path().to_str().expect("utf-8"),
            "html",
        ])
        .output()
        .expect("invocation succeeds");
    assert!(output.status.success());
    let body = String::from_utf8(output.stdout).expect("utf-8");
    assert!(body.contains("<h1>Code Quality Metrics Summary</h1>"));
    // No tables when there is nothing to report.
    assert!(!body.contains("class=\"hotspot\""));
    assert_html_well_formed(&body);
}

#[test]
fn report_html_is_deterministic_across_runs() {
    let fp = fixture_path();
    let outputs: Vec<Vec<u8>> = (0..3)
        .map(|_| {
            cli()
                .args(["report", "--paths", &fp, "html"])
                .output()
                .expect("invocation")
                .stdout
        })
        .collect();
    assert!(
        outputs.windows(2).all(|w| w[0] == w[1]),
        "non-deterministic HTML report across 3 runs"
    );
}

#[test]
fn report_html_strip_prefix_removes_path_prefix() {
    let fp = fixture_path();
    let prefix = {
        let idx = fp
            .find("DeepSpeech/")
            .expect("fixture contains DeepSpeech/");
        &fp[..idx]
    };
    let output = cli()
        .args(["report", "--paths", &fp, "html", "--strip-prefix", prefix])
        .output()
        .expect("invocation");
    assert!(output.status.success());
    let body = String::from_utf8(output.stdout).expect("utf-8");
    assert!(
        body.contains("DeepSpeech/stats.py"),
        "stripped path should appear in HTML report"
    );
    // `--strip-prefix` rewrites the per-file table paths only. The provenance
    // footer (issue #680) deliberately records the literal seed path the user
    // passed (what was scanned), which here is the full `fp`; exclude that
    // footer from the "stripped everywhere" check.
    let before_footer = body
        .split_once("<footer class=\"provenance\">")
        .map_or(body.as_str(), |(head, _)| head);
    assert!(
        !before_footer.contains(&fp),
        "full unstripped path should not appear in the report body: {fp}"
    );
    assert_html_well_formed(&body);
}
