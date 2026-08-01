//! Every walk result channel must deliver every record, whatever the
//! job count (#1119).
//!
//! The walk streams its per-file results back over four channels hung
//! off `Config` — `check_tx`, `markdown_tx`, `exemptions_tx`, and the
//! `aggregate_tx` shared by `metrics` / `ops`. #1119 replaced
//! `Mutex<std::sync::mpsc::Sender<_>>` with a bare
//! `crossbeam::channel::Sender<_>`, which is `Sync` and so needs no
//! per-file lock. That rewires the send path of all four.
//!
//! Each test below runs the same corpus serially and at high
//! concurrency and demands byte-identical output. A lost send under
//! contention, a sender clone that outlives the walk (the receiver
//! would never see disconnect and the run would hang), or an ordering
//! dependency that only appears with many workers all fail here — and
//! none of them are visible in a single-threaded run, which is what the
//! rest of the suite exercises.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use tempfile::TempDir;

mod common;

fn cli(dir: &Path) -> Command {
    common::cli_in(dir)
}

/// Enough distinct files that the pool genuinely interleaves, each with
/// a branchy function so `check` has something to report. Bodies differ
/// per index so no two files collapse to the same output.
fn corpus(dir: &TempDir) -> String {
    let root = dir.path().join("src");
    fs::create_dir_all(&root).expect("create corpus dir");
    for i in 0..120 {
        let body = format!(
            "pub fn classify_{i}(n: i32) -> &'static str {{\n\
             \x20   if n < {i} {{\n\
             \x20       \"low\"\n\
             \x20   }} else if n == {i} {{\n\
             \x20       \"exact\"\n\
             \x20   }} else if n < {} {{\n\
             \x20       \"mid\"\n\
             \x20   }} else {{\n\
             \x20       \"high\"\n\
             \x20   }}\n\
             }}\n",
            i + 100,
        );
        fs::write(root.join(format!("f{i}.rs")), body).expect("write fixture");
    }
    root.to_str().expect("utf8 corpus path").to_owned()
}

/// Run `bca` with `args` plus an explicit `--jobs`, returning
/// `(stdout, stderr)` as text.
fn run_at_jobs(dir: &TempDir, jobs: &str, args: &[&str]) -> (String, String) {
    let mut cmd = cli(dir.path());
    cmd.args(args).args(["--jobs", jobs]);
    let out = cmd.output().expect("bca runs");
    (
        String::from_utf8(out.stdout).expect("utf8 stdout"),
        String::from_utf8(out.stderr).expect("utf8 stderr"),
    )
}

/// Sorted lines, so a comparison isolates *content* loss from the
/// arrival-order nondeterminism a parallel walk is entitled to.
fn sorted_lines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.lines().collect();
    lines.sort_unstable();
    lines
}

/// `check_tx`: one `Violation` per offending function, many per file.
#[test]
fn check_reports_the_same_violations_serially_and_in_parallel() {
    let dir = TempDir::new().unwrap();
    let root = corpus(&dir);
    let args = [
        "check",
        "--no-config",
        "--paths",
        &root,
        "--threshold",
        "cyclomatic=1",
        "--no-fail",
        "--no-summary",
    ];

    let (_, serial) = run_at_jobs(&dir, "1", &args);
    let (_, parallel) = run_at_jobs(&dir, "16", &args);

    // 120 files, one offending function each: a dropped send shows up
    // as a short count before the line comparison even runs. Count only
    // offender lines — stderr also carries the trailing remediation
    // block, which `--no-summary` does not suppress.
    let offenders = |text: &str| {
        text.lines()
            .filter(|l| l.contains(": cyclomatic = "))
            .count()
    };
    assert_eq!(
        offenders(&serial),
        120,
        "fixture drifted; expected one offender per file:\n{serial}"
    );
    assert_eq!(
        sorted_lines(&serial),
        sorted_lines(&parallel),
        "check violations differ between --jobs 1 and --jobs 16"
    );
}

/// `aggregate_tx`: `metrics --output <FILE>` streams every file's space
/// to a post-walk collector that writes one document.
#[test]
fn metrics_aggregate_output_is_identical_serially_and_in_parallel() {
    let dir = TempDir::new().unwrap();
    let root = corpus(&dir);

    let collect = |jobs: &str, name: &str| -> String {
        let out = dir.path().join(name);
        let out_str = out.to_str().expect("utf8 output path").to_owned();
        run_at_jobs(
            &dir,
            jobs,
            &[
                "metrics",
                "--no-config",
                "--paths",
                &root,
                "--output",
                &out_str,
                "--format",
                "json",
            ],
        );
        fs::read_to_string(&out).expect("aggregate document written")
    };

    let serial = collect("1", "serial.json");
    let parallel = collect("16", "parallel.json");

    // The aggregate is one JSON document; compare it structurally so a
    // legitimate difference in file arrival order is not mistaken for a
    // lost record.
    let as_set = |text: &str| -> Vec<String> {
        let v: serde_json::Value = serde_json::from_str(text).expect("valid JSON aggregate");
        let mut items: Vec<String> = v
            .as_array()
            .expect("aggregate is a JSON array")
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        items.sort();
        items
    };
    let serial_items = as_set(&serial);
    assert_eq!(
        serial_items.len(),
        120,
        "expected one aggregate record per file"
    );
    assert_eq!(
        serial_items,
        as_set(&parallel),
        "metrics aggregate differs between --jobs 1 and --jobs 16"
    );
}

/// `markdown_tx`: one `FunctionSummary` per function, streamed and then
/// rendered into the report body after the walk.
#[test]
fn report_body_is_identical_serially_and_in_parallel() {
    let dir = TempDir::new().unwrap();
    let root = corpus(&dir);
    let args = ["report", "--no-config", "--paths", &root];

    let (serial, _) = run_at_jobs(&dir, "1", &args);
    let (parallel, _) = run_at_jobs(&dir, "16", &args);

    assert!(
        serial.contains("classify_0"),
        "fixture drifted; report names no fixture function:\n{serial}"
    );
    assert_eq!(
        sorted_lines(&serial),
        sorted_lines(&parallel),
        "report body differs between --jobs 1 and --jobs 16"
    );
}

/// `exemptions_tx`: one batch per file that carries a marker, skipped
/// entirely for files that carry none — so the fixture seeds markers in
/// a subset, exercising both arms under contention.
#[test]
fn exemptions_audit_is_identical_serially_and_in_parallel() {
    let dir = TempDir::new().unwrap();
    let root = corpus(&dir);
    // Every third file gets a marker; the rest take the early return
    // that never touches the channel.
    for i in (0..120).step_by(3) {
        let path = Path::new(&root).join(format!("f{i}.rs"));
        let body = fs::read_to_string(&path).expect("read fixture");
        let marked = body.replace(
            " -> &'static str {",
            " -> &'static str {\n    // bca: suppress(cyclomatic)",
        );
        assert_ne!(body, marked, "marker insertion found no anchor");
        fs::write(&path, marked).expect("write marked fixture");
    }
    let args = ["exemptions", "--no-config", "--paths", &root];

    let (serial, _) = run_at_jobs(&dir, "1", &args);
    let (parallel, _) = run_at_jobs(&dir, "16", &args);

    assert!(
        serial.contains("f0.rs"),
        "fixture drifted; audit lists no marked file:\n{serial}"
    );
    assert_eq!(
        sorted_lines(&serial),
        sorted_lines(&parallel),
        "exemptions audit differs between --jobs 1 and --jobs 16"
    );
}
