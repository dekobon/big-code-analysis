//! Integration tests for the unreadable-input exit contract (#1098).
//!
//! `#1060` made `bca check` exit 1 when an input file could not be read;
//! every other walking subcommand still exited 0 with only a per-file
//! `error processing <path>: …` line on stderr. These tests pin the
//! resolved contract: **any** read failure is a tool error (exit 1) for
//! every walking subcommand, whether or not the run also produced
//! output.
//!
//! Unix-only, because the scenario is staged with a mode-000 file.
//! `unreadable_fixture` probes the real capability rather than the uid,
//! so a privileged test runner (root ignores mode bits) skips instead of
//! failing. The suite lives in a `#[cfg(unix)]` module rather than
//! behind a file-level `#![cfg(unix)]`, so these crate-level docs
//! survive on Windows and the workspace `missing_docs` lint stays quiet
//! there.

mod common;

#[cfg(unix)]
mod unix {
    use std::fs;

    use assert_cmd::Command;
    use predicates::prelude::*;
    use tempfile::TempDir;

    use super::common;

    /// C source with one function. Chosen over Rust so `preproc` and
    /// `strip-comments` — which only walk C/C++ — share the fixture with
    /// every other subcommand.
    const TRIVIAL_C: &str = "int add(int a, int b) { return a + b; }\n";

    /// The summary line every walking subcommand now emits. Deliberately
    /// unprefixed by a subcommand name (`bca init` scaffolds through
    /// `run_check`, so a `check:` prefix would misattribute the failure).
    const SUMMARY: &str = "1 input file could not be read";

    /// Write `body` to `name` under `dir` and return its path.
    fn write_fixture(dir: &TempDir, name: &str, body: &str) -> String {
        let path = dir.path().join(name);
        fs::write(&path, body).expect("write fixture");
        path.to_str().expect("utf8 fixture path").to_owned()
    }

    /// Write `name` under `dir` and strip every permission bit so reading it
    /// fails with `EACCES`. Returns `None` when this process can read it
    /// anyway (root ignores mode bits), because then the scenario under test
    /// cannot be staged at all. Mirrors the helper of the same name in
    /// `check_thresholds.rs`.
    fn unreadable_fixture(dir: &TempDir, name: &str) -> Option<String> {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.path().join(name);
        fs::write(&path, TRIVIAL_C).expect("write fixture");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).expect("chmod 000");
        // Probe the actual capability rather than the uid: `fs::read`
        // succeeding here means mode bits do not deny this process.
        if fs::read(&path).is_ok() {
            return None;
        }
        Some(path.to_str().expect("utf8 fixture path").to_owned())
    }

    /// Hermetic `bca` rooted at `dir` (see `common::cli_in` for why the cwd
    /// must not be the repo).
    fn cli(dir: &TempDir) -> Command {
        common::cli_in(dir.path())
    }

    /// Drive `subcommand` (plus `extra` flags it requires) against a single
    /// unreadable file and assert the shared contract: exit 1, the per-file
    /// `error processing` line, and the summary line.
    ///
    /// Both stderr assertions matter. The exit code alone cannot tell a read
    /// failure from any other tool error — `--no-config` plus an
    /// always-valid flag set means the only exit-1 path left is the one
    /// under test — and the per-file line is what tells the user *which*
    /// file failed.
    fn assert_read_failure_exits_one(subcommand: &str, extra: &[&str]) {
        let dir = TempDir::new().expect("tempdir");
        let Some(path) = unreadable_fixture(&dir, "locked.c") else {
            eprintln!("skipping: this process can read a mode-000 file");
            return;
        };

        cli(&dir)
            .arg(subcommand)
            .args(extra)
            .args(["--no-config", "--paths", &path])
            .assert()
            .code(1)
            .stderr(predicate::str::contains("error processing"))
            .stderr(predicate::str::contains("Permission denied"))
            .stderr(predicate::str::contains(SUMMARY));
    }

    #[test]
    fn metrics_exits_one_when_an_input_file_is_unreadable() {
        assert_read_failure_exits_one("metrics", &[]);
    }

    #[test]
    fn ops_exits_one_when_an_input_file_is_unreadable() {
        assert_read_failure_exits_one("ops", &[]);
    }

    #[test]
    fn report_exits_one_when_an_input_file_is_unreadable() {
        assert_read_failure_exits_one("report", &[]);
    }

    #[test]
    fn functions_exits_one_when_an_input_file_is_unreadable() {
        assert_read_failure_exits_one("functions", &[]);
    }

    #[test]
    fn find_exits_one_when_an_input_file_is_unreadable() {
        assert_read_failure_exits_one(
            "find",
            &["--type", "function_definition", "--language", "c"],
        );
    }

    #[test]
    fn count_exits_one_when_an_input_file_is_unreadable() {
        assert_read_failure_exits_one(
            "count",
            &["--type", "function_definition", "--language", "c"],
        );
    }

    #[test]
    fn dump_exits_one_when_an_input_file_is_unreadable() {
        assert_read_failure_exits_one("dump", &[]);
    }

    #[test]
    fn exemptions_exits_one_when_an_input_file_is_unreadable() {
        assert_read_failure_exits_one("exemptions", &[]);
    }

    #[test]
    fn preproc_exits_one_when_an_input_file_is_unreadable() {
        assert_read_failure_exits_one("preproc", &[]);
    }

    #[test]
    fn strip_comments_exits_one_when_an_input_file_is_unreadable() {
        assert_read_failure_exits_one("strip-comments", &[]);
    }

    /// The mixed case: the readable half of the input still produces its
    /// output (the guard runs *after* the walk, so nothing that worked is
    /// withheld), and the run still exits 1 because the emitted document is
    /// missing a file the user asked for.
    #[test]
    fn mixed_input_emits_readable_output_and_still_exits_one() {
        let dir = TempDir::new().expect("tempdir");
        let readable = write_fixture(&dir, "readable.c", TRIVIAL_C);
        let Some(locked) = unreadable_fixture(&dir, "locked.c") else {
            eprintln!("skipping: this process can read a mode-000 file");
            return;
        };

        cli(&dir)
            .args([
                "metrics",
                "--no-config",
                "--paths",
                &readable,
                "--paths",
                &locked,
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::contains("readable.c"))
            .stderr(predicate::str::contains(SUMMARY));
    }

    /// Aggregate mode (`--output <FILE>`, #669) is the case the issue rates
    /// as most misleading: the document is written after the walk and would
    /// otherwise look complete while silently missing a file. The guard runs
    /// first, so no artifact reaches disk at all.
    #[test]
    fn aggregate_output_file_is_not_written_when_a_file_is_unreadable() {
        let dir = TempDir::new().expect("tempdir");
        let readable = write_fixture(&dir, "readable.c", TRIVIAL_C);
        let Some(locked) = unreadable_fixture(&dir, "locked.c") else {
            eprintln!("skipping: this process can read a mode-000 file");
            return;
        };
        let out = dir.path().join("aggregate.json");

        cli(&dir)
            .args([
                "metrics",
                "--no-config",
                "--format",
                "json",
                "--output",
                out.to_str().expect("utf8 output path"),
                "--paths",
                &readable,
                "--paths",
                &locked,
            ])
            .assert()
            .code(1)
            .stderr(predicate::str::contains(SUMMARY));

        assert!(
            !out.exists(),
            "a partial aggregate document must not reach disk"
        );
    }

    /// The per-subcommand cases above all *name* the locked file. This
    /// one reaches it by walking a directory instead, so the guard is
    /// pinned for files the user never mentioned — the path a future
    /// change gating the tally on `explicit_seeds` would break.
    #[test]
    fn directory_walk_exits_one_when_a_walked_file_is_unreadable() {
        let dir = TempDir::new().expect("tempdir");
        write_fixture(&dir, "readable.c", TRIVIAL_C);
        if unreadable_fixture(&dir, "locked.c").is_none() {
            eprintln!("skipping: this process can read a mode-000 file");
            return;
        }

        cli(&dir)
            .args([
                "metrics",
                "--no-config",
                "--paths",
                dir.path().to_str().expect("utf8 dir"),
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::contains("readable.c"))
            .stderr(predicate::str::contains(SUMMARY));
    }

    /// `count` assembles its tally *after* the walk, so a partial one is
    /// indistinguishable from a complete one — it is suppressed, not
    /// printed alongside the error. Pins the documented split between
    /// streamed output (still emitted, see the mixed test above) and
    /// post-walk output.
    #[test]
    fn post_walk_tally_is_suppressed_when_a_file_is_unreadable() {
        let dir = TempDir::new().expect("tempdir");
        write_fixture(&dir, "readable.c", TRIVIAL_C);
        if unreadable_fixture(&dir, "locked.c").is_none() {
            eprintln!("skipping: this process can read a mode-000 file");
            return;
        }

        cli(&dir)
            .args([
                "count",
                "--type",
                "function_definition",
                "--language",
                "c",
                "--no-config",
                "--paths",
                dir.path().to_str().expect("utf8 dir"),
            ])
            .assert()
            .code(1)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(SUMMARY));
    }

    /// The same rule for a post-walk document written to a file:
    /// `preproc --output` must leave nothing on disk.
    #[test]
    fn preproc_output_file_is_not_written_when_a_file_is_unreadable() {
        let dir = TempDir::new().expect("tempdir");
        write_fixture(&dir, "readable.c", TRIVIAL_C);
        if unreadable_fixture(&dir, "locked.c").is_none() {
            eprintln!("skipping: this process can read a mode-000 file");
            return;
        }
        let out = dir.path().join("preproc.json");

        cli(&dir)
            .args([
                "preproc",
                "--no-config",
                "--output",
                out.to_str().expect("utf8 output path"),
                "--paths",
                dir.path().to_str().expect("utf8 dir"),
            ])
            .assert()
            .code(1)
            .stderr(predicate::str::contains(SUMMARY));

        assert!(
            !out.exists(),
            "a partial preproc document must not reach disk"
        );
    }

    /// The negative test: the new guard must not turn every run non-zero.
    /// Same fixture directory, same command, nothing unreadable.
    #[test]
    fn clean_run_still_exits_zero() {
        let dir = TempDir::new().expect("tempdir");
        let readable = write_fixture(&dir, "readable.c", TRIVIAL_C);

        cli(&dir)
            .args(["metrics", "--no-config", "--paths", &readable])
            .assert()
            .success()
            .stderr(predicate::str::contains("could not be read").not());
    }

    /// A file that is never read is never a read failure. `--exclude` prunes
    /// the unreadable file during seed expansion, before any worker opens
    /// it, so the run is clean — the escape hatch for a tree that
    /// legitimately contains files this user cannot open.
    #[test]
    fn unreadable_file_excluded_by_a_filter_does_not_fail_the_run() {
        let dir = TempDir::new().expect("tempdir");
        write_fixture(&dir, "readable.c", TRIVIAL_C);
        if unreadable_fixture(&dir, "locked.c").is_none() {
            eprintln!("skipping: this process can read a mode-000 file");
            return;
        }

        cli(&dir)
            .args([
                "metrics",
                "--no-config",
                "--paths",
                dir.path().to_str().expect("utf8 dir"),
                "--exclude",
                "**/locked.c",
            ])
            .assert()
            .success()
            .stdout(predicate::str::contains("readable.c"))
            .stderr(predicate::str::contains("could not be read").not());
    }
}
