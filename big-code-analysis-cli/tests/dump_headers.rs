//! Integration tests for `bca dump` per-file headers and the
//! explicit-path requirement (issue #690).

use std::fs;

use predicates::prelude::*;
use tempfile::TempDir;

mod common;

fn fixture() -> (TempDir, String, String) {
    let dir = TempDir::new().unwrap();
    let a = dir.path().join("a.rs");
    let b = dir.path().join("b.rs");
    fs::write(&a, "fn alpha() {}\n").unwrap();
    fs::write(&b, "fn beta() {}\n").unwrap();
    (
        dir,
        a.to_str().unwrap().to_string(),
        b.to_str().unwrap().to_string(),
    )
}

/// #690: each file's tree is prefixed with a `== <path> ==` banner so a
/// multi-file dump is attributable despite the parallel walk interleaving
/// output.
#[test]
fn dump_emits_per_file_headers() {
    let (_dir, a, b) = fixture();
    let out = common::bca_command()
        .args(["dump", "--paths", &a, "--paths", &b])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).expect("utf8");
    assert!(
        text.contains(&format!("== {a} ==")),
        "missing header for a.rs: {text}"
    );
    assert!(
        text.contains(&format!("== {b} ==")),
        "missing header for b.rs: {text}"
    );
}

/// #690: `find` text output carries the same `== <path> ==` banner.
#[test]
fn find_emits_per_file_headers() {
    let (_dir, a, _b) = fixture();
    common::bca_command()
        .args(["find", "--paths", &a, "-t", "function_item"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("== {a} ==")));
}

/// #690: bare `bca dump` (no explicit path) errors instead of defaulting
/// to a whole-tree dump of the current directory — the documented
/// exception to #596's default-`.` walk.
#[test]
fn bare_dump_requires_an_explicit_path() {
    let dir = TempDir::new().unwrap();
    // A `.git` marker stops `bca.toml` discovery from injecting a manifest
    // `paths` key that would satisfy the requirement.
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join("x.rs"), "fn x() {}\n").unwrap();
    common::cli_in(dir.path())
        .arg("dump")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("dump needs an explicit path"));
}

/// #690: `dump <path>` still works (the requirement is only "give a
/// path", not a behavior change for explicit invocations).
#[test]
fn dump_with_explicit_path_works() {
    let (_dir, a, _b) = fixture();
    common::bca_command()
        .args(["dump", "--paths", &a])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("== {a} ==")));
}
