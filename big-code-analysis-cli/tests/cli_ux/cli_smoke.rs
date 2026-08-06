//! End-to-end smoke tests that drive the binary against real source
//! files and verify each subcommand actually produces the work it
//! claims, not just a successful exit code.
//!
//! These complement the parse-shape tests in `main.rs::tests`, which
//! only verify that clap accepts the command's flags.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

use crate::common;

fn cli() -> Command {
    common::bca_command()
}

/// A small fixture file known to the repo, resolved relative to the workspace
/// root so the path is valid regardless of the test runner's CWD. The shared
/// helper makes a missing integration corpus name itself (#1171).
fn fixture_path() -> String {
    common::corpus_fixture_path()
}

#[test]
fn metrics_writes_per_file_json_to_output_dir() {
    let dir = TempDir::new().unwrap();
    cli()
        .args([
            "metrics",
            "--paths",
            &fixture_path(),
            "-O",
            "json",
            "--output-dir",
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
            "metrics",
            "--paths",
            &fixture_path(),
            "-O",
            "json",
            "--pretty",
            "--output-dir",
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
            "ops",
            "--paths",
            &fixture_path(),
            "-O",
            "json",
            "--output-dir",
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
        .args(["dump", "--paths", &fixture_path()])
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
        .args(["functions", "--paths", &fixture_path()])
        .assert()
        .success()
        // The fixture contains `read_csvs`, `absolutify`, and `main`.
        .stdout(predicate::str::contains("read_csvs"))
        .stdout(predicate::str::contains("main"));
}

#[test]
fn find_locates_call_expressions() {
    cli()
        .args(["find", "--paths", &fixture_path(), "-t", "call"])
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
        .args([
            "count",
            "--paths",
            &fixture_path(),
            "-t",
            "function_definition",
        ])
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
        .args(["strip-comments", "--paths", src.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("x = 1"))
        .stdout(predicate::str::contains("this is a comment").not());
}

#[test]
fn strip_comments_output_flag_writes_to_file() {
    // `--output <file>` routes the stripped source to a file (not the
    // input, which `--in-place` would rewrite) and leaves stdout empty.
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("snippet.py");
    let out = dir.path().join("stripped.py");
    std::fs::write(&src, "# this is a comment\nx = 1\n").unwrap();
    cli()
        .args([
            "strip-comments",
            "--paths",
            src.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .success()
        // Nothing printed to stdout when routed to a file.
        .stdout(predicate::str::is_empty());

    let written = std::fs::read_to_string(&out).expect("output file written");
    assert!(
        written.contains("x = 1"),
        "code line preserved in output file"
    );
    assert!(
        !written.contains("this is a comment"),
        "comment stripped from output file"
    );
    // The input file is untouched (this is not an in-place rewrite).
    let original = std::fs::read_to_string(&src).unwrap();
    assert!(
        original.contains("this is a comment"),
        "input file must not be modified by --output"
    );
}

#[test]
fn strip_comments_output_conflicts_with_in_place() {
    // The two output sinks are mutually exclusive (clap `conflicts_with`);
    // requesting both is a usage error (exit 1 — clap usage errors map to
    // the tool-error code, not the metric-gate band; #594), not a silent
    // precedence.
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("snippet.py");
    std::fs::write(&src, "# c\nx = 1\n").unwrap();
    cli()
        .args([
            "strip-comments",
            "--paths",
            src.to_str().unwrap(),
            "--in-place",
            "--output",
            dir.path().join("out.py").to_str().unwrap(),
        ])
        .assert()
        .code(1);
}

#[test]
fn strip_comments_output_rejects_multi_file_input() {
    // `--output` is a single-file sink: with more than one input file
    // every worker would write to the same path (racing, last-writer-
    // wins, silent data loss). The command must reject the multi-file
    // case (die, exit 1) rather than clobber — `--in-place` is the
    // multi-file verb. Regression guard for the #539 `--output` addition.
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.py"), "# c\na = 1\n").unwrap();
    std::fs::write(dir.path().join("b.py"), "# c\nb = 2\n").unwrap();
    let out = dir.path().join("stripped.py");
    cli()
        .args([
            "strip-comments",
            "--paths",
            dir.path().to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("input files matched"));
    // No partial sink file is left behind.
    assert!(
        !out.exists(),
        "--output sink must not be written for a rejected multi-file run"
    );
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
        .args(["preproc", "--paths", src.to_str().unwrap()])
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
            "preproc",
            "--paths",
            src.to_str().unwrap(),
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
    // Join components separately so the path uses the OS separator: a
    // literal `"sub/helper.h"` would leave a forward slash on Windows
    // (mixed `...\sub/helper.h`) that never matches the walk-emitted,
    // backslash-separated `indirect_includes` path.
    let helper_h = dir.path().join("sub").join("helper.h");
    std::fs::write(
        &main_c,
        "#include \"helper.h\"\nint main(void){ return HELPER; }\n",
    )
    .unwrap();
    std::fs::write(&helper_h, "#define HELPER 42\n").unwrap();

    let output = cli()
        .args(["preproc", "--paths", dir.path().to_str().unwrap()])
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

/// The presentation half of the diagnostic-prefix contract (#1199): the
/// library's `PreprocDiagnostic::Display` renders a bare message, and
/// the CLI is what adds `warning:` — via `diag::warn`, the same helper
/// every other CLI diagnostic goes through. `src/preproc_tests.rs` pins
/// the bare messages; only an end-to-end run can pin what a user reads,
/// and without this half the prefix is guarded nowhere.
///
/// The fixture produces two of the five variants: a header-only
/// `SelfInclusion` and the multi-line `IncludeCycle`, which is the one
/// with a shape to get wrong. `warn` prefixes its header line only and
/// leaves the member lines indented beneath, and — since #1199 dropped
/// the `writeln!` that terminated the last member — the block is no
/// longer followed by a blank line.
#[test]
fn preproc_diagnostics_render_under_the_warning_prefix() {
    let dir = TempDir::new().unwrap();
    let self_h = dir.path().join("self.h");
    let a_h = dir.path().join("a.h");
    let b_h = dir.path().join("b.h");
    std::fs::write(&self_h, "#include \"self.h\"\nint s;\n").unwrap();
    std::fs::write(&a_h, "#include \"b.h\"\nint a;\n").unwrap();
    std::fs::write(&b_h, "#include \"a.h\"\nint b;\n").unwrap();

    let output = cli()
        .args(["preproc", "--paths", dir.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "preproc should succeed");
    let stderr = String::from_utf8(output.stderr).unwrap();
    let lines: Vec<&str> = stderr.lines().collect();

    // Diagnostics are emitted in `PreprocResults::files` (a `HashMap`)
    // order, so assert per-line rather than pinning the whole block.
    let self_line = format!("warning: possible self inclusion {}", self_h.display());
    assert!(
        lines.contains(&self_line.as_str()),
        "expected {self_line:?} among {lines:?}"
    );

    let header = lines
        .iter()
        .position(|l| *l == "warning: possible include cycle:")
        .unwrap_or_else(|| panic!("no cycle header in {lines:?}"));
    let mut members = lines
        .get(header + 1..header + 3)
        .unwrap_or_else(|| panic!("cycle header not followed by two members: {lines:?}"))
        .to_vec();
    members.sort_unstable();
    assert_eq!(
        members,
        vec![
            format!("  - \"{}\"", a_h.display()),
            format!("  - \"{}\"", b_h.display()),
        ],
        "the cycle members are indented under the prefixed header, unprefixed"
    );

    // A `Display` ending in a newline would stack with `warn`'s own and
    // leave a blank line after the block — the pre-#1199 output.
    assert!(
        !stderr.contains("\n\n"),
        "no diagnostic block is followed by a blank line: {stderr:?}"
    );
    assert!(
        lines
            .iter()
            .all(|l| l.starts_with("warning: ") || l.starts_with("  - \"")),
        "every stderr line is either a prefixed diagnostic or a cycle member: {lines:?}"
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

// Issue #595: forcing a language by canonical name (`-l rust`) on an
// extensionless file must produce metrics. Pre-#595 the value was only
// resolved through `get_from_ext`, so `rust` (a name, not an extension)
// silently disabled analysis — empty output, exit 0.
#[test]
fn force_language_by_name_analyzes_extensionless_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("noext_sample");
    std::fs::write(&file, b"fn main() { let x = 1; }\n").unwrap();

    let output = cli()
        .args([
            "metrics",
            "--paths",
            file.to_str().unwrap(),
            "-l",
            "rust",
            "-O",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(output).expect("stdout is utf-8");
    assert!(
        !text.trim().is_empty(),
        "forcing `-l rust` on an extensionless file must emit metrics, got empty output"
    );
    let parsed: serde_json::Value =
        serde_json::from_str(text.trim()).expect("metrics output must be valid JSON");
    assert!(
        parsed.get("metrics").is_some(),
        "expected a metrics record, got: {parsed}"
    );
}

// Issue #595: the extension spelling (`-l rs`) keeps working.
#[test]
fn force_language_by_extension_still_works() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("noext_sample");
    std::fs::write(&file, b"fn main() { let x = 1; }\n").unwrap();

    cli()
        .args([
            "metrics",
            "--paths",
            file.to_str().unwrap(),
            "-l",
            "rs",
            "-O",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"metrics\""));
}

// Issue #595: an unknown `--language` value is now a hard error (exit 1)
// that lists the valid language names — it no longer silently disables
// analysis with exit 0.
#[test]
fn unknown_language_value_exits_one_and_lists_valid_values() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("noext_sample");
    std::fs::write(&file, b"fn main() {}\n").unwrap();

    cli()
        .args(["metrics", "--paths", file.to_str().unwrap(), "-l", "bogus"])
        .assert()
        .failure()
        .code(1)
        .stderr(
            predicate::str::contains("unknown --language value 'bogus'")
                .and(predicate::str::contains("valid languages are:"))
                .and(predicate::str::contains("rust")),
        );
}
