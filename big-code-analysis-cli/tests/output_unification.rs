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
    // `is_array()` alone is satisfied by `[]` — the canonical signature
    // of a dropped-records regression (#912). Pin the record count (one
    // fixture analyzed) and each element's real ops shape, mirroring the
    // metrics sibling `output_writes_single_aggregate_file`.
    let arr = doc.as_array().expect("ops aggregate is a top-level array");
    assert_eq!(arr.len(), 1, "one element per analyzed file");
    for elem in arr {
        assert!(
            elem.get("name").is_some()
                && elem.get("operands").is_some()
                && elem.get("operators").is_some(),
            "each element is an ops record (name + operands + operators): {elem}"
        );
    }
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

// ---------------------------------------------------------------------
// #1115 — every file destination writes through a buffer
// ---------------------------------------------------------------------

/// A Rust source whose metric document comfortably exceeds the 64 KiB
/// output buffer in every format, so the written file spans several
/// buffer fills rather than landing in one.
///
/// Each function is distinct so the *last* one can be looked for by
/// name: a document that lost its final partial buffer parses as
/// truncated JSON but would still contain the earlier functions, which
/// is exactly the failure a small fixture cannot distinguish.
fn find_single_file(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let found = if path.is_dir() {
            find_single_file(&path)
        } else {
            Some(path)
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

fn many_functions(count: usize) -> String {
    use std::fmt::Write as _;

    let mut src = String::new();
    for i in 0..count {
        let _ = writeln!(
            src,
            "pub fn generated_function_{i}(n: i32) -> i32 {{\n    \
             if n > {i} {{ n + {i} }} else {{ n - {i} }}\n}}"
        );
    }
    src
}

/// Every per-file format must write a complete document when it is
/// larger than the buffer placed in front of the destination (#1115).
///
/// The buffered writers flush explicitly rather than leaving it to
/// `Drop`; a document that fits in one buffer cannot tell a working
/// flush from a broken one on the *tail*, because a broken flush loses
/// everything and any assertion fails. This fixture is deliberately
/// several buffers long so a lost final partial buffer — the realistic
/// truncation — is what fails.
#[test]
fn per_file_documents_survive_spanning_several_output_buffers() {
    const FUNCTIONS: usize = 400;
    const MIN_MULTI_BUFFER_BYTES: u64 = 64 * 1_024;

    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("big.rs"), many_functions(FUNCTIONS)).unwrap();

    for format in ["json", "yaml", "toml", "cbor"] {
        let out_dir = dir.path().join(format!("out-{format}"));
        cli()
            .args([
                "metrics",
                "--paths",
                dir.path().join("big.rs").to_str().unwrap(),
                "-O",
                format,
                "--output-dir",
                out_dir.to_str().unwrap(),
            ])
            .assert()
            .success();

        let written = find_single_file(&out_dir)
            .unwrap_or_else(|| panic!("{format}: --output-dir produced no file"));
        let bytes = std::fs::read(&written).unwrap();

        assert!(
            bytes.len() as u64 > MIN_MULTI_BUFFER_BYTES,
            "{format}: fixture must exceed one output buffer to be a \
             meaningful truncation test, got {} bytes",
            bytes.len()
        );

        // Every format must parse end to end. A truncated tail fails
        // here for JSON/YAML/TOML/CBOR alike, since all four are
        // self-delimiting documents.
        let last = format!("generated_function_{}", FUNCTIONS - 1);
        let parsed_names = match format {
            "json" => {
                let v: serde_json::Value = serde_json::from_slice(&bytes)
                    .unwrap_or_else(|e| panic!("{format}: truncated or invalid document: {e}"));
                v.to_string()
            }
            "yaml" => {
                let v: serde_yaml::Value = serde_yaml::from_slice(&bytes)
                    .unwrap_or_else(|e| panic!("{format}: truncated or invalid document: {e}"));
                serde_json::to_string(&v).unwrap()
            }
            "toml" => {
                let text = std::str::from_utf8(&bytes).expect("toml is utf-8");
                let v: toml::Table = toml::from_str(text)
                    .unwrap_or_else(|e| panic!("{format}: truncated or invalid document: {e}"));
                serde_json::to_string(&v).unwrap()
            }
            _ => {
                let v: ciborium::Value = ciborium::from_reader(&bytes[..])
                    .unwrap_or_else(|e| panic!("{format}: truncated or invalid document: {e}"));
                format!("{v:?}")
            }
        };

        assert!(
            parsed_names.contains(&last),
            "{format}: the final function is missing — the tail of the \
             document was lost"
        );
    }
}

/// The aggregate `--output <FILE>` destination is the one the issue
/// measured at 4.76 million `write(2)` calls for a 12 MB document; it
/// must still produce one complete, parseable file.
#[test]
fn aggregate_document_survives_spanning_several_output_buffers() {
    const FUNCTIONS: usize = 400;

    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("big.rs"), many_functions(FUNCTIONS)).unwrap();
    let out = dir.path().join("agg.json");

    cli()
        .args([
            "metrics",
            "--paths",
            dir.path().join("big.rs").to_str().unwrap(),
            "-O",
            "json",
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let bytes = std::fs::read(&out).expect("aggregate written");
    assert!(
        bytes.len() > 64 * 1_024,
        "fixture must span several buffers"
    );
    let doc: serde_json::Value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|e| panic!("aggregate truncated or invalid: {e}"));
    let arr = doc.as_array().expect("aggregate is a top-level array");
    assert_eq!(arr.len(), 1, "one element per analyzed file");
    assert!(
        doc.to_string()
            .contains(&format!("generated_function_{}", FUNCTIONS - 1)),
        "the tail of the aggregate document was lost"
    );
}
