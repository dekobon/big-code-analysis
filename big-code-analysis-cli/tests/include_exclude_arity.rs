//! Regression tests for #601: `-I/--include` and `-X/--exclude` take
//! exactly one value per occurrence and are repeatable.
//!
//! Before #601 both flags were declared `num_args(0..)`, making them
//! greedy: every positional that followed (a path, a subcommand token,
//! a `find` node kind) was swallowed as another glob until the next
//! flag, and a bare `--include` (zero values) parsed as a silent no-op.
//! The flags now use `num_args(1)` + `ArgAction::Append`, matching the
//! rg/fd convention (`-X a -X b`) and making flag/positional mixing
//! unambiguous.

use std::path::Path;

use assert_cmd::Command;
use tempfile::TempDir;

mod common;

fn cli(dir: &Path) -> Command {
    let mut cmd = common::bca_command();
    cmd.current_dir(dir);
    cmd
}

/// Sorted basenames of the `*.json` metric files emitted into `out`.
fn emitted_json(out: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(out)
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| Path::new(n).extension().is_some_and(|e| e == "json"))
        .collect();
    names.sort();
    names
}

/// Write a single Rust source file into a fresh tempdir and return the
/// guard plus the file path string.
fn fixture() -> (TempDir, String) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("a.rs");
    std::fs::write(&path, "pub fn f() -> i32 { 1 }\n").unwrap();
    let path = path.to_str().unwrap().to_owned();
    (dir, path)
}

/// A `find` positional (`<NODES>...`) after `-X` must be parsed as the
/// node kind, not swallowed as a second exclude glob. This is the exact
/// failure reported in #601 (`bca find -p src -X '*.tmp' function_item`
/// errored with "required arguments were not provided: <NODES>...").
#[test]
fn exclude_does_not_swallow_find_positional() {
    let (dir, path) = fixture();
    cli(dir.path())
        .args([
            "find",
            "--paths",
            &path,
            "--exclude",
            "*.tmp",
            "-t",
            "function_item",
        ])
        .assert()
        .success();
}

/// Same contract for `-I/--include`.
#[test]
fn include_does_not_swallow_find_positional() {
    let (dir, path) = fixture();
    cli(dir.path())
        .args([
            "find",
            "--paths",
            &path,
            "--include",
            "*.rs",
            "-t",
            "function_item",
        ])
        .assert()
        .success();
}

/// Repeating `-X` accumulates: `-X a -X b` yields the two-element list
/// the old space-separated `-X a b` spelling used to produce.
#[test]
fn exclude_flag_repeats_accumulate() {
    let dir = TempDir::new().unwrap();
    // Two excludable files plus one keeper.
    for name in ["drop_a.rs", "drop_b.rs", "keep.rs"] {
        std::fs::write(dir.path().join(name), "pub fn f() -> i32 { 1 }\n").unwrap();
    }
    let out = TempDir::new().unwrap();
    cli(dir.path())
        .args([
            "metrics",
            "--paths",
            dir.path().to_str().unwrap(),
            "--exclude",
            "**/drop_a.rs",
            "--exclude",
            "**/drop_b.rs",
            "-O",
            "json",
            "--output-dir",
            out.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    assert_eq!(
        emitted_json(out.path()),
        vec!["keep.rs.json".to_string()],
        "both repeated --exclude globs must apply; only keep.rs survives"
    );
}

/// Repeating `-I` accumulates the same way.
#[test]
fn include_flag_repeats_accumulate() {
    let dir = TempDir::new().unwrap();
    for name in ["a.rs", "b.py", "c.txt"] {
        std::fs::write(dir.path().join(name), "x = 1\n").unwrap();
    }
    let out = TempDir::new().unwrap();
    cli(dir.path())
        .args([
            "metrics",
            "--paths",
            dir.path().to_str().unwrap(),
            "--include",
            "**/*.rs",
            "--include",
            "**/*.py",
            "-O",
            "json",
            "--output-dir",
            out.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    assert_eq!(
        emitted_json(out.path()),
        vec!["a.rs.json".to_string(), "b.py.json".to_string()],
        "both repeated --include globs must apply; c.txt excluded"
    );
}

/// A bare `--include` with no value is now a usage error (exit 1 —
/// clap usage errors map to the tool-error code, not the metric-gate
/// band; #594), where it used to parse as a silent no-op.
#[test]
fn bare_include_is_usage_error() {
    let (dir, _path) = fixture();
    // `--include` trailing a real subcommand with no value of its own.
    // Under `num_args(1)` clap demands the missing value and errors
    // "a value is required for '--include'". Under the pre-#601
    // `num_args(0..)` arity a zero-value `--include` was legal, so this
    // ran `metrics` and never reached a usage error — that is the
    // regression this discriminator catches. Putting the subcommand
    // first means clap cannot satisfy the value by swallowing it, so the
    // two arities diverge here where `--include metrics` would not.
    let assert = cli(dir.path())
        .args(["metrics", "--include"])
        .assert()
        .code(1);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("a value is required for '--include"),
        "bare --include must error that a value is required (num_args(1)); \
         stderr was:\n{stderr}"
    );
}

/// A comma inside a single glob value is kept literal — `value_delimiter`
/// is deliberately NOT set (#601), so a brace-expansion glob like
/// `{a,b}/**` survives as one pattern rather than splitting on the comma.
#[test]
fn exclude_value_with_comma_is_one_glob() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir(dir.path().join("a")).unwrap();
    std::fs::create_dir(dir.path().join("b")).unwrap();
    std::fs::write(dir.path().join("a/x.rs"), "pub fn f() {}\n").unwrap();
    std::fs::write(dir.path().join("b/y.rs"), "pub fn g() {}\n").unwrap();
    std::fs::write(dir.path().join("keep.rs"), "pub fn h() {}\n").unwrap();
    let out = TempDir::new().unwrap();
    cli(dir.path())
        .args([
            "metrics",
            "--paths",
            dir.path().to_str().unwrap(),
            "--exclude",
            "**/{a,b}/**",
            "-O",
            "json",
            "--output-dir",
            out.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    assert_eq!(
        emitted_json(out.path()),
        vec!["keep.rs.json".to_string()],
        "a brace-expansion glob with a comma must be treated as one \
         pattern (no value_delimiter splitting), excluding both a/ and b/"
    );
}
