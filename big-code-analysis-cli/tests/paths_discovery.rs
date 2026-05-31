//! Tests for the gitignore-aware pre-walker and `--paths-from` /
//! `--no-ignore` flag wiring.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

mod common;

fn cli(env_dir: &Path) -> Command {
    let mut cmd = common::bca_command();
    // Isolate from any user-level global gitignore so tests are
    // deterministic across machines.
    cmd.env("HOME", env_dir)
        .env("XDG_CONFIG_HOME", env_dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null");
    // These tests exercise `--paths` / `--paths-from` / gitignore walking
    // against synthetic tempdir trees and assert on the exact file set
    // walked. The runner's cwd is inside the repo, whose root `bca.toml`
    // declares `paths` and `exclude_from`; `--no-config` suppresses that
    // discovery so the manifest cannot leak extra path/exclude rules
    // into the walk. As a global flag it must precede
    // the subcommand, so it is injected here before any per-test `args`.
    cmd.arg("--no-config");
    cmd
}

fn make_tree(dir: &Path) -> (PathBuf, PathBuf) {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    let keep = src.join("keep.py");
    let skip = src.join("skip.py");
    std::fs::write(&keep, "def f(): return 1\n").unwrap();
    std::fs::write(&skip, "def g(): return 2\n").unwrap();
    std::fs::write(dir.join(".gitignore"), "skip.py\n").unwrap();
    (keep, skip)
}

fn json_files(dir: &Path) -> Vec<String> {
    fn visit(dir: &Path, found: &mut Vec<String>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    visit(&p, found);
                } else if p.extension().and_then(|e| e.to_str()) == Some("json") {
                    found.push(p.file_name().unwrap().to_string_lossy().into_owned());
                }
            }
        }
    }
    let mut found = Vec::new();
    visit(dir, &mut found);
    found.sort();
    found
}

#[test]
fn gitignore_skips_excluded_file_when_walking_dir() {
    let dir = TempDir::new().unwrap();
    let _ = make_tree(dir.path());
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();

    cli(dir.path())
        .args([
            "--paths",
            dir.path().to_str().unwrap(),
            "metrics",
            "-O",
            "json",
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let names = json_files(&out);
    assert!(
        names.iter().any(|n| n.contains("keep.py")),
        "expected keep.py in output, got {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.contains("skip.py")),
        "skip.py should be filtered by .gitignore, got {names:?}"
    );
}

#[test]
fn gitignore_explicit_path_bypasses_ignore() {
    let dir = TempDir::new().unwrap();
    let (_keep, skip) = make_tree(dir.path());
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();

    cli(dir.path())
        .args([
            "--paths",
            skip.to_str().unwrap(),
            "metrics",
            "-O",
            "json",
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let names = json_files(&out);
    assert!(
        names.iter().any(|n| n.contains("skip.py")),
        "explicit path must bypass .gitignore, got {names:?}"
    );
}

#[test]
fn no_ignore_flag_includes_gitignored_file() {
    let dir = TempDir::new().unwrap();
    let _ = make_tree(dir.path());
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();

    cli(dir.path())
        .args([
            "--no-ignore",
            "--paths",
            dir.path().to_str().unwrap(),
            "metrics",
            "-O",
            "json",
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let names = json_files(&out);
    assert!(
        names.iter().any(|n| n.contains("keep.py")),
        "keep.py missing under --no-ignore, got {names:?}"
    );
    assert!(
        names.iter().any(|n| n.contains("skip.py")),
        "skip.py must appear under --no-ignore, got {names:?}"
    );
}

#[test]
fn paths_from_file_reads_paths() {
    let dir = TempDir::new().unwrap();
    let (keep, _skip) = make_tree(dir.path());
    let listfile = dir.path().join("paths.txt");
    std::fs::write(&listfile, format!("{}\n", keep.display())).unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();

    cli(dir.path())
        .args([
            "--paths-from",
            listfile.to_str().unwrap(),
            "metrics",
            "-O",
            "json",
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let names = json_files(&out);
    // Exact cardinality catches a class of bugs where --paths-from is
    // misread as a directory walk seed (e.g., walking the listfile's
    // parent), which would silently include extra files the listfile
    // never named.
    assert_eq!(
        names,
        vec!["keep.py.json".to_string()],
        "expected exactly keep.py.json from one-line --paths-from"
    );
}

#[test]
fn paths_from_stdin_reads_paths() {
    let dir = TempDir::new().unwrap();
    let (keep, skip) = make_tree(dir.path());
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();

    let stdin = format!("{}\n{}\n", keep.display(), skip.display());

    cli(dir.path())
        .args([
            "--paths-from",
            "-",
            "metrics",
            "-O",
            "json",
            "-o",
            out.to_str().unwrap(),
        ])
        .write_stdin(stdin)
        .assert()
        .success();

    let names = json_files(&out);
    assert!(
        names.iter().any(|n| n.contains("keep.py")),
        "keep.py missing from stdin output, got {names:?}"
    );
    assert!(
        names.iter().any(|n| n.contains("skip.py")),
        "skip.py from stdin should bypass .gitignore (explicit), got {names:?}"
    );
}

#[test]
fn paths_from_and_paths_union_both() {
    let dir = TempDir::new().unwrap();
    let (keep, skip) = make_tree(dir.path());
    let listfile = dir.path().join("paths.txt");
    std::fs::write(&listfile, format!("{}\n", skip.display())).unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();

    cli(dir.path())
        .args([
            "--paths",
            keep.to_str().unwrap(),
            "--paths-from",
            listfile.to_str().unwrap(),
            "metrics",
            "-O",
            "json",
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let names = json_files(&out);
    assert!(
        names.iter().any(|n| n.contains("keep.py")),
        "keep.py missing from union, got {names:?}"
    );
    assert!(
        names.iter().any(|n| n.contains("skip.py")),
        "skip.py missing from union, got {names:?}"
    );
}

#[test]
fn paths_from_file_trims_whitespace() {
    let dir = TempDir::new().unwrap();
    let (keep, _skip) = make_tree(dir.path());
    let listfile = dir.path().join("paths.txt");
    // Line has trailing spaces and a tab — the bug would construct a path
    // that doesn't exist, causing a "File doesn't exist" warning and no output.
    std::fs::write(&listfile, format!("{}  \t\n\n   \n", keep.display())).unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();

    cli(dir.path())
        .args([
            "--paths-from",
            listfile.to_str().unwrap(),
            "metrics",
            "-O",
            "json",
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("doesn't exist").not());

    let names = json_files(&out);
    assert_eq!(
        names,
        vec!["keep.py.json".to_string()],
        "trailing whitespace must be trimmed before PathBuf construction"
    );
}

#[test]
fn paths_from_missing_file_dies() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("does-not-exist.txt");

    cli(dir.path())
        .args([
            "--paths-from",
            missing.to_str().unwrap(),
            "metrics",
            "-O",
            "json",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("--paths-from")
                .and(predicate::str::contains("does-not-exist.txt")),
        );
}

#[test]
fn paths_from_strips_utf8_bom_on_first_line() {
    let dir = TempDir::new().unwrap();
    let (keep, _skip) = make_tree(dir.path());
    let listfile = dir.path().join("paths.txt");
    // UTF-8 BOM (`\u{feff}`, three bytes: EF BB BF) followed by an
    // otherwise valid path. Without BOM stripping, the first line
    // would be `\u{feff}<keep_path>` — a literal path the
    // walker would warn about (file doesn't exist) and skip,
    // turning a green output assertion into an empty directory.
    // The fix lives in the shared `collect_lines` helper, so this
    // mirrors `exclude_from_strips_utf8_bom_on_first_line` to keep
    // both flag families covered.
    let mut bytes: Vec<u8> = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(format!("{}\n", keep.display()).as_bytes());
    std::fs::write(&listfile, bytes).unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();

    cli(dir.path())
        .args([
            "--paths-from",
            listfile.to_str().unwrap(),
            "metrics",
            "-O",
            "json",
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let names = json_files(&out);
    assert_eq!(
        names,
        vec!["keep.py.json".to_string()],
        "BOM must be stripped so the first path is recognized as a real file"
    );
}
