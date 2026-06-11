#![allow(missing_docs)]
//! Regression coverage for the 2.0 CLI output-semantics fixes.
//!
//! - #663: an *explicitly-named* file with an unrecognized language warns
//!   on stderr and exits 1 when it is the sole input that produced
//!   nothing, while a directory-discovered file with the same extension
//!   stays silently skipped (exit 0).
//! - #669: `--output <FILE>` means one aggregate file everywhere;
//!   `metrics`/`ops` gain `--output-dir <DIR>` for the per-file tree;
//!   passing both is an error.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

mod common;

fn cli() -> Command {
    common::bca_command()
}

/// Recursively count `*.json` files under `dir`, accumulating into
/// `count`. Used to assert the per-file-tree (`--output-dir`) shape.
fn count_json_files(dir: &std::path::Path, count: &mut usize) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            count_json_files(&p, count);
        } else if p.extension().and_then(|x| x.to_str()) == Some("json") {
            *count += 1;
        }
    }
}

// ---------------------------------------------------------------------
// #663 — explicit unrecognized-language file warns + exits 1
// ---------------------------------------------------------------------

/// The key distinction the issue calls out: an explicitly-named
/// unrecognized file warns + exits 1, but the *same file* discovered by
/// walking its containing directory stays silently skipped (exit 0).
#[test]
fn explicit_unrecognized_exits_one_but_dir_walk_stays_silent() {
    let dir = TempDir::new().unwrap();
    let notes = dir.path().join("notes.unknownlang999");
    std::fs::write(&notes, "free-form text, no parser\n").unwrap();

    // Explicitly named -> warning + exit 1 (no output produced at all).
    cli()
        .args(["metrics", notes.to_str().unwrap()])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "skipping explicitly-named file with unrecognized language:",
        ));

    // The same file reached only by a directory walk: silent, exit 0.
    cli()
        .args(["metrics", "--paths", dir.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

/// A mixed run (one recognized file + one explicit unrecognized file)
/// still succeeds (exit 0) but surfaces the per-file warning — the user
/// got *some* output, so the run is not a total failure.
#[test]
fn explicit_unrecognized_mixed_with_recognized_warns_but_exits_zero() {
    let dir = TempDir::new().unwrap();
    let good = dir.path().join("lib.rs");
    let bad = dir.path().join("notes.unknownlang999");
    std::fs::write(&good, "fn main() {}\n").unwrap();
    std::fs::write(&bad, "free-form text\n").unwrap();

    cli()
        .args([
            "metrics",
            "-O",
            "json",
            good.to_str().unwrap(),
            bad.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "skipping explicitly-named file with unrecognized language:",
        ));
}

/// `--language` is the escape hatch: forcing a parser on an
/// extension-less / mislabeled file parses it and exits 0.
#[test]
fn explicit_unrecognized_parses_under_forced_language() {
    let dir = TempDir::new().unwrap();
    let bad = dir.path().join("notes.unknownlang999");
    std::fs::write(&bad, "fn main() { let x = 1; }\n").unwrap();

    cli()
        .args(["metrics", "--language", "rust", bad.to_str().unwrap()])
        .assert()
        .success();
}

/// The same rule applies to `ops`, the sibling structured command.
#[test]
fn ops_explicit_unrecognized_exits_one() {
    let dir = TempDir::new().unwrap();
    let bad = dir.path().join("notes.unknownlang999");
    std::fs::write(&bad, "free-form text\n").unwrap();

    cli()
        .args(["ops", "-O", "json", bad.to_str().unwrap()])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "skipping explicitly-named file with unrecognized language:",
        ));
}

// ---------------------------------------------------------------------
// #669 — unified --output (file) / --output-dir (tree) semantics
// ---------------------------------------------------------------------

/// `--output <FILE>` writes ONE aggregate document (a top-level JSON
/// array of the per-file results) — not a directory named after the
/// file's extension, which was the surprising pre-2.0 behavior.
#[test]
fn output_writes_single_aggregate_file() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    std::fs::write(dir.path().join("b.rs"), "fn b() { let x = 1; }\n").unwrap();
    let out = dir.path().join("metrics.json");

    cli()
        .args([
            "metrics",
            "--paths",
            dir.path().to_str().unwrap(),
            "-O",
            "json",
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(out.is_file(), "--output must produce a regular file");
    let doc: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&out).unwrap()).expect("aggregate is valid JSON");
    let arr = doc.as_array().expect("aggregate is a top-level array");
    assert_eq!(arr.len(), 2, "one element per analyzed file");
    for elem in arr {
        assert!(
            elem.get("metrics").is_some() && elem.get("name").is_some(),
            "each element is a metrics record: {elem}"
        );
    }
}

/// `--output-dir <DIR>` writes the per-file tree (the historical
/// `--output` directory mode): one document per input file, named by the
/// input path plus the format extension.
#[test]
fn output_dir_writes_per_file_tree() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    std::fs::write(dir.path().join("b.rs"), "fn b() {}\n").unwrap();
    let out_dir = dir.path().join("out");

    cli()
        .args([
            "metrics",
            "--paths",
            dir.path().to_str().unwrap(),
            "-O",
            "json",
            "--output-dir",
            out_dir.to_str().unwrap(),
        ])
        .assert()
        .success();

    let mut count = 0;
    count_json_files(&out_dir, &mut count);
    assert_eq!(count, 2, "one per-file JSON document per analyzed file");
}

/// `--output` and `--output-dir` together is a usage error (exit 1):
/// exactly one destination may be chosen.
#[test]
fn output_and_output_dir_together_error() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();

    cli()
        .args([
            "metrics",
            "--paths",
            dir.path().to_str().unwrap(),
            "-O",
            "json",
            "--output",
            dir.path().join("agg.json").to_str().unwrap(),
            "--output-dir",
            dir.path().join("tree").to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("mutually exclusive"));
}

/// `ops --output <FILE>` writes a single aggregate array too — the
/// unification is consistent across the structured commands.
#[test]
fn ops_output_writes_single_aggregate_file() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    let out = dir.path().join("ops.json");

    cli()
        .args([
            "ops",
            "--paths",
            dir.path().to_str().unwrap(),
            "-O",
            "json",
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(out.is_file(), "ops --output must produce a regular file");
    let doc: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&out).unwrap()).expect("aggregate is valid JSON");
    assert!(doc.is_array(), "ops aggregate is a top-level array");
}

/// `--output` without a structured format errors (#661 coordination):
/// the default text stream writes no files, so a destination under it
/// would silently no-op.
#[test]
fn output_without_format_errors() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();

    cli()
        .args([
            "metrics",
            "--paths",
            dir.path().to_str().unwrap(),
            "--output",
            dir.path().join("o.json").to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("needs a structured format"));
}
