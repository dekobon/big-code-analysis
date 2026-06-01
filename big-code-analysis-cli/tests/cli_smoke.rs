//! End-to-end smoke tests that drive the binary against real source
//! files and verify each subcommand actually produces the work it
//! claims, not just a successful exit code.
//!
//! These complement the parse-shape tests in `main.rs::tests`, which
//! only verify that clap accepts the command's flags.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

mod common;

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
fn metrics_writes_per_file_json_to_output_dir() {
    let dir = TempDir::new().unwrap();
    cli()
        .args([
            "--paths",
            &fixture_path(),
            "metrics",
            "-O",
            "json",
            "-o",
            dir.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    // Filenames mirror the input path with a `.json` suffix; the only
    // *.json file in the output dir should be a parseable metrics record.
    let entries: Vec<_> = walkdir_entries(dir.path(), "json").collect();
    assert_eq!(entries.len(), 1, "expected one .json file: {entries:?}");
    let content = std::fs::read_to_string(&entries[0]).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&content).expect("metrics output must be valid JSON");
    // Both fields are always present in a metrics record. AND-ing catches
    // regressions that drop one field while preserving the other.
    assert!(
        parsed.get("metrics").is_some() && parsed.get("name").is_some(),
        "expected metrics record with both `metrics` and `name` fields, got: {parsed}"
    );
}

#[test]
fn metrics_pretty_emits_indented_json() {
    let dir = TempDir::new().unwrap();
    cli()
        .args([
            "--paths",
            &fixture_path(),
            "metrics",
            "-O",
            "json",
            "--pretty",
            "-o",
            dir.path().to_str().unwrap(),
        ])
        .assert()
        .success();
    let entries: Vec<_> = walkdir_entries(dir.path(), "json").collect();
    let content = std::fs::read_to_string(&entries[0]).unwrap();
    assert!(
        content.contains("\n  "),
        "pretty JSON should be indented; got: {content}"
    );
}

#[test]
fn ops_writes_per_file_json_to_output_dir() {
    let dir = TempDir::new().unwrap();
    cli()
        .args([
            "--paths",
            &fixture_path(),
            "ops",
            "-O",
            "json",
            "-o",
            dir.path().to_str().unwrap(),
        ])
        .assert()
        .success();
    let entries: Vec<_> = walkdir_entries(dir.path(), "json").collect();
    assert_eq!(entries.len(), 1);
    let content = std::fs::read_to_string(&entries[0]).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&content).expect("ops output must be valid JSON");
    // An ops record exposes operand/operator collections. Asserting the
    // top-level object is non-empty rules out the trivial-pass case where
    // the writer emits `{}`.
    let obj = parsed
        .as_object()
        .expect("ops output must be a JSON object");
    assert!(!obj.is_empty(), "ops record must not be empty: {parsed}");
}

#[test]
fn dump_prints_ast_to_stdout() {
    cli()
        .args(["--paths", &fixture_path(), "dump"])
        .assert()
        .success()
        // The Python file declares functions; the AST dump should
        // include at least one identifier and one function-definition
        // node-kind label.
        .stdout(predicate::str::contains("function_definition"))
        .stdout(predicate::str::contains("identifier"));
}

#[test]
fn functions_lists_function_spans() {
    cli()
        .args(["--paths", &fixture_path(), "functions"])
        .assert()
        .success()
        // The fixture contains `read_csvs`, `absolutify`, and `main`.
        .stdout(predicate::str::contains("read_csvs"))
        .stdout(predicate::str::contains("main"));
}

#[test]
fn find_locates_call_expressions() {
    cli()
        .args(["--paths", &fixture_path(), "find", "call"])
        .assert()
        .success()
        // `find` emits matches with the node-kind label `{call:<id>}`.
        // Asserting on the prefix `{call:` (instead of bare "call") locks
        // the test to that output structure — the search term echoed
        // anywhere else in the output would not satisfy this assertion.
        .stdout(predicate::str::contains("{call:"));
}

#[test]
fn count_reports_node_counts() {
    cli()
        .args(["--paths", &fixture_path(), "count", "function_definition"])
        .assert()
        .success()
        // Count emits a "Total nodes / Found nodes / Percentage" report.
        .stdout(predicate::str::contains("Total nodes"))
        .stdout(predicate::str::contains("Found nodes"));
}

#[test]
fn strip_comments_writes_to_stdout_without_comments() {
    // A small Python source with one comment; verify the comment is gone
    // from the output but the code line is preserved.
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("snippet.py");
    std::fs::write(&src, "# this is a comment\nx = 1\n").unwrap();
    cli()
        .args(["--paths", src.to_str().unwrap(), "strip-comments"])
        .assert()
        .success()
        .stdout(predicate::str::contains("x = 1"))
        .stdout(predicate::str::contains("this is a comment").not());
}

#[test]
fn preproc_emits_json_to_stdout_without_output() {
    // The producer walks paths and emits a `PreprocResults` JSON. Even
    // when no C/C++ files are present, it must emit a syntactically
    // valid (possibly empty) JSON object so downstream tooling can
    // consume it unconditionally.
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("nothing.txt");
    std::fs::write(&src, "no preproc here\n").unwrap();

    let output = cli()
        .args(["--paths", src.to_str().unwrap(), "preproc"])
        .output()
        .unwrap();
    assert!(output.status.success(), "preproc should succeed");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let _: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("preproc must emit valid JSON to stdout");
}

#[test]
fn preproc_writes_json_to_output_file() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("dummy.txt");
    std::fs::write(&src, "irrelevant\n").unwrap();
    let out = dir.path().join("preproc.json");

    cli()
        .args([
            "--paths",
            src.to_str().unwrap(),
            "preproc",
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    let content = std::fs::read_to_string(&out).unwrap();
    let _: serde_json::Value =
        serde_json::from_str(&content).expect("preproc output file must be valid JSON");
}

#[test]
fn preproc_resolves_cross_file_include_across_directory() {
    // Regression guard for the #489 → #495 preproc breakage: with a
    // multi-file C tree where `main.c` includes `helper.h` from a
    // sibling subdirectory, the basename-grouping the resolver needs
    // must be computed from the analyzed file list so `fix_includes`
    // can resolve the directive. Before #495 the grouping callback was
    // dead (the library walk no longer fired it), leaving every file's
    // `indirect_includes` containing only itself — the cross-file
    // include silently unresolved. The smoke test above misses this
    // because its fixture has no `#include`.
    let dir = TempDir::new().unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    let main_c = dir.path().join("main.c");
    let helper_h = dir.path().join("sub/helper.h");
    std::fs::write(
        &main_c,
        "#include \"helper.h\"\nint main(void){ return HELPER; }\n",
    )
    .unwrap();
    std::fs::write(&helper_h, "#define HELPER 42\n").unwrap();

    let output = cli()
        .args(["--paths", dir.path().to_str().unwrap(), "preproc"])
        .output()
        .unwrap();
    assert!(output.status.success(), "preproc should succeed");

    let stdout = String::from_utf8(output.stdout).unwrap();
    let json: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("preproc must emit valid JSON");

    let main_key = main_c.to_str().unwrap();
    let helper_key = helper_h.to_str().unwrap();
    let main_entry = json
        .get("files")
        .and_then(|f| f.get(main_key))
        .unwrap_or_else(|| panic!("preproc output missing entry for {main_key}: {json:#}"));

    // The raw directive is recorded verbatim as a direct include.
    let direct: Vec<&str> = main_entry["direct_includes"]
        .as_array()
        .expect("direct_includes is an array")
        .iter()
        .map(|v| v.as_str().expect("include is a string"))
        .collect();
    assert!(
        direct.contains(&"helper.h"),
        "main.c must record the raw `helper.h` direct include, got {direct:?}",
    );

    // The cross-file include must be RESOLVED into main.c's indirect
    // includes by basename grouping — this is the assertion that fails
    // against the empty-`all_files` regression.
    let indirect: Vec<&str> = main_entry["indirect_includes"]
        .as_array()
        .expect("indirect_includes is an array")
        .iter()
        .map(|v| v.as_str().expect("include is a string"))
        .collect();
    assert!(
        indirect.contains(&helper_key),
        "main.c's indirect_includes must resolve the cross-directory \
         `helper.h` to {helper_key}, got {indirect:?}",
    );
}

/// Recursively yield files under `dir` whose extension equals `ext`.
fn walkdir_entries(dir: &std::path::Path, ext: &str) -> impl Iterator<Item = std::path::PathBuf> {
    fn visit(dir: &std::path::Path, ext: &str, found: &mut Vec<std::path::PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    visit(&p, ext, found);
                } else if p.extension().and_then(|e| e.to_str()) == Some(ext) {
                    found.push(p);
                }
            }
        }
    }
    let mut found = Vec::new();
    visit(dir, ext, &mut found);
    found.into_iter()
}
