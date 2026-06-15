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

/// Return the contiguous output block that follows the `== <banner> ==`
/// line for `path` — every line up to the next `== ` banner or EOF. The
/// banner line itself is excluded so the block holds only the tree (or
/// match list) attributed to that file. Returns `None` if the banner is
/// absent. This is the discriminator the bare `text.contains("== a ==")`
/// presence checks lacked: it lets a test assert *which* tree sits under
/// each banner, so a banner-to-tree detachment regression (the exact #690
/// failure mode) fails instead of passing on mere string presence.
fn block_under_banner<'a>(text: &'a str, path: &str) -> Option<&'a str> {
    let banner = format!("== {path} ==");
    let after = text.split_once(&banner)?.1;
    // The next banner opens with "== "; truncate there so the block holds
    // only this file's output, not the following file's.
    Some(match after.find("\n== ") {
        Some(end) => &after[..end],
        None => after,
    })
}

/// #690: each file's tree is prefixed with a `== <path> ==` banner *and*
/// the tree that follows the banner is that file's own — not merely that
/// both banner strings appear somewhere in stdout. `--jobs 1` serializes
/// the walk so the per-file (banner, tree) pairs are emitted in a stable
/// order; the assertion then pins each banner to its own function node so a
/// regression that detached a banner from its tree would fail here.
#[test]
fn dump_emits_per_file_headers() {
    let (_dir, a, b) = fixture();
    let out = common::bca_command()
        .args(["dump", "--jobs", "1", "--paths", &a, "--paths", &b])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).expect("utf8");

    let a_block =
        block_under_banner(&text, &a).unwrap_or_else(|| panic!("missing header for a.rs:\n{text}"));
    assert!(
        a_block.contains("alpha") && !a_block.contains("beta"),
        "a.rs banner must bracket alpha's tree, not beta's; block was:\n{a_block}"
    );

    let b_block =
        block_under_banner(&text, &b).unwrap_or_else(|| panic!("missing header for b.rs:\n{text}"));
    assert!(
        b_block.contains("beta") && !b_block.contains("alpha"),
        "b.rs banner must bracket beta's tree, not alpha's; block was:\n{b_block}"
    );
}

/// #690: `find` text output carries the same `== <path> ==` banner, and
/// each banner brackets its own file's matches. Two distinct fixtures plus
/// `--jobs 1` make a cross-file mis-attribution observable, which the
/// single-file presence check could not exercise.
#[test]
fn find_emits_per_file_headers() {
    let (_dir, a, b) = fixture();
    let out = common::bca_command()
        .args([
            "find",
            "--jobs",
            "1",
            "--paths",
            &a,
            "--paths",
            &b,
            "-t",
            "identifier",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).expect("utf8");

    let a_block =
        block_under_banner(&text, &a).unwrap_or_else(|| panic!("missing header for a.rs:\n{text}"));
    assert!(
        a_block.contains("alpha") && !a_block.contains("beta"),
        "a.rs banner must bracket alpha's match, not beta's; block was:\n{a_block}"
    );

    let b_block =
        block_under_banner(&text, &b).unwrap_or_else(|| panic!("missing header for b.rs:\n{text}"));
    assert!(
        b_block.contains("beta") && !b_block.contains("alpha"),
        "b.rs banner must bracket beta's match, not alpha's; block was:\n{b_block}"
    );
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
