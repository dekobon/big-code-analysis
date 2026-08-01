//! Tests for the gitignore-aware pre-walker and `--paths-from` /
//! `--no-ignore` flag wiring.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

use crate::common;

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
    // into the walk. As a scoped flag it must follow the subcommand, so
    // each per-test `args` array passes it right after the subcommand
    // token rather than injecting it here.
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
            "metrics",
            "--no-config",
            "--paths",
            dir.path().to_str().unwrap(),
            "-O",
            "json",
            "--output-dir",
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
            "metrics",
            "--no-config",
            "--paths",
            skip.to_str().unwrap(),
            "-O",
            "json",
            "--output-dir",
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
            "metrics",
            "--no-config",
            "--no-ignore",
            "--paths",
            dir.path().to_str().unwrap(),
            "-O",
            "json",
            "--output-dir",
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
            "metrics",
            "--no-config",
            "--paths-from",
            listfile.to_str().unwrap(),
            "-O",
            "json",
            "--output-dir",
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
            "metrics",
            "--no-config",
            "--paths-from",
            "-",
            "-O",
            "json",
            "--output-dir",
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
            "metrics",
            "--no-config",
            "--paths",
            keep.to_str().unwrap(),
            "--paths-from",
            listfile.to_str().unwrap(),
            "-O",
            "json",
            "--output-dir",
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
    // Line has trailing spaces and a tab. If the trim regressed, the seed
    // `<keep>  \t` would fail `seed_kind` and production would
    // `die("path does not exist: ...")` with exit 1 — caught by both the
    // `.success()` below and the `does not exist` stderr guard.
    std::fs::write(&listfile, format!("{}  \t\n\n   \n", keep.display())).unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();

    cli(dir.path())
        .args([
            "metrics",
            "--no-config",
            "--paths-from",
            listfile.to_str().unwrap(),
            "-O",
            "json",
            "--output-dir",
            out.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("does not exist").not());

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
            "metrics",
            "--no-config",
            "--paths-from",
            missing.to_str().unwrap(),
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
            "metrics",
            "--no-config",
            "--paths-from",
            listfile.to_str().unwrap(),
            "-O",
            "json",
            "--output-dir",
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

/// #596: a walk command with no `--paths` (and no manifest seeds)
/// defaults to `.`, analyzing the directory the user is standing in
/// rather than silently producing nothing with exit 0.
#[test]
fn no_paths_defaults_to_current_directory() {
    let dir = TempDir::new().unwrap();
    let (keep, _skip) = make_tree(dir.path());
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();

    cli(dir.path())
        .current_dir(dir.path())
        .args([
            "metrics",
            "--no-config",
            "-O",
            "json",
            "--output-dir",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let names = json_files(&out);
    assert!(
        names.iter().any(|n| n == "keep.py.json"),
        "bare `bca metrics` must walk the cwd and analyze keep.py, got {names:?} (keep={})",
        keep.display(),
    );
}

/// #596: an explicitly-supplied `--paths` entry that does not exist is a
/// tool error (exit 1) naming the offending path, not a skipped warning
/// with exit 0. Guards against a typo silently analyzing nothing.
#[test]
fn nonexistent_explicit_path_fails() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("no-such-dir");

    cli(dir.path())
        .args([
            "metrics",
            "--no-config",
            "--paths",
            missing.to_str().unwrap(),
            "-O",
            "json",
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(
            predicate::str::contains("does not exist").and(predicate::str::contains("no-such-dir")),
        );
}

/// #596: a nonexistent path sourced from `--paths-from` is explicitly
/// supplied too, so it must fail the same way as a bad `--paths` entry.
#[test]
fn nonexistent_paths_from_entry_fails() {
    let dir = TempDir::new().unwrap();
    let listfile = dir.path().join("paths.txt");
    let missing = dir.path().join("ghost.py");
    std::fs::write(&listfile, format!("{}\n", missing.display())).unwrap();

    cli(dir.path())
        .args([
            "metrics",
            "--no-config",
            "--paths-from",
            listfile.to_str().unwrap(),
            "-O",
            "json",
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(
            predicate::str::contains("does not exist").and(predicate::str::contains("ghost.py")),
        );
}

/// #596: a walk that resolves zero files (here, an `--include` that
/// matches nothing) prints a stderr notice instead of being a silent
/// no-op. Non-gate commands still exit 0.
#[test]
fn empty_match_emits_stderr_notice() {
    let dir = TempDir::new().unwrap();
    let _ = make_tree(dir.path());
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();

    cli(dir.path())
        .args([
            "metrics",
            "--no-config",
            "--paths",
            dir.path().to_str().unwrap(),
            "--include",
            "**/*.nonesuch",
            "-O",
            "json",
            "--output-dir",
            out.to_str().unwrap(),
        ])
        .assert()
        .success()
        // #609: the notice carries the unified bare lowercase `warning:`
        // prefix — not the old redundant `bca: warning:` double prefix.
        .stderr(predicate::str::contains("warning: 0 files matched"))
        .stderr(predicate::str::contains("bca: warning:").not());

    assert!(
        json_files(&out).is_empty(),
        "no files should be emitted when nothing matched",
    );
}

/// Read a `metrics --output <FILE>` aggregate document and return the
/// number of per-file records (the top-level array length). One element
/// per analyzed file, so a double-counted file inflates this.
fn aggregate_len(path: &Path) -> usize {
    let doc: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).unwrap()).expect("aggregate is valid JSON");
    doc.as_array()
        .expect("aggregate is a top-level array")
        .len()
}

/// #704: overlapping seeds (a directory plus an explicit file already
/// inside it) must contribute each file exactly once. Before the dedup
/// fix the file reachable from both seeds was analyzed and counted twice,
/// inflating every file-level aggregate.
#[test]
fn overlapping_seeds_do_not_double_count() {
    let dir = TempDir::new().unwrap();
    let (keep, _skip) = make_tree(dir.path());
    let out = dir.path().join("agg.json");

    cli(dir.path())
        .args([
            "metrics",
            "--no-config",
            "--no-ignore", // so skip.py is not filtered; we control the set via seeds
            "--paths",
            keep.parent().unwrap().to_str().unwrap(), // the src/ directory
            "--paths",
            keep.to_str().unwrap(), // and the same file again, explicitly
            "-O",
            "json",
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    // src/ holds exactly keep.py + skip.py = 2 unique files; the duplicate
    // explicit `keep.py` seed must NOT add a third record.
    assert_eq!(
        aggregate_len(&out),
        2,
        "overlapping dir + file seeds must yield 2 unique records, not 3",
    );
}

/// #704: the same file named twice via two explicit `--paths` seeds is
/// also deduplicated.
#[test]
fn duplicate_explicit_file_seed_is_deduped() {
    let dir = TempDir::new().unwrap();
    let (keep, _skip) = make_tree(dir.path());
    let out = dir.path().join("agg.json");

    cli(dir.path())
        .args([
            "metrics",
            "--no-config",
            "--paths",
            keep.to_str().unwrap(),
            "--paths",
            keep.to_str().unwrap(),
            "-O",
            "json",
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert_eq!(
        aggregate_len(&out),
        1,
        "the same file named twice must produce one record",
    );
}

/// #704: a `--paths-from` line whose bytes are not valid UTF-8 must not
/// abort the whole list — the rest of the crate tolerates non-UTF-8
/// paths, so `--paths-from` must too. Unix-only: there is no stable
/// byte→path view on other platforms.
#[cfg(unix)]
#[test]
fn paths_from_tolerates_non_utf8_line() {
    use std::os::unix::ffi::OsStrExt;

    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();

    // A valid file plus one whose name carries a non-UTF-8 byte (0x80, a
    // lone continuation byte). Both must be analyzed.
    let good = src.join("good.py");
    std::fs::write(&good, "def f(): return 1\n").unwrap();
    let mut bad_name = std::ffi::OsString::from("bad");
    bad_name.push(std::ffi::OsStr::from_bytes(b"\x80"));
    bad_name.push(".py");
    let bad = src.join(&bad_name);
    if std::fs::write(&bad, "def g(): return 2\n").is_err() {
        // Some filesystems (notably macOS APFS) reject non-UTF-8 file
        // names outright, so the non-UTF-8 `--paths-from` tolerance can
        // only be exercised where the OS can represent such a path. Skip
        // rather than fail where the scenario is unrepresentable.
        return;
    }

    // Build the listfile from raw bytes so the non-UTF-8 path survives.
    let mut listing = Vec::new();
    listing.extend_from_slice(good.as_os_str().as_bytes());
    listing.push(b'\n');
    listing.extend_from_slice(bad.as_os_str().as_bytes());
    listing.push(b'\n');
    let listfile = dir.path().join("paths.txt");
    std::fs::write(&listfile, &listing).unwrap();

    let out = dir.path().join("agg.json");
    cli(dir.path())
        .args([
            "metrics",
            "--no-config",
            "--paths-from",
            listfile.to_str().unwrap(),
            "-O",
            "json",
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Both files analyzed: the non-UTF-8 line did not abort the list.
    assert_eq!(
        aggregate_len(&out),
        2,
        "both the UTF-8 and non-UTF-8 --paths-from entries must be analyzed",
    );
}

/// #704: a per-entry walk error (here, an unreadable subdirectory) must
/// skip-with-warning and keep processing the rest of the tree, not abort
/// the whole run. Unix-only: it relies on POSIX directory permissions.
///
/// #1131 kept that tolerance and changed only what happens *after* the
/// walk: the run now exits 1, because a subtree missing from the result
/// is invisible in it. The continuation property this test exists for is
/// therefore observed on streamed stdout rather than through the
/// `--output` aggregate — a post-walk document the guard suppresses
/// precisely so a partial one cannot look complete. The two claims are
/// deliberately kept in one test: "warns", "continues", and "then fails"
/// are one contract, and splitting them invites a future change to
/// satisfy one while dropping another.
#[cfg(unix)]
#[test]
fn unreadable_subdir_warns_continues_then_fails_the_run() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("ok.py"), "def f(): return 1\n").unwrap();

    // A subdirectory the walk cannot descend into (mode 000). `ignore`
    // surfaces the EACCES as a per-entry error.
    //
    // The probe is `read_dir`, not `fs::read`: root ignores mode bits
    // for a directory listing, but `fs::read` returns `EISDIR` whatever
    // the mode, so probing with it reports a denial that is not there
    // and the test fails instead of skipping under a privileged runner.
    let locked = src.join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::write(locked.join("hidden.py"), "def g(): return 2\n").unwrap();
    if !common::deny_dir_listing(&locked) {
        eprintln!("skipping: this process can list a mode-000 directory");
        return;
    }

    cli(dir.path())
        .args(["metrics", "--no-config", "--paths", src.to_str().unwrap()])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("skipping walk entry"))
        .stderr(predicate::str::contains(
            "1 directory entry could not be read",
        ))
        // The readable sibling was still analyzed despite the locked
        // subdir — the #704 property. `hidden.py` is the file the walk
        // lost, and its absence is what the exit code above reports.
        .stdout(predicate::str::contains("ok.py"))
        .stdout(predicate::str::contains("hidden.py").not());

    // Restore permissions so the TempDir can be cleaned up on drop.
    common::restore_dir_access(&locked);
}

// --- #1114: the directory walk runs in parallel -----------------------

/// Build a wide, nested tree: enough directories that the parallel
/// walker actually splits the work, and file names whose readdir order
/// is unlikely to match sorted order.
fn wide_tree(root: &Path) -> Vec<String> {
    let mut expected = Vec::new();
    for d in 0..12 {
        let dir = root.join(format!("pkg{d}")).join("deep").join("nested");
        std::fs::create_dir_all(&dir).expect("create nested dir");
        for f in 0..15 {
            // Interleave two naming shapes so lexical order and creation
            // order differ.
            for stem in [format!("z{f}_mod"), format!("a{f}-impl")] {
                let path = dir.join(format!("{stem}.rs"));
                std::fs::write(&path, format!("pub fn f{f}() -> u32 {{ {f} }}\n"))
                    .expect("write fixture");
                expected.push(format!("{stem}.rs"));
            }
        }
    }
    expected.sort();
    expected
}

/// The parallel walk (#1114) must find exactly the same files the
/// single-threaded one did — no entry lost to a race, none duplicated
/// across walker threads.
#[test]
fn parallel_walk_finds_every_file_at_every_job_count() {
    let home = TempDir::new().unwrap();
    let tree = TempDir::new().unwrap();
    let expected = wide_tree(tree.path());
    let root = tree.path().to_str().expect("utf8 tree path");

    for jobs in ["1", "2", "8", "16"] {
        let out = cli(home.path())
            .args([
                "metrics",
                "--no-config",
                "--paths",
                root,
                "--format",
                "json",
            ])
            .args(["--jobs", jobs])
            .output()
            .expect("bca runs");
        let stdout = String::from_utf8(out.stdout).expect("utf8 stdout");
        // One JSON document per file; take each `name` and keep the
        // path in full, so a file walked twice shows up as a duplicate
        // rather than collapsing into its same-named sibling.
        let mut found: Vec<String> = stdout
            .lines()
            .filter_map(|l| l.split_once("\"name\":\""))
            .filter_map(|(_, rest)| rest.split_once('"'))
            .map(|(name, _)| name.to_owned())
            .collect();
        let total = found.len();
        found.sort();
        found.dedup();
        assert_eq!(
            found.len(),
            total,
            "--jobs {jobs} walked at least one file twice"
        );
        // `Path::file_name` rather than splitting on '/': the emitted
        // name carries the platform separator, so on Windows a manual
        // '/' split returns the whole `C:\…\a0-impl.rs` path and every
        // basename mismatches.
        let mut basenames: Vec<String> = found
            .iter()
            .filter_map(|p| Path::new(p).file_name()?.to_str().map(str::to_owned))
            .collect();
        basenames.sort();
        assert_eq!(
            basenames, expected,
            "--jobs {jobs} walked a different file set than expected"
        );
    }
}
