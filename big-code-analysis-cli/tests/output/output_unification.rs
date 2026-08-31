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
//! - #1115: every file destination is written through a buffer that is
//!   explicitly flushed, so a document larger than the buffer is
//!   complete on disk rather than missing its tail.
//! - #1244: the aggregate document's elements are emitted in sorted
//!   emitted-path order rather than worker-completion order, so two
//!   runs over an unchanged tree produce byte-identical files.
//! - #1303: the same for the *streaming* stdout destination, which
//!   cannot sort after the fact — its documents are held in a reorder
//!   buffer and released in walk order.
//! - #1304: `preproc --output` closes the same family. Its document is
//!   not an aggregate of per-file records but one `HashMap` of
//!   `HashSet`s, so neither of the two fixes above reaches it; the
//!   ordering is imposed at the library's serialization seam instead.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

use crate::common;

fn cli() -> Command {
    common::bca_command()
}

/// Bytes buffered in front of every output destination, mirroring
/// `formats::OUTPUT_BUFFER_BYTES`. A fixture larger than this spans
/// several buffer fills, which is what makes a lost final partial
/// buffer observable.
const OUTPUT_BUFFER_BYTES: u64 = 64 * 1_024;

/// Every file under `dir`, recursively, in directory order.
///
/// Backs both the per-file-tree (`--output-dir`) shape assertions and
/// the "find the one document that was written" lookups; those were two
/// separate hand-rolled `read_dir` walks.
fn files_under(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files_under(&path));
        } else {
            found.push(path);
        }
    }
    found
}

/// The `*.json` subset of [`files_under`].
fn json_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    files_under(dir)
        .into_iter()
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json"))
        .collect()
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
    // The elements are ordered by emitted path, not by whichever worker
    // finished first (#1244). Two files can pair up in the arrival order
    // by chance, so the discriminating case is the many-file fixture in
    // `aggregate_elements_are_ordered_by_path_not_worker_arrival`; this
    // is the cheap guard on the shape everyone reads.
    assert_eq!(
        aggregate_names(&doc),
        [
            dir.path().join("a.rs").to_str().unwrap(),
            dir.path().join("b.rs").to_str().unwrap(),
        ],
        "aggregate elements must be emitted in sorted emitted-path order"
    );
}

/// Read the `name` of every element of a JSON aggregate, positionally.
/// Deliberately order-preserving: the order *is* the property under
/// test in the #1244 assertions below.
fn aggregate_names(doc: &serde_json::Value) -> Vec<&str> {
    doc.as_array()
        .expect("aggregate is a top-level array")
        .iter()
        .map(|elem| {
            elem.get("name")
                .and_then(serde_json::Value::as_str)
                .expect("every element carries a name")
        })
        .collect()
}

/// Write `FILES` one-function sources under `dir` and return their
/// paths in the order the aggregate must emit them.
///
/// The expectation is sorted as `PathBuf`s, not as strings: `Path`
/// compares component-wise (`a/b` sorts before `a-x/c`, the reverse of
/// the bytewise answer), and production sorts paths. A string-sorted
/// expectation agrees only as long as every fixture file sits in one
/// flat directory, and would fail spuriously the day one does not.
fn write_ordering_fixture(dir: &std::path::Path, files: usize) -> Vec<String> {
    let mut expected: Vec<std::path::PathBuf> = Vec::with_capacity(files);
    for i in 0..files {
        // `m10.rs` sorts before `m2.rs`, so the numeric and lexicographic
        // orders disagree — an assertion that accidentally described
        // creation order would not match.
        let file = dir.join(format!("m{i}.rs"));
        std::fs::write(&file, format!("fn f{i}() {{ let x = {i}; }}\n")).unwrap();
        expected.push(file);
    }
    expected.sort_unstable();
    expected
        .iter()
        .map(|p| p.to_str().unwrap().to_owned())
        .collect()
}

/// Run `command --output <FILE>` as JSON over `dir` at `--jobs 8` and
/// return the emitted document.
fn aggregate_at_eight_jobs(
    dir: &std::path::Path,
    command: &str,
    out: &std::path::Path,
) -> serde_json::Value {
    cli()
        .args([
            command,
            "--paths",
            dir.to_str().unwrap(),
            "-O",
            "json",
            "--output",
            out.to_str().unwrap(),
            "--jobs",
            "8",
        ])
        .assert()
        .success();

    serde_json::from_slice(&std::fs::read(out).unwrap()).expect("aggregate is valid JSON")
}

/// Enough files that worker completion order reliably diverges from the
/// walk's sorted file list at `--jobs 8`. Measured against the pre-#1303
/// binary: ten `metrics -O json` runs over 24 files gave ten distinct
/// stdout hashes.
const ORDERING_FIXTURE_FILES: usize = 24;

/// Identical runs compared byte for byte by the #1303 determinism
/// assertions. Three is enough to fail every pre-fix permutation
/// measured; the assertions also pin the *order*, so a run that happens
/// to repeat itself cannot pass by luck.
const DETERMINISM_RUNS: usize = 3;

/// Workers finish out of order, so before #1244 this fixture emitted a
/// different permutation on every run — twelve measured runs gave
/// twelve distinct orders and not one of them sorted. The aggregate is
/// now sorted by emitted path before serialization, which makes a
/// single run's order the whole assertion: no repeat-and-compare, which
/// could pass on scheduling luck.
#[test]
fn aggregate_elements_are_ordered_by_path_not_worker_arrival() {
    let dir = TempDir::new().unwrap();
    let expected = write_ordering_fixture(dir.path(), ORDERING_FIXTURE_FILES);
    let out = dir.path().join("agg.json");

    let doc = aggregate_at_eight_jobs(dir.path(), "metrics", &out);

    assert_eq!(
        aggregate_names(&doc),
        expected,
        "aggregate elements must be emitted in sorted emitted-path order"
    );
}

/// The `ops` half of the same contract, end to end. `AggregateItem::Ops`
/// carries a `PathBuf` for one reason — it is the sort key — and only a
/// multi-file `ops --output` run exercises the wiring that puts it
/// there: with the walk's path dropped at the send site, every unit test
/// above still passes because they construct the items by hand.
#[test]
fn ops_aggregate_elements_are_ordered_by_path_not_worker_arrival() {
    let dir = TempDir::new().unwrap();
    let expected = write_ordering_fixture(dir.path(), ORDERING_FIXTURE_FILES);
    let out = dir.path().join("ops.json");

    let doc = aggregate_at_eight_jobs(dir.path(), "ops", &out);

    assert_eq!(
        aggregate_names(&doc),
        expected,
        "ops aggregate elements must be emitted in sorted emitted-path order"
    );
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

    assert_eq!(
        json_files(&out_dir).len(),
        2,
        "one per-file JSON document per analyzed file"
    );
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

/// A Rust source whose metric document comfortably exceeds
/// [`OUTPUT_BUFFER_BYTES`] in every format, so the written file spans
/// several buffer fills rather than landing in one.
///
/// Each function is distinct so the *last* one can be looked for by
/// name: a document that lost its final partial buffer parses as
/// truncated JSON but would still contain the earlier functions, which
/// is exactly the failure a small fixture cannot distinguish.
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

        // Exactly one input file, so exactly one document — asserting
        // the count rather than picking the first entry also catches a
        // stray sibling the writer never should have created.
        let written = files_under(&out_dir);
        assert_eq!(
            written.len(),
            1,
            "{format}: --output-dir produced {written:?} for one input"
        );
        let bytes = std::fs::read(&written[0]).unwrap();

        assert!(
            bytes.len() as u64 > OUTPUT_BUFFER_BYTES,
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
        bytes.len() as u64 > OUTPUT_BUFFER_BYTES,
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

// ---------------------------------------------------------------------
// #1303 — stdout emits its documents in walk order
// ---------------------------------------------------------------------

/// The stdout of a `metrics` / `ops` run over `dir` at `--jobs 8`,
/// verbatim. The bytes are the property under test, so nothing here
/// parses, sorts, or normalizes them.
fn stdout_at_eight_jobs(dir: &std::path::Path, command: &str) -> Vec<u8> {
    let assert = cli()
        .args([
            command,
            "--paths",
            dir.to_str().unwrap(),
            "-O",
            "json",
            "--jobs",
            "8",
        ])
        // A reorder buffer that stalls waiting for a slot nobody
        // releases would otherwise hang the suite rather than fail it.
        .timeout(std::time::Duration::from_secs(120))
        .assert()
        .success();

    assert.get_output().stdout.clone()
}

/// The `name` of every document on a stream of one JSON document per
/// line, positionally — the order *is* the property under test.
fn streamed_names(stdout: &[u8]) -> Vec<String> {
    std::str::from_utf8(stdout)
        .expect("stdout is UTF-8")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let doc: serde_json::Value =
                serde_json::from_str(line).expect("each line is one JSON document");
            doc.get("name")
                .and_then(serde_json::Value::as_str)
                .expect("every document carries a name")
                .to_owned()
        })
        .collect()
}

/// Stdout streams each document as its worker finishes, so before #1303
/// this fixture emitted a different permutation on every run — ten
/// measured runs at `--jobs 8` gave ten distinct byte hashes, and not
/// one of them was in walk order. Both halves are asserted: a
/// stability-only test passes if every run is identically wrong, and an
/// order-only test cannot see a run that reorders under a different
/// schedule.
#[test]
fn stdout_documents_are_ordered_by_walk_order_not_worker_arrival() {
    let dir = TempDir::new().unwrap();
    let expected = write_ordering_fixture(dir.path(), ORDERING_FIXTURE_FILES);

    let first = stdout_at_eight_jobs(dir.path(), "metrics");

    assert_eq!(
        streamed_names(&first),
        expected,
        "stdout documents must be emitted in the walk's file order"
    );
    for run in 1..DETERMINISM_RUNS {
        assert_eq!(
            stdout_at_eight_jobs(dir.path(), "metrics"),
            first,
            "run {run} produced different bytes on an unchanged tree"
        );
    }
}

/// The `ops` half of the same contract. `ops` renders a different
/// document through the same emitter, so a fix wired into only the
/// `metrics` dispatch would pass the test above and fail here.
#[test]
fn ops_stdout_documents_are_ordered_by_walk_order_not_worker_arrival() {
    let dir = TempDir::new().unwrap();
    let expected = write_ordering_fixture(dir.path(), ORDERING_FIXTURE_FILES);

    let first = stdout_at_eight_jobs(dir.path(), "ops");

    assert_eq!(
        streamed_names(&first),
        expected,
        "ops stdout documents must be emitted in the walk's file order"
    );
    for run in 1..DETERMINISM_RUNS {
        assert_eq!(
            stdout_at_eight_jobs(dir.path(), "ops"),
            first,
            "run {run} produced different bytes on an unchanged tree"
        );
    }
}

/// Files that produce no document at all — empty, generated, or of an
/// unrecognized language — still occupy a slot in the emission order,
/// and every one of them sorts *between* two files that do produce one.
/// If any of them failed to release its slot the drain would stall
/// behind it and the run would hang; the `timeout` in
/// [`stdout_at_eight_jobs`] makes that a failure rather than a hung CI
/// job.
#[test]
fn files_that_emit_no_document_do_not_stall_the_emission_order() {
    let dir = TempDir::new().unwrap();
    let expected = write_ordering_fixture(dir.path(), ORDERING_FIXTURE_FILES);

    // `m3.empty.rs` sorts between `m3.rs` and `m4.rs`, so each of these
    // sits mid-stream rather than at either end, where a stalled slot
    // would be invisible.
    std::fs::write(dir.path().join("m3.empty.rs"), "").unwrap();
    std::fs::write(
        dir.path().join("m7.generated.rs"),
        "// @generated\nfn g() {}\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("m9.notes.unknownlang999"), "free text\n").unwrap();

    let stdout = stdout_at_eight_jobs(dir.path(), "metrics");

    assert_eq!(
        streamed_names(&stdout),
        expected,
        "the analyzable documents must still arrive in walk order"
    );
}

/// `--output-dir <DIR>` gives every document its own file, so no order
/// exists to get wrong — the tree is identical run to run by
/// construction. Pinned so a future refactor that routes the per-file
/// tree through the streaming emitter has to keep it that way.
#[test]
fn output_dir_tree_is_identical_across_runs() {
    let dir = TempDir::new().unwrap();
    write_ordering_fixture(dir.path(), ORDERING_FIXTURE_FILES);

    let mut digests = Vec::new();
    for run in 0..DETERMINISM_RUNS {
        let out_dir = dir.path().join(format!("out{run}"));
        cli()
            .args([
                "metrics",
                "--paths",
                dir.path().to_str().unwrap(),
                "-O",
                "json",
                "--output-dir",
                out_dir.to_str().unwrap(),
                "--jobs",
                "8",
            ])
            .assert()
            .success();

        let mut tree: Vec<(String, Vec<u8>)> = json_files(&out_dir)
            .into_iter()
            .map(|path| {
                let name = path.file_name().unwrap().to_str().unwrap().to_owned();
                (name, std::fs::read(&path).unwrap())
            })
            .collect();
        tree.sort();
        assert_eq!(
            tree.len(),
            ORDERING_FIXTURE_FILES,
            "one document per input file"
        );
        digests.push(tree);
    }

    assert!(
        digests.windows(2).all(|pair| pair[0] == pair[1]),
        "--output-dir must write an identical tree on every run"
    );
}

/// The C fixture `preproc` runs over, in creation order. Neither
/// sorted nor reverse-sorted, so an emission that merely preserved the
/// walk's arrival order could not match the sorted expectation either.
/// `x.c` includes `a.h` and `c.h`, and `c.h` includes `b.h`, so `x.c`'s
/// transitive closure spans every file — which is what makes its
/// `indirect_includes` a four-element set worth asserting on.
const PREPROC_FIXTURE: [(&str, &str); 4] = [
    (
        "x.c",
        "#include \"a.h\"\n#include \"c.h\"\nint main(void){return 0;}\n",
    ),
    ("c.h", "#include \"b.h\"\n#define MACRO_D 4\n"),
    ("a.h", "#define MACRO_A 1\n#define MACRO_B 2\n"),
    ("b.h", "#define MACRO_C 3\n"),
];

/// Write [`PREPROC_FIXTURE`] under `dir` and return its file paths in
/// the order the document's `files` keys must carry.
///
/// Sorted as `PathBuf`s, not as strings, for the reason
/// [`write_ordering_fixture`] documents — the keys really are compared
/// that way. The `indirect_includes` array is `String`s and sorts by
/// bytes instead, an order this flat single-directory fixture cannot
/// tell apart from the component-wise one. That distinction is pinned
/// where it can be built exactly, in
/// `preproc::tests::serialization_emits_files_and_name_sets_in_sorted_order`;
/// here the concern is end-to-end wiring, so one list serves both.
fn write_preproc_fixture(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut expected = Vec::with_capacity(PREPROC_FIXTURE.len());
    for (name, body) in PREPROC_FIXTURE {
        let file = dir.join(name);
        std::fs::write(&file, body).unwrap();
        expected.push(file);
    }
    expected.sort_unstable();
    expected
}

/// The path rendered as it appears in the document — a JSON string
/// literal, quotes and escapes included.
fn json_string(path: &std::path::Path) -> String {
    serde_json::to_string(path.to_str().unwrap()).unwrap()
}

/// Run `preproc --output <FILE>` over `dir` at `--jobs 8` and return
/// the emitted document as raw text.
///
/// Raw text, not a parsed `serde_json::Value`: this workspace
/// deliberately leaves `serde_json`'s `preserve_order` feature off (the
/// root `Cargo.toml` says why), so a parsed map re-sorts its own keys
/// and every key-order assertion over one is vacuously true.
///
/// `--no-config` rather than the `cli_in` cwd anchor the sibling tests
/// use: `preproc` renders a key relative to the working directory when
/// the file sits under it, so anchoring the cwd at the fixture would
/// make every key `./a.h` and put the assertions on a rendering
/// convention instead of on the order. Run from the inherited cwd the
/// keys are the absolute fixture paths, and `--no-config` supplies the
/// hermeticity the anchor would have.
fn preproc_document(dir: &std::path::Path, out: &std::path::Path) -> String {
    cli()
        .args([
            "preproc",
            "--no-config",
            "--paths",
            dir.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
            "--jobs",
            "8",
        ])
        .assert()
        .success();

    std::fs::read_to_string(out).expect("the preproc document is UTF-8")
}

/// The text opening a `PreprocFile`'s nested include closure.
const INDIRECT_KEY: &str = "\"indirect_includes\":[";

/// `preproc --output` emits its `files` map in sorted path order, and
/// each file's include/macro sets in sorted order within that (#1304).
///
/// Both containers are hashed (`HashMap<PathBuf, PreprocFile>` holding
/// `HashSet<String>`s), so before the fix this document was a fresh
/// permutation on nearly every run: eight runs over a five-file tree
/// gave eight distinct hashes. The `indirect_includes` assertion is
/// separate on purpose — it is the nested container, and a fix that
/// ordered only the top-level map would leave it moving. Perturbing
/// each half of production alone fails both, measured.
#[test]
fn preproc_output_map_and_include_sets_are_sorted() {
    let dir = TempDir::new().unwrap();
    let expected = write_preproc_fixture(dir.path());
    let out = dir.path().join("pp.json");

    let doc = preproc_document(dir.path(), &out);

    // Where each entry's key sits in the raw document. Sorted-path
    // order is the order they were looked up in, so the positions must
    // come out strictly increasing.
    let at: Vec<usize> = expected
        .iter()
        .map(|path| {
            doc.find(&format!("{}:{{", json_string(path)))
                .unwrap_or_else(|| {
                    panic!("document must carry an entry for {}: {doc}", path.display())
                })
        })
        .collect();
    // `windows(2)` over fewer than two entries yields nothing and makes
    // the assertion below vacuously true, so pin the count first.
    assert_eq!(
        at.len(),
        PREPROC_FIXTURE.len(),
        "every fixture file must have an entry to order"
    );
    assert!(
        at.windows(2).all(|pair| pair[0] < pair[1]),
        "the files map must be emitted in sorted path order: {doc}"
    );

    // `x.c` reaches every fixture file, so its closure is the whole
    // sorted path list.
    let x_c = format!("{}:{{", json_string(&dir.path().join("x.c")));
    let entry = doc.split_once(&x_c).expect("x.c has an entry").1;
    let array = entry
        .split_once(INDIRECT_KEY)
        .expect("x.c's entry carries an indirect_includes array")
        .1;
    let emitted = array.split_once(']').expect("the array closes").0;
    let want: Vec<String> = expected.iter().map(|path| json_string(path)).collect();
    assert_eq!(
        emitted,
        want.join(","),
        "the nested include closure must be emitted sorted too"
    );
}

/// The whole document is byte-identical across runs over an unchanged
/// tree, which is the property #1304 reports and the one the sibling
/// destinations already hold.
#[test]
fn preproc_output_is_byte_identical_across_runs() {
    let dir = TempDir::new().unwrap();
    write_preproc_fixture(dir.path());

    let runs: Vec<String> = (0..DETERMINISM_RUNS)
        .map(|run| preproc_document(dir.path(), &dir.path().join(format!("pp{run}.json"))))
        .collect();

    assert!(
        runs.windows(2).all(|pair| pair[0] == pair[1]),
        "preproc --output must write the same bytes on every run: {runs:?}"
    );
}
