use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn cli() -> Command {
    Command::cargo_bin("big-code-analysis-cli").unwrap()
}

/// A small fixture file known to the repo, resolved relative to the workspace
/// root so the path is valid regardless of the test runner's CWD.
fn fixture_path() -> String {
    let manifest = env!("CARGO_MANIFEST_DIR"); // .../big-code-analysis-cli
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
fn help_lists_markdown_and_new_flags() {
    cli()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("markdown"))
        .stdout(predicate::str::contains("--top"))
        .stdout(predicate::str::contains("--strip-prefix"));
}

#[test]
fn markdown_rejects_ops() {
    let fp = fixture_path();
    cli()
        .args(["--paths", &fp, "-O", "markdown", "--ops"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "-O markdown is incompatible with --ops",
        ));
}

#[test]
fn markdown_rejects_dump() {
    let fp = fixture_path();
    cli()
        .args(["--paths", &fp, "-O", "markdown", "--dump"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "-O markdown is incompatible with --dump",
        ));
}

#[test]
fn markdown_rejects_comments() {
    let fp = fixture_path();
    cli()
        .args(["--paths", &fp, "-O", "markdown", "--comments"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "-O markdown is incompatible with --comments",
        ));
}

#[test]
fn markdown_rejects_function() {
    let fp = fixture_path();
    cli()
        .args(["--paths", &fp, "-O", "markdown", "--function"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "-O markdown is incompatible with --function",
        ));
}

#[test]
fn markdown_rejects_find() {
    let fp = fixture_path();
    cli()
        .args(["--paths", &fp, "-O", "markdown", "--find", "identifier"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "-O markdown is incompatible with --find",
        ));
}

#[test]
fn markdown_requires_metrics() {
    let fp = fixture_path();
    cli()
        .args(["--paths", &fp, "-O", "markdown"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("-O markdown requires --metrics"));
}

#[test]
fn top_zero_rejected() {
    let fp = fixture_path();
    cli()
        .args(["--metrics", "--paths", &fp, "-O", "markdown", "--top", "0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--top"));
}

#[test]
fn markdown_rejects_count() {
    let fp = fixture_path();
    cli()
        .args([
            "--paths",
            &fp,
            "-O",
            "markdown",
            "--metrics",
            "-C",
            "identifier",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "-O markdown is incompatible with --count",
        ));
}

#[test]
fn output_dir_rejected_for_markdown() {
    let fp = fixture_path();
    let dir = TempDir::new().unwrap();
    cli()
        .args([
            "--metrics",
            "--paths",
            &fp,
            "-O",
            "markdown",
            "--output",
            dir.path().to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--output must be a file path when -O markdown is used",
        ));
}

#[test]
fn output_parent_missing_rejected() {
    let fp = fixture_path();
    cli()
        .args([
            "--metrics",
            "--paths",
            &fp,
            "-O",
            "markdown",
            "--output",
            "/tmp/nonexistent_dir_12345/report.md",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "parent directory of --output does not exist",
        ));
}

#[test]
fn markdown_summary_count_to_stdout() {
    let fp = fixture_path();
    cli()
        .args(["--metrics", "--paths", &fp, "-O", "markdown"])
        .assert()
        .success()
        .stdout(predicate::str::contains("# Code Quality Metrics Summary"))
        .stdout(predicate::str::contains("**Files analyzed:** 1"));
}

#[test]
fn markdown_summary_count_to_file() {
    let fp = fixture_path();
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("report.md");
    cli()
        .args([
            "--metrics",
            "--paths",
            &fp,
            "-O",
            "markdown",
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let content = std::fs::read_to_string(&out).unwrap();
    assert!(
        content.contains("# Code Quality Metrics Summary"),
        "expected markdown report header in file output",
    );
    assert!(
        content.contains("**Files analyzed:** 1"),
        "expected file count in report",
    );
    assert!(
        content.contains("## Python"),
        "expected per-language section for Python",
    );
}

#[test]
fn markdown_collects_nonzero_summaries() {
    let fp = fixture_path();
    cli()
        .args(["--metrics", "--paths", &fp, "-O", "markdown"])
        .assert()
        .success()
        .stdout(predicate::str::contains("**Functions/methods:** 0").not())
        .stdout(predicate::str::contains("**Functions/methods:**"))
        .stdout(predicate::str::contains("## Per-language overview"));
}

#[test]
fn markdown_empty_paths_produces_zero_summaries() {
    cli()
        .args(["--metrics", "-O", "markdown"])
        .assert()
        .success()
        .stdout(predicate::str::contains("**Files analyzed:** 0"));
}

#[test]
fn markdown_determinism_five_runs() {
    let fp = fixture_path();
    let outputs: Vec<Vec<u8>> = (0..5)
        .map(|_| {
            cli()
                .args(["--metrics", "--paths", &fp, "-O", "markdown"])
                .output()
                .unwrap()
                .stdout
        })
        .collect();
    assert!(
        outputs.windows(2).all(|w| w[0] == w[1]),
        "non-deterministic output across 5 runs"
    );
}

#[test]
fn cbor_without_output_rejects_cleanly() {
    let fp = fixture_path();
    cli()
        .args(["--metrics", "--paths", &fp, "-O", "cbor"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "CBOR is binary and cannot be printed to stdout; use --output",
        ));
}

#[test]
fn markdown_strip_prefix_removes_path_prefix() {
    let fp = fixture_path();
    // Strip everything up to and including "DeepSpeech/" so only "stats.py" remains.
    let prefix = {
        let idx = fp
            .find("DeepSpeech/")
            .expect("fixture contains DeepSpeech/");
        &fp[..idx]
    };
    let output = cli()
        .args([
            "--metrics",
            "--paths",
            &fp,
            "-O",
            "markdown",
            "--strip-prefix",
            prefix,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The stripped path should appear (prefix removed).
    assert!(
        stdout.contains("DeepSpeech/stats.py"),
        "stripped path should appear in report"
    );
    // The full absolute fixture path should NOT appear.
    assert!(
        !stdout.contains(&fp),
        "full path should be stripped from report, but found: {fp}"
    );
}
