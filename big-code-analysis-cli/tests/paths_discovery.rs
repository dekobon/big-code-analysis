//! Tests for the gitignore-aware pre-walker and `--paths-from` /
//! `--no-ignore` flag wiring.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn cli(env_dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("big-code-analysis-cli").unwrap();
    // Isolate from any user-level global gitignore so tests are
    // deterministic across machines.
    cmd.env("HOME", env_dir)
        .env("XDG_CONFIG_HOME", env_dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null");
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
    let mut found = Vec::new();
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
    assert!(
        names.iter().any(|n| n.contains("keep.py")),
        "keep.py missing from --paths-from output, got {names:?}"
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
        .stderr(predicate::str::contains("--paths-from"));
}
