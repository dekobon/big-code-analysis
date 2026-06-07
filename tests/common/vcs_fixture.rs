//! Deterministic temp git-repo builder for change-history (VCS) tests.
//!
//! Commits are made through the `git` CLI with fixed author / committer
//! identities and explicit UNIX timestamps (`GIT_AUTHOR_DATE` /
//! `GIT_COMMITTER_DATE`), so windows, ages, and ordering are fully
//! reproducible. Pair with [`big_code_analysis::vcs::Options::as_of`]
//! pinned to [`FIXED_NOW`] for snapshot-stable runs.

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

/// Reference "now" for every VCS fixture test. Commit timestamps are
/// expressed as offsets before this instant.
pub const FIXED_NOW: i64 = 1_700_000_000;

/// One day in seconds, for expressing commit ages.
pub const DAY: i64 = 86_400;

/// A throwaway git repository under a `TempDir` (auto-removed on drop).
pub struct Repo {
    dir: TempDir,
}

impl Repo {
    /// Initialise an empty repo on a `main` branch with signing off.
    pub fn init() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        run_git(dir.path(), &["init", "-q", "-b", "main"], &[]);
        // Keep commits hermetic regardless of the host's global config.
        run_git(dir.path(), &["config", "commit.gpgsign", "false"], &[]);
        run_git(dir.path(), &["config", "gc.auto", "0"], &[]);
        Self { dir }
    }

    /// Repository root.
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Write (or overwrite) a file relative to the repo root, creating
    /// parent directories.
    pub fn write(&self, rel: &str, contents: &str) {
        let path = self.dir.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir -p");
        }
        std::fs::write(path, contents).expect("write file");
    }

    /// Stage everything and commit as `(name, email)` at `secs`
    /// (UNIX seconds) with `message`.
    pub fn commit(&self, name: &str, email: &str, secs: i64, message: &str) {
        self.commit_inner(name, email, secs, message);
    }

    /// Run an arbitrary `git` subcommand (e.g. `mv`) in the repo.
    pub fn git(&self, args: &[&str]) {
        run_git(self.dir.path(), args, &[]);
    }

    /// Run an arbitrary `git` subcommand with the fixed identity and a
    /// controlled author/committer date (for commands that create a
    /// commit, e.g. `merge --no-ff`).
    pub fn git_at(&self, name: &str, email: &str, secs: i64, args: &[&str]) {
        let date = format!("@{secs} +0000");
        let env = [
            ("GIT_AUTHOR_NAME", name),
            ("GIT_AUTHOR_EMAIL", email),
            ("GIT_AUTHOR_DATE", date.as_str()),
            ("GIT_COMMITTER_NAME", name),
            ("GIT_COMMITTER_EMAIL", email),
            ("GIT_COMMITTER_DATE", date.as_str()),
        ];
        run_git(self.dir.path(), args, &env);
    }

    /// Write a `.mailmap` mapping canonical identities.
    pub fn mailmap(&self, contents: &str) {
        self.write(".mailmap", contents);
    }

    fn commit_inner(&self, name: &str, email: &str, secs: i64, message: &str) {
        run_git(self.dir.path(), &["add", "-A"], &[]);
        let date = format!("@{secs} +0000");
        let env = [
            ("GIT_AUTHOR_NAME", name),
            ("GIT_AUTHOR_EMAIL", email),
            ("GIT_AUTHOR_DATE", date.as_str()),
            ("GIT_COMMITTER_NAME", name),
            ("GIT_COMMITTER_EMAIL", email),
            ("GIT_COMMITTER_DATE", date.as_str()),
        ];
        run_git(
            self.dir.path(),
            &["commit", "-q", "--allow-empty", "-m", message],
            &env,
        );
    }
}

/// Run `git <args>` in `dir` with extra environment, asserting success.
fn run_git(dir: &Path, args: &[&str], env: &[(&str, &str)]) {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(dir);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let status = cmd.status().expect("spawn git");
    assert!(status.success(), "git {args:?} failed");
}
