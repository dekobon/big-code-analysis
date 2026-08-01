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
//! The fourth part (#1131) moves the same rule one directory level up: a
//! directory the walk could not *list* drops its whole subtree before
//! any file is selected, so the per-file tally stayed zero and every
//! subcommand — `bca check` included — reported success over a tree it
//! had not read. Its negative case pins the deliberate exemption: only
//! walk errors carrying an `io::Error` are fatal, so a malformed
//! `.gitignore` keeps warning and keeps exiting 0.
//!
//! The fifth part covers what that sweep missed: `bca vcs`, whose
//! emission runs through `formats::write_text` rather than the walk's
//! stdout helpers. `Stdout` is a `LineWriter` over a 1 KiB buffer, so a
//! shorter payload containing no newline at all is accepted into the
//! buffer and only written during the exit-time cleanup flush, whose
//! error nobody reads — the run exits 0 having emitted nothing. `vcs`'s
//! *compact* JSON is the only document `bca` prints with that shape;
//! everything else is line-oriented or pretty-printed, so the buffer
//! spills on an interior newline. The matching hole in
//! `path_io::write_stdout_parts_or_die` cannot be reached from any
//! subcommand and is pinned by a unit test on `write_parts_flushed`
//! instead.
//!
//! Unix-only, because the scenarios are staged with a mode-000 file, a
//! mode-000 directory, and `/dev/full`.
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

    /// The traversal-side summary line (#1131). Mirrors [`SUMMARY`], but
    /// counts unreadable *entries*: an unlistable directory costs the run
    /// a whole subtree, not one file.
    const WALK_SUMMARY: &str = "1 directory entry could not be read";

    /// Stage the #1131 tree under a fresh tempdir: one readable source
    /// file at the top, and a subdirectory holding another that the
    /// process cannot list. Returns the tempdir plus the locked
    /// directory, or `None` when the denial does not bite.
    ///
    /// The readable file matters. Without it the walk resolves zero
    /// files, and `check` would exit 1 through its pre-existing
    /// "no input files matched" guard — an exit code that looks
    /// identical to the one under test.
    fn tree_with_unlistable_subdir() -> Option<(TempDir, std::path::PathBuf)> {
        let dir = TempDir::new().expect("tempdir");
        write_fixture(&dir, "top.c", TRIVIAL_C);
        let locked = common::unlistable_dir(dir.path(), "sub", "inner.c", TRIVIAL_C)?;
        Some((dir, locked))
    }

    /// Drive `subcommand` over a tree containing a directory it cannot
    /// list and assert the #1131 contract: exit 1, the per-entry warning,
    /// and the summary line.
    ///
    /// Before the fix every one of these exited 0 with the subtree
    /// silently missing from the result.
    fn assert_unlistable_directory_exits_one(subcommand: &str, extra: &[&str]) {
        let Some((dir, locked)) = tree_with_unlistable_subdir() else {
            eprintln!("skipping: this process can list a mode-000 directory");
            return;
        };

        cli(&dir)
            .arg(subcommand)
            .args(extra)
            .args([
                "--no-config",
                "--paths",
                dir.path().to_str().expect("utf8 dir"),
            ])
            .assert()
            .code(1)
            .stderr(predicate::str::contains("skipping walk entry"))
            .stderr(predicate::str::contains("Permission denied"))
            .stderr(predicate::str::contains(WALK_SUMMARY));

        common::restore_dir_access(&locked);
    }

    #[test]
    fn metrics_exits_one_when_a_directory_cannot_be_listed() {
        assert_unlistable_directory_exits_one("metrics", &[]);
    }

    #[test]
    fn count_exits_one_when_a_directory_cannot_be_listed() {
        assert_unlistable_directory_exits_one(
            "count",
            &["--type", "function_definition", "--language", "c"],
        );
    }

    /// `strip-comments` resolves its own file list before dispatching
    /// (it rejects a multi-file `--output`), so it reaches the guard
    /// through `run_walk_resolved` rather than `run_walk` — a separate
    /// path that has to be handed the tally explicitly.
    #[test]
    fn strip_comments_exits_one_when_a_directory_cannot_be_listed() {
        assert_unlistable_directory_exits_one("strip-comments", &[]);
    }

    /// The headline case. `bca check` is the CI gate, so reporting clean
    /// on a tree it could not fully read is worse than a wrong diff —
    /// it is indistinguishable from success.
    ///
    /// The threshold is deliberately one the readable file *breaches*
    /// (`add` has one exit; the limit is 0), so the gate has a verdict to
    /// report and would exit 2 if the walk guard did not pre-empt it.
    /// Exit 1 therefore proves both halves of the contract: the run fails,
    /// and it fails as a tool error rather than as a metric regression.
    /// A threshold the tree passes would leave exit 1 consistent with
    /// "clean gate plus tool error", which is the weaker claim.
    #[test]
    fn check_exits_one_rather_than_two_when_a_directory_cannot_be_listed() {
        assert_unlistable_directory_exits_one("check", &["--threshold", "nexits=0"]);
    }

    /// The same threshold against the same tree, nothing locked: exit 2.
    /// Without this control the test above cannot tell the tool-error
    /// exit from a gate that simply never fired.
    #[test]
    fn check_exits_two_on_the_same_threshold_when_the_tree_is_readable() {
        let dir = TempDir::new().expect("tempdir");
        write_fixture(&dir, "top.c", TRIVIAL_C);

        cli(&dir)
            .args([
                "check",
                "--threshold",
                "nexits=0",
                "--no-config",
                "--paths",
                dir.path().to_str().expect("utf8 dir"),
            ])
            .assert()
            .code(2)
            .stderr(predicate::str::contains("could not be read").not());
    }

    /// The negative case, and the reason the tally is filtered on
    /// `ignore::Error::io_error()` rather than counting every error the
    /// walker reports.
    ///
    /// A malformed `.gitignore` in a *parent* of the walk root reaches
    /// the same `Err` arm as an unlistable directory — the parallel
    /// walker visits `add_parents` failures as errors — but it describes
    /// how the walk was configured, not files it lost. It must keep
    /// warning and keep exiting 0. Drop the `io_error()` filter and this
    /// test is the one that fails.
    ///
    /// The parent placement is load-bearing: a malformed `.gitignore` in
    /// the walk root or below is attached to the `DirEntry`
    /// (`Worker::read_dir` sets `dent.err`) and never reaches a visitor
    /// at all, so the same fixture one directory lower produces no
    /// warning and pins nothing. Measured against `ignore` 0.4.31.
    #[test]
    fn malformed_parent_gitignore_warns_but_still_exits_zero() {
        let dir = TempDir::new().expect("tempdir");
        // `[z-a]` is a reversed character range: globset rejects it, so
        // `ignore` reports the line as an `Error::Glob` carrying no
        // `io::Error`.
        fs::write(dir.path().join(".gitignore"), "[z-a]\n").expect("write gitignore");
        let root = dir.path().join("root");
        fs::create_dir(&root).expect("create walk root");
        fs::write(root.join("top.c"), TRIVIAL_C).expect("write fixture");

        common::cli_in(&root)
            .args([
                "metrics",
                "--no-config",
                "--paths",
                root.to_str().expect("utf8 root"),
            ])
            .assert()
            .success()
            .stdout(predicate::str::contains("top.c"))
            // The warning is half the contract: a benign walk error must
            // stay *visible*, just not fatal.
            .stderr(predicate::str::contains("invalid range"))
            .stderr(predicate::str::contains("could not be read").not());
    }

    /// The escape hatch for a tree that legitimately contains a
    /// directory you cannot list — and the asymmetry with the read-side
    /// hatch that makes it worth pinning.
    ///
    /// An *ignore file* prunes the directory inside the walker, which
    /// never descends and so never fails. `--exclude` does **not**: it is
    /// a post-walk filter over the paths the walker yielded, so the
    /// listing has already been attempted and already failed by the time
    /// it applies. Both halves are asserted here, because documenting
    /// only the working one would send a user to the flag that cannot
    /// help.
    #[test]
    fn ignore_file_prunes_an_unlistable_directory_but_exclude_does_not() {
        let Some((dir, locked)) = tree_with_unlistable_subdir() else {
            eprintln!("skipping: this process can list a mode-000 directory");
            return;
        };
        let root = dir.path().to_str().expect("utf8 dir").to_owned();

        cli(&dir)
            .args([
                "metrics",
                "--no-config",
                "--paths",
                &root,
                "--exclude",
                "**/sub/**",
            ])
            .assert()
            .code(1)
            .stderr(predicate::str::contains(WALK_SUMMARY));

        fs::write(dir.path().join(".gitignore"), "sub/\n").expect("write gitignore");
        cli(&dir)
            .args(["metrics", "--no-config", "--paths", &root])
            .assert()
            .success()
            .stdout(predicate::str::contains("top.c"))
            .stderr(predicate::str::contains("could not be read").not());

        common::restore_dir_access(&locked);
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
        common::restore_dir_access(std::path::Path::new(path));
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

    /// Run `git <args>` in `dir` with fixed identities, reporting
    /// whether it succeeded. `git` may be absent or refuse to build a
    /// repo (a sandbox with no writable config, a hostile `GIT_*`
    /// environment), which is a skip rather than a failure — the same
    /// probe-the-capability convention [`dev_full`] and
    /// `unreadable_fixture` follow.
    fn git(dir: &TempDir, args: &[&str]) -> bool {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .env("GIT_AUTHOR_NAME", "Ada")
            .env("GIT_AUTHOR_EMAIL", "ada@example.com")
            .env("GIT_COMMITTER_NAME", "Ada")
            .env("GIT_COMMITTER_EMAIL", "ada@example.com")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    /// `git` that refuses to be skipped past.
    ///
    /// Only the initial `git init` may legitimately fail — a machine
    /// without git. Once a repository exists, every later command
    /// operates on a directory this test owns, so a failure is a defect
    /// in the fixture, not an environment we should tolerate. Returning
    /// `None` there turned an unrelated host misconfiguration into four
    /// silently-passing tests.
    fn git_or_panic(dir: &TempDir, args: &[&str]) {
        assert!(git(dir, args), "git {args:?} failed in the vcs fixture");
    }

    /// `git init` plus the host-config neutralisation every fixture in
    /// this workspace needs, returning `None` only when git is absent.
    ///
    /// `commit.gpgsign` alone is not enough. A global `core.hooksPath`
    /// with a failing `pre-commit` makes every `git commit` here fail,
    /// and because the old helper reported that as "no usable git", the
    /// entire `bca vcs` stdout contract — both the ENOSPC cases and the
    /// `BrokenPipe` case — evaporated into skips on such a machine.
    /// `blame_fixture` in `src/vcs_command.rs` already neutralises this
    /// class (#941); these fixtures now do the same.
    fn git_init_neutralized() -> Option<TempDir> {
        let dir = TempDir::new().expect("tempdir");
        if !git(&dir, &["init", "-q", "-b", "main"]) {
            return None;
        }
        let hooks = dir.path().join("empty-hooks");
        fs::create_dir_all(&hooks).expect("create empty hooks dir");
        git_or_panic(&dir, &["config", "commit.gpgsign", "false"]);
        git_or_panic(
            &dir,
            &[
                "config",
                "core.hooksPath",
                hooks.to_str().expect("utf8 hooks path"),
            ],
        );
        git_or_panic(&dir, &["config", "gc.auto", "0"]);
        git_or_panic(&dir, &["config", "core.autocrlf", "false"]);
        Some(dir)
    }

    /// A throwaway repo with two commits touching one file, which is
    /// the least history `bca vcs` / `vcs commit` / `vcs trend` all
    /// produce a ranked document from.
    fn git_repo_with_history() -> Option<TempDir> {
        let dir = git_init_neutralized()?;
        write_fixture(&dir, "work.c", TRIVIAL_C);
        git_or_panic(&dir, &["add", "."]);
        git_or_panic(&dir, &["commit", "-qm", "add work"]);
        write_fixture(&dir, "work.c", COMMENTED_C);
        git_or_panic(&dir, &["commit", "-aqm", "fix work"]);
        Some(dir)
    }

    /// #1132's sweep missed `bca vcs`, whose emission runs through
    /// `formats::write_text` rather than the walk's stdout helpers. That
    /// wrote the document with no flush, so `vcs -O json`, `vcs commit`,
    /// and `vcs trend` — the three shapes whose document is *compact*
    /// JSON — exited **0** with their output dropped. Measured before
    /// the fix: 782 / 852 / 836 bytes on this fixture, all silently
    /// discarded, while `yaml` / `toml` / `markdown` / `html` / `csv` /
    /// the default table already exited 1.
    ///
    /// What separates them is not the *trailing* newline but any newline
    /// at all: `LineWriter` flushes through the last one it finds, so a
    /// pretty-printed document surfaces the error from its interior
    /// newline even though it ends in `}`. The control run therefore
    /// asserts the payload is newline-free and inside the buffer, rather
    /// than assuming it — either property lost, and the test would pass
    /// against the unflushed code.
    fn assert_vcs_stdout_write_failure_exits_one(extra: &[&str]) {
        let Some(repo) = git_repo_with_history() else {
            eprintln!("skipping: no usable git for the `bca vcs` fixture");
            return;
        };
        let Some(full) = dev_full() else {
            eprintln!("skipping: no write-failing /dev/full on this platform");
            return;
        };
        let run = |stdout: Stdio| {
            common::std_bca_command_in(repo.path())
                .arg("vcs")
                .arg("--no-config")
                .args(extra)
                .stdout(stdout)
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn bca")
                .wait_with_output()
                .expect("wait for bca")
        };

        let failed = run(full.into());
        let stderr = String::from_utf8_lossy(&failed.stderr).into_owned();
        assert_eq!(
            failed.status.code(),
            Some(1),
            "`bca vcs {extra:?}` must exit 1 on an unwritable stdout; stderr: {stderr}"
        );
        assert!(
            stderr.contains("error: ") && stderr.contains("No space left on device"),
            "`bca vcs {extra:?}` must name the I/O failure in its own \
             diagnostic; stderr: {stderr}"
        );
        assert!(
            !stderr.contains("panicked at"),
            "`bca vcs {extra:?}` must not panic on an unwritable stdout; stderr: {stderr}"
        );

        let ok = run(Stdio::piped());
        assert!(
            ok.status.success(),
            "`bca vcs {extra:?}` must succeed with a writable stdout; stderr: {}",
            String::from_utf8_lossy(&ok.stderr)
        );
        assert!(
            !ok.stdout.is_empty() && !ok.stdout.contains(&b'\n'),
            "`bca vcs {extra:?}` must still emit a document with no newline \
             in it for this test to reach the missing flush at all; got {} \
             bytes, first newline at {:?}",
            ok.stdout.len(),
            ok.stdout.iter().position(|&b| b == b'\n'),
        );
        assert!(
            ok.stdout.len() < STDOUT_LINE_BUFFER_BYTES,
            "`bca vcs {extra:?}` emits {} bytes, at or over stdout's line \
             buffer — a payload that large is written through on its own \
             and no longer exercises the flush",
            ok.stdout.len(),
        );
    }

    /// Capacity `std::io::stdout`'s `LineWriter` is built with. A
    /// newline-free payload shorter than this is swallowed whole by the
    /// buffer, which is the precondition every assertion above depends
    /// on; a longer one spills straight to the fd and fails on its own.
    const STDOUT_LINE_BUFFER_BYTES: usize = 1_024;

    #[test]
    fn vcs_json_exits_one_when_stdout_cannot_be_written() {
        assert_vcs_stdout_write_failure_exits_one(&["-O", "json"]);
    }

    #[test]
    fn vcs_commit_exits_one_when_stdout_cannot_be_written() {
        assert_vcs_stdout_write_failure_exits_one(&["commit"]);
    }

    #[test]
    fn vcs_trend_exits_one_when_stdout_cannot_be_written() {
        assert_vcs_stdout_write_failure_exits_one(&["trend"]);
    }

    /// Capacity of a Linux pipe, and the reason the fixture below is
    /// 400 files rather than one. A document that fits here is accepted
    /// whole before the reader can close, so the child never meets the
    /// closed pipe and the assertion passes against an unfixed build.
    const PIPE_BUFFER_BYTES: usize = 64 * 1_024;

    /// A repository with enough tracked files that `bca vcs --top 0`
    /// emits more than [`PIPE_BUFFER_BYTES`] of compact JSON.
    fn git_repo_with_many_files() -> Option<TempDir> {
        let dir = git_init_neutralized()?;
        for i in 0..400 {
            write_fixture(&dir, &format!("f{i:04}.c"), TRIVIAL_C);
        }
        git_or_panic(&dir, &["add", "."]);
        git_or_panic(&dir, &["commit", "-qm", "add sources"]);
        Some(dir)
    }

    /// The pipe-close half of the #1132 contract, for the `vcs` family.
    ///
    /// `bca vcs … | head` is routine. `formats::write_text` had no
    /// `BrokenPipe` exemption and `emit`'s caller `die`d on every error,
    /// so once the flush that makes a write failure *visible* landed,
    /// a closed consumer became `error: writing vcs output: Broken pipe`
    /// and exit 1 — while `dump`, `metrics`, and `ops` piped into the
    /// same consumer exit 0. The
    /// `vcs_*_exits_one_when_stdout_cannot_be_written` tests above
    /// cannot see this: `/dev/full` fails every write with `ENOSPC`,
    /// which must stay fatal.
    ///
    /// Both runs are load-bearing. The control asserts the document
    /// really exceeds the pipe buffer, without which the child would
    /// complete its write into the buffer and never observe the close —
    /// and the test would pass against the unfixed code.
    #[test]
    fn vcs_exits_zero_when_its_consumer_closes_the_pipe() {
        use std::io::Read;

        let Some(repo) = git_repo_with_many_files() else {
            eprintln!("skipping: no usable git for the `bca vcs` fixture");
            return;
        };
        let run = || {
            common::std_bca_command_in(repo.path())
                .args(["vcs", "--no-config", "--top", "0", "--format", "json"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn bca")
        };

        let control = run().wait_with_output().expect("wait for bca");
        assert!(
            control.stdout.len() > PIPE_BUFFER_BYTES,
            "the document must outgrow the pipe buffer for the close to \
             be observable at all; got {} bytes",
            control.stdout.len(),
        );

        // Read one byte, then close the read end — the `head -c1` shape.
        let mut child = run();
        let mut stdout = child.stdout.take().expect("piped stdout");
        let mut first = [0u8; 1];
        stdout.read_exact(&mut first).expect("read first byte");
        drop(stdout);

        let output = child.wait_with_output().expect("wait for bca");
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(
            output.status.success(),
            "a closed consumer pipe is routine, not a tool error; stderr: {stderr}"
        );
        assert!(
            stderr.is_empty(),
            "a closed consumer pipe must be silent; stderr: {stderr}"
        );
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
