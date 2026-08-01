//! Integration tests for the walk's I/O-failure exit contract (#1098).
//!
//! `#1060` made `bca check` exit 1 when an input file could not be read;
//! every other walking subcommand still exited 0 with only a per-file
//! `error processing <path>: …` line on stderr. These tests pin the
//! resolved contract: **any** read failure is a tool error (exit 1) for
//! every walking subcommand, whether or not the run also produced
//! output.
//!
//! The second part covers the mirror image, which #1098 left open: a
//! failure to *write* a per-file document — an unwritable
//! `--output-dir`, a full disk — printed the same per-file error and
//! still exited 0, so a CI script read a missing or truncated output
//! tree as a clean run.
//!
//! The third part (#1132) covers the emission paths that reached stdout
//! through `println!`, which *panics* on a write error rather than
//! returning one: `count` and `preproc` exited 101, and `dump` / `find` /
//! `strip-comments` crashed a worker thread and reported the I/O error
//! as `Receiver("A thread used to process a file panicked")`. Those
//! cases also pin the `BrokenPipe` exemption from the other direction —
//! `bca dump | head` must stay exit 0.
//!
//! Unix-only, because the scenarios are staged with a mode-000 file and
//! `/dev/full`.
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
    use std::process::{Output, Stdio};

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

    /// [`common::unreadable_fixture`] over [`TRIVIAL_C`], with this
    /// suite's `String`-path convention.
    fn unreadable_fixture(dir: &TempDir, name: &str) -> Option<String> {
        let path = common::unreadable_fixture(dir.path(), name, TRIVIAL_C)?;
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

    /// The write-side summary line. Mirrors [`SUMMARY`].
    const WRITE_SUMMARY: &str = "1 output file could not be written";

    /// Create `name` under `dir` and strip its write bits so a per-file
    /// document cannot be created inside it. `None` when the denial does
    /// not bite (root ignores mode bits), same as `unreadable_fixture`.
    ///
    /// The caller must restore the mode before the `TempDir` drops, or
    /// cleanup fails.
    fn readonly_dir(dir: &TempDir, name: &str) -> Option<String> {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.path().join(name);
        fs::create_dir(&path).expect("create output dir");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o555)).expect("chmod 555");
        // Probe the real capability: creating a file here must fail.
        if fs::write(path.join(".probe"), b"x").is_ok() {
            return None;
        }
        Some(path.to_str().expect("utf8 dir path").to_owned())
    }

    /// Run `subcommand` over one perfectly readable file, writing JSON
    /// into `output_dir`, and return the assertion.
    fn run_into_output_dir(
        dir: &TempDir,
        subcommand: &str,
        source: &str,
        output_dir: &str,
    ) -> assert_cmd::assert::Assert {
        cli(dir)
            .arg(subcommand)
            .args([
                "--no-config",
                "--paths",
                source,
                "-O",
                "json",
                "--output-dir",
                output_dir,
            ])
            .assert()
    }

    /// #1098's mirror image: the input read fine, the output document
    /// did not, and that must be exit 1 rather than a silent success.
    ///
    /// The writable control run in the same test is what keeps this
    /// honest — without it, any unrelated exit-1 path (a rejected flag,
    /// a missing fixture) would satisfy the assertion.
    fn assert_write_failure_exits_one(subcommand: &str) {
        let dir = TempDir::new().expect("tempdir");
        let source = write_fixture(&dir, "ok.c", TRIVIAL_C);
        let Some(locked) = readonly_dir(&dir, "locked-out") else {
            eprintln!("skipping: this process can write into a mode-555 directory");
            return;
        };

        run_into_output_dir(&dir, subcommand, &source, &locked)
            .code(1)
            .stderr(predicate::str::contains("error processing"))
            .stderr(predicate::str::contains("Permission denied"))
            .stderr(predicate::str::contains(WRITE_SUMMARY));

        // Same command, writable destination: exit 0 and no summary. The
        // exit-1 above therefore came from the write, not the invocation.
        let open = dir.path().join("open-out");
        run_into_output_dir(
            &dir,
            subcommand,
            &source,
            open.to_str().expect("utf8 dir path"),
        )
        .success()
        .stderr(predicate::str::contains("could not be written").not());

        restore_dir_permissions(&locked);
    }

    /// Give the mode-555 fixture its write bits back so `TempDir`'s
    /// recursive delete can remove it.
    fn restore_dir_permissions(path: &str) {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod 755");
    }

    #[test]
    fn metrics_exits_one_when_an_output_document_cannot_be_written() {
        assert_write_failure_exits_one("metrics");
    }

    #[test]
    fn ops_exits_one_when_an_output_document_cannot_be_written() {
        assert_write_failure_exits_one("ops");
    }

    /// Every failing file is counted, not just the first — the summary
    /// is what tells the user how much of the output tree is missing.
    #[test]
    fn every_unwritable_output_document_is_counted() {
        let dir = TempDir::new().expect("tempdir");
        let sources = dir.path().join("src");
        fs::create_dir(&sources).expect("create src dir");
        for name in ["a.c", "b.c", "c.c"] {
            fs::write(sources.join(name), TRIVIAL_C).expect("write fixture");
        }
        let Some(locked) = readonly_dir(&dir, "locked-out") else {
            eprintln!("skipping: this process can write into a mode-555 directory");
            return;
        };

        run_into_output_dir(
            &dir,
            "metrics",
            sources.to_str().expect("utf8 src path"),
            &locked,
        )
        .code(1)
        .stderr(predicate::str::contains(
            "3 output files could not be written",
        ));

        restore_dir_permissions(&locked);
    }

    /// C source carrying a comment, so `strip-comments` has something to
    /// emit — over [`TRIVIAL_C`] it produces no output at all and its
    /// stdout is never written.
    const COMMENTED_C: &str = "/* doc */\nint add(int a, int b) { return a + b; }\n";

    /// Open `/dev/full` for writing, or `None` when the platform has no
    /// such device (it is a Linux-ism) or its writes unexpectedly succeed.
    ///
    /// The mode-555 directory above cannot stage this scenario: stdout is
    /// inherited from the parent, never opened by `bca`, so the only way
    /// to make it unwritable is to hand the child a file descriptor that
    /// fails every write. The capability is probed rather than inferred
    /// from the platform, matching `unreadable_fixture`.
    fn dev_full() -> Option<fs::File> {
        use std::io::Write;

        let mut file = fs::OpenOptions::new().write(true).open("/dev/full").ok()?;
        file.write_all(b"probe").is_err().then_some(file)
    }

    /// Run `subcommand` over `source` with the child's stdout pointed at
    /// `stdout`, returning its status and captured stderr.
    fn run_with_stdout(
        dir: &TempDir,
        subcommand: &str,
        extra: &[&str],
        source: &str,
        stdout: Stdio,
    ) -> Output {
        common::std_bca_command_in(dir.path())
            .arg(subcommand)
            .args(extra)
            .args(["--no-config", "--paths", source])
            .stdout(stdout)
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn bca")
            .wait_with_output()
            .expect("wait for bca")
    }

    /// #1132: a subcommand whose *stdout* cannot be written must exit
    /// `EXIT_TOOL_ERROR` with the CLI's own `error:` diagnostic — never a
    /// panic. `count` and `preproc` exited 101 (a `println!` panic on
    /// `main`); `dump`, `find`, and `strip-comments` panicked a worker and
    /// surfaced it as `Receiver("A thread used to process a file
    /// panicked")`, which misreports an I/O error as a thread crash.
    ///
    /// The two panic-absence assertions are the load-bearing half. `dump`
    /// already exited 1 before the fix, by way of that caught worker
    /// panic, so an exit-code-only test passes against the bug it claims
    /// to guard. The positive assertions (`error:` plus the OS message)
    /// are what keep the absence checks from passing vacuously.
    fn assert_stdout_write_failure_exits_one(subcommand: &str, extra: &[&str], body: &str) {
        let dir = TempDir::new().expect("tempdir");
        let source = write_fixture(&dir, "ok.c", body);
        let Some(full) = dev_full() else {
            eprintln!("skipping: no write-failing /dev/full on this platform");
            return;
        };

        let failed = run_with_stdout(&dir, subcommand, extra, &source, full.into());
        let stderr = String::from_utf8_lossy(&failed.stderr).into_owned();
        assert_eq!(
            failed.status.code(),
            Some(1),
            "`bca {subcommand}` must exit 1 on an unwritable stdout; stderr: {stderr}"
        );
        assert!(
            stderr.contains("error: "),
            "`bca {subcommand}` must emit the CLI's own diagnostic; stderr: {stderr}"
        );
        assert!(
            stderr.contains("No space left on device"),
            "the diagnostic must name the I/O failure; stderr: {stderr}"
        );
        assert!(
            !stderr.contains("panicked at"),
            "`bca {subcommand}` must not panic on an unwritable stdout; stderr: {stderr}"
        );
        assert!(
            !stderr.contains("RUST_BACKTRACE"),
            "`bca {subcommand}` must not emit a panic backtrace note; stderr: {stderr}"
        );

        // Control: the same invocation against a writable stdout exits 0,
        // so the exit-1 above came from the write and not from a rejected
        // flag set or an unusable fixture.
        let ok = run_with_stdout(&dir, subcommand, extra, &source, Stdio::null());
        assert!(
            ok.status.success(),
            "`bca {subcommand}` must succeed with a writable stdout; stderr: {}",
            String::from_utf8_lossy(&ok.stderr)
        );
    }

    #[test]
    fn count_exits_one_when_stdout_cannot_be_written() {
        assert_stdout_write_failure_exits_one(
            "count",
            &["--type", "function_definition", "--language", "c"],
            TRIVIAL_C,
        );
    }

    #[test]
    fn dump_exits_one_when_stdout_cannot_be_written() {
        assert_stdout_write_failure_exits_one("dump", &[], TRIVIAL_C);
    }

    #[test]
    fn find_exits_one_when_stdout_cannot_be_written() {
        assert_stdout_write_failure_exits_one(
            "find",
            &["--type", "function_definition", "--language", "c"],
            TRIVIAL_C,
        );
    }

    #[test]
    fn strip_comments_exits_one_when_stdout_cannot_be_written() {
        assert_stdout_write_failure_exits_one("strip-comments", &[], COMMENTED_C);
    }

    #[test]
    fn preproc_exits_one_when_stdout_cannot_be_written() {
        assert_stdout_write_failure_exits_one("preproc", &[], TRIVIAL_C);
    }

    /// The other half of the contract: `BrokenPipe` is not a tool error.
    /// `bca dump | head -1` is routine, and converting the banner from
    /// `println!` to a fallible write is exactly the change that could
    /// turn it into an exit-1 (or, as before this fix, an exit-1 *and* a
    /// screenful of panic text — the pre-#1132 behaviour this pins
    /// against).
    ///
    /// The fixture shape is load-bearing twice over, and a simpler one
    /// makes the test vacuous:
    ///
    /// - Each file must dump more than a pipe buffer (~64 KiB) so the
    ///   child is still blocked in a write when the read end closes,
    ///   rather than finishing into the buffer and never seeing `EPIPE`.
    /// - There must be *several* files, because the banner is what this
    ///   pins and the first one is written before the pipe fills. Workers
    ///   serialise on the stdout lock, so exactly one banner precedes the
    ///   block and every later one meets a closed pipe. Against a
    ///   single-file fixture this test passes with the `println!` banner
    ///   restored — measured, not assumed.
    #[test]
    fn dump_exits_zero_when_its_consumer_closes_the_pipe() {
        use std::fmt::Write as _;
        use std::io::{BufRead, BufReader};

        let dir = TempDir::new().expect("tempdir");
        let mut body = String::new();
        for i in 0..400 {
            writeln!(body, "int f{i}(int a, int b) {{ return a + b * {i}; }}")
                .expect("writing to a String is infallible");
        }
        for name in ["one.c", "two.c", "three.c"] {
            write_fixture(&dir, name, &body);
        }
        let source = dir.path().to_str().expect("utf8 dir").to_owned();

        let mut child = common::std_bca_command_in(dir.path())
            .args(["dump", "--no-config", "--paths", &source])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn bca");

        // Read one line, then close the read end — the `head -1` shape.
        let stdout = child.stdout.take().expect("piped stdout");
        let mut reader = BufReader::new(stdout);
        let mut first = String::new();
        reader.read_line(&mut first).expect("read first line");
        assert!(
            first.starts_with("== "),
            "the first line should be the per-file banner, got {first:?}"
        );
        drop(reader);

        let output = child.wait_with_output().expect("wait for bca");
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(
            output.status.success(),
            "a closed consumer pipe is routine, not a tool error; stderr: {stderr}"
        );
        assert!(
            !stderr.contains("panicked at"),
            "a closed consumer pipe must not panic a worker; stderr: {stderr}"
        );
        assert!(
            stderr.is_empty(),
            "a closed consumer pipe must be silent; stderr: {stderr}"
        );
    }
}
