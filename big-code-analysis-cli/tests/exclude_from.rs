//! Tests for `--exclude-from <FILE>`: union semantics with
//! `--exclude`, `.gitignore`-style blank / comment line handling,
//! missing-file errors, and malformed-glob errors.

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
    cmd
}

/// Three sibling Python files. Tests use globs over their names to
/// exercise --exclude / --exclude-from independently.
fn make_tree(dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    let keep = src.join("keep.py");
    let drop_a = src.join("drop_a.py");
    let drop_b = src.join("drop_b.py");
    std::fs::write(&keep, "def k(): return 1\n").unwrap();
    std::fs::write(&drop_a, "def a(): return 2\n").unwrap();
    std::fs::write(&drop_b, "def b(): return 3\n").unwrap();
    (keep, drop_a, drop_b)
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
fn exclude_from_file_drops_listed_patterns() {
    let dir = TempDir::new().unwrap();
    let (_keep, _drop_a, _drop_b) = make_tree(dir.path());
    let bcaignore = dir.path().join(".bcaignore");
    std::fs::write(&bcaignore, "**/drop_*.py\n").unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();

    cli(dir.path())
        .args([
            "--paths",
            dir.path().join("src").to_str().unwrap(),
            "--exclude-from",
            bcaignore.to_str().unwrap(),
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
        "exclude-from should drop both drop_a.py and drop_b.py"
    );
}

#[test]
fn exclude_from_unions_with_exclude_flag() {
    let dir = TempDir::new().unwrap();
    let _ = make_tree(dir.path());
    let bcaignore = dir.path().join(".bcaignore");
    // File excludes drop_a; CLI flag excludes drop_b. Both should
    // disappear from the output, confirming union semantics.
    std::fs::write(&bcaignore, "**/drop_a.py\n").unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();

    cli(dir.path())
        .args([
            "--paths",
            dir.path().join("src").to_str().unwrap(),
            "--exclude-from",
            bcaignore.to_str().unwrap(),
            // `--exclude=VAL` form binds the value to the flag; the
            // bare `-X VAL` shape would let clap's `num_args(0..)`
            // gobble `metrics` as another exclude.
            "--exclude=**/drop_b.py",
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
        "--exclude and --exclude-from patterns should union into one deny-set"
    );
}

#[test]
fn exclude_from_skips_blank_and_comment_lines() {
    let dir = TempDir::new().unwrap();
    let _ = make_tree(dir.path());
    let bcaignore = dir.path().join(".bcaignore");
    // The first comment is a malformed glob (`[` opens a character
    // class that never closes). If production stops skipping
    // `#`-prefixed lines, `mk_globset` would try to parse this as a
    // pattern and bail out with "invalid glob pattern" — turning
    // the green `["keep.py.json"]` assertion red. Without this
    // line, every comment in the fixture is also a benign glob
    // (globset accepts arbitrary literal text), so the test would
    // pass under a regression that failed to skip comments.
    std::fs::write(
        &bcaignore,
        "\
# unclosed [bracket — malformed glob, must be skipped

**/drop_a.py
   # indented comment, must be skipped
\t
**/drop_b.py
",
    )
    .unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();

    cli(dir.path())
        .args([
            "--paths",
            dir.path().join("src").to_str().unwrap(),
            "--exclude-from",
            bcaignore.to_str().unwrap(),
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
        "blank and `#`-comment lines must not be parsed as glob patterns"
    );
}

#[test]
fn exclude_from_missing_file_dies_with_path_in_message() {
    let dir = TempDir::new().unwrap();
    let _ = make_tree(dir.path());
    let missing = dir.path().join("does-not-exist.bcaignore");

    cli(dir.path())
        .args([
            "--paths",
            dir.path().join("src").to_str().unwrap(),
            "--exclude-from",
            missing.to_str().unwrap(),
            "metrics",
            "-O",
            "json",
        ])
        .assert()
        // Missing file is a tool error (exit 1), not a violation.
        .code(1)
        .stderr(
            predicate::str::contains("--exclude-from")
                .and(predicate::str::contains("does-not-exist.bcaignore")),
        );
}

#[test]
fn exclude_from_invalid_glob_in_file_dies_like_exclude_flag() {
    let dir = TempDir::new().unwrap();
    let _ = make_tree(dir.path());
    let bcaignore = dir.path().join(".bcaignore");
    std::fs::write(&bcaignore, "[\n").unwrap();

    cli(dir.path())
        .args([
            "--paths",
            dir.path().join("src").to_str().unwrap(),
            "--exclude-from",
            bcaignore.to_str().unwrap(),
            "metrics",
            "-O",
            "json",
        ])
        .assert()
        // Pin exit 1 (tool error) rather than `.failure()` (any
        // non-zero). The CLI contract reserves exit 2 for
        // threshold violations; a regression that routed a
        // malformed glob through the violation path would slip
        // past a bare `.failure()` assertion.
        .code(1)
        .stderr(predicate::str::contains("invalid glob pattern"));
}

#[test]
fn exclude_from_stdin_reads_patterns() {
    let dir = TempDir::new().unwrap();
    let _ = make_tree(dir.path());
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    // Stdin payload mirrors the .gitignore-style file contents: blank
    // and `#`-comment lines must still be tolerated when patterns
    // arrive via `-` rather than from disk.
    let stdin = "# piped via stdin\n**/drop_a.py\n\n**/drop_b.py\n";

    cli(dir.path())
        .args([
            "--paths",
            dir.path().join("src").to_str().unwrap(),
            "--exclude-from",
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
    assert_eq!(
        names,
        vec!["keep.py.json".to_string()],
        "--exclude-from - should consume patterns from stdin the same way it consumes them from a file"
    );
}

#[test]
fn exclude_from_empty_file_leaves_inline_excludes_intact() {
    let dir = TempDir::new().unwrap();
    let _ = make_tree(dir.path());
    let bcaignore = dir.path().join(".bcaignore");
    std::fs::write(&bcaignore, "").unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();

    // Pair the empty `--exclude-from` with a meaningful
    // `--exclude=` so the test distinguishes "empty file → no
    // patterns added" (correct) from "flag silently ignored →
    // empty deny-set" (regression). Under correct code, the
    // inline `--exclude` still drops drop_a.py; under a
    // regression where `globals.exclude_from` is unread, this
    // assertion would still hold — so the file-read path is
    // independently pinned by `exclude_from_missing_file_dies_*`
    // and `exclude_from_file_drops_listed_patterns`. Together
    // the three tests triangulate the wiring.
    cli(dir.path())
        .args([
            "--paths",
            dir.path().join("src").to_str().unwrap(),
            "--exclude-from",
            bcaignore.to_str().unwrap(),
            "--exclude=**/drop_a.py",
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
        vec!["drop_b.py.json".to_string(), "keep.py.json".to_string()],
        "empty .bcaignore must not perturb inline `--exclude` semantics"
    );
}

#[test]
fn exclude_from_strips_utf8_bom_on_first_line() {
    let dir = TempDir::new().unwrap();
    let _ = make_tree(dir.path());
    let bcaignore = dir.path().join(".bcaignore");
    // UTF-8 BOM (`\u{feff}`, three bytes: EF BB BF) followed by a
    // `#`-comment containing a deliberately malformed glob. Without
    // BOM stripping, `# unclosed [bracket ...` would survive
    // `starts_with('#')` (the BOM is the first char, not `#`), be
    // passed to `Glob::new`, fail to parse, and crash the run with
    // "invalid glob pattern". With BOM stripping, the trimmed line
    // starts with `#` and is correctly skipped — the run succeeds.
    let mut bytes: Vec<u8> = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"# unclosed [bracket - malformed if not skipped\n**/drop_a.py\n");
    std::fs::write(&bcaignore, bytes).unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();

    cli(dir.path())
        .args([
            "--paths",
            dir.path().join("src").to_str().unwrap(),
            "--exclude-from",
            bcaignore.to_str().unwrap(),
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
        vec!["drop_b.py.json".to_string(), "keep.py.json".to_string()],
        "the BOM must be stripped so the `#`-comment is recognized and skipped, leaving only `**/drop_a.py` as an active exclude"
    );
}
