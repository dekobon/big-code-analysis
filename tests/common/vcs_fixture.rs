//! Deterministic temp git-repo builder for change-history (VCS) tests.
//!
//! Commits are made through the `git` CLI with fixed author / committer
//! identities and explicit UNIX timestamps (`GIT_AUTHOR_DATE` /
//! `GIT_COMMITTER_DATE`), so windows, ages, and ordering are fully
//! reproducible. Pair with [`big_code_analysis::vcs::Options::as_of`]
//! pinned to [`FIXED_NOW`] for snapshot-stable runs.

use std::path::Path;
use std::process::Command;

use big_code_analysis::vcs::{HistoryIndex, Stats};
use tempfile::TempDir;

/// Reference "now" for every VCS fixture test. Commit timestamps are
/// expressed as offsets before this instant.
pub const FIXED_NOW: i64 = 1_700_000_000;

/// One day in seconds, for expressing commit ages.
pub const DAY: i64 = 86_400;

/// The stats `index` ranks the repo-relative file `rel` under, failing
/// loudly — naming the file and the index size — when it is absent.
pub fn stats_for<'a>(index: &'a HistoryIndex, rel: &str) -> &'a Stats {
    index
        .get(Path::new(rel))
        .unwrap_or_else(|| panic!("expected stats for {rel}; index has {} files", index.len()))
}

/// A throwaway git repository under a `TempDir` (auto-removed on drop).
pub struct Repo {
    dir: TempDir,
}

impl Repo {
    /// Initialise an empty repo on a `main` branch with signing off.
    pub fn init() -> Self {
        Self::init_with_env(&[])
    }

    /// [`init`](Self::init) with extra environment on the `git init`
    /// itself, so a test can point it at a hostile host configuration and
    /// check the fixture does not read it.
    pub fn init_with_env(env: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        run_git(dir.path(), &["init", "-q", "-b", "main"], env);
        configure(dir.path());
        Self { dir }
    }

    /// Clone this repository shallowly at `depth` into a fresh temp dir,
    /// configured like [`init`](Self::init). The `file://` spelling forces
    /// the non-local transport, since a bare path clone ignores
    /// `--depth`.
    pub fn shallow_clone(&self, depth: u32) -> Self {
        self.shallow_clone_with_env(depth, &[])
    }

    /// [`shallow_clone`](Self::shallow_clone) with extra environment on
    /// the `git clone` itself, so a test can hand it a hostile host
    /// configuration and check the clone does not consult it.
    pub fn shallow_clone_with_env(&self, depth: u32, env: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let url = format!("file://{}", self.dir.path().display());
        let depth = depth.to_string();
        let dest = dir.path().to_str().expect("tempdir path is valid UTF-8");
        run_git(
            self.dir.path(),
            &["clone", "--quiet", "--depth", &depth, &url, dest],
            env,
        );
        configure(dir.path());
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
        self.commit_with_env(name, email, secs, message, &[]);
    }

    /// [`commit`](Self::commit) with extra environment on the `git add`
    /// and `git commit`, so a test can hand them a hostile host
    /// configuration and check neither reads it.
    pub fn commit_with_env(
        &self,
        name: &str,
        email: &str,
        secs: i64,
        message: &str,
        env: &[(&str, &str)],
    ) {
        run_git(self.dir.path(), &["add", "-A"], env);
        let date = format!("@{secs} +0000");
        let dated = [
            ("GIT_AUTHOR_NAME", name),
            ("GIT_AUTHOR_EMAIL", email),
            ("GIT_AUTHOR_DATE", date.as_str()),
            ("GIT_COMMITTER_NAME", name),
            ("GIT_COMMITTER_EMAIL", email),
            ("GIT_COMMITTER_DATE", date.as_str()),
        ];
        let env: Vec<(&str, &str)> = env.iter().chain(&dated).copied().collect();
        run_git(
            self.dir.path(),
            // `--no-verify` skips any commit/commit-msg hook a
            // contributor's global `core.hooksPath` might install.
            &[
                "commit",
                "-q",
                "--no-verify",
                "--allow-empty",
                "-m",
                message,
            ],
            &env,
        );
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
}

/// Pin the repository at `dir` to a hermetic configuration.
///
/// [`run_git`] already keeps the host's global and system files away from
/// the fixture's own `git`; these pins are written into the repository as
/// well, so they also hold for a `git` that does not go through it.
fn configure(dir: &Path) {
    run_git(dir, &["config", "commit.gpgsign", "false"], &[]);
    // Disable hooks for *every* commit-producing command, not just
    // the ones that remember `--no-verify`. `git_at` forwards a
    // verbatim arg array (`merge --no-ff`, `commit --amend`), so it
    // cannot inject `--no-verify` per call the way `commit_with_env`
    // does; pointing `core.hooksPath` at an empty dir inside the
    // temp repo neutralises any hook a contributor's global
    // `core.hooksPath` would otherwise fire (#941). The per-call
    // `--no-verify` flags then become belt-and-suspenders.
    let no_hooks = dir.join(".bca-empty-hooks");
    std::fs::create_dir_all(&no_hooks).expect("mkdir empty hooks dir");
    let no_hooks = no_hooks.to_str().expect("hooks path is valid UTF-8");
    run_git(dir, &["config", "core.hooksPath", no_hooks], &[]);
    run_git(dir, &["config", "gc.auto", "0"], &[]);
    // Pin line endings so churn line-counts are identical on a
    // contributor with `core.autocrlf=true` (the Git-for-Windows
    // default) — the integration tests assert exact churn values.
    run_git(dir, &["config", "core.autocrlf", "false"], &[]);
}

/// Run `git <args>` in `dir` with extra environment, asserting success.
///
/// The host's global and system configuration are switched off for every
/// call, and after `env` so no caller can switch them back on. The
/// per-repository pins in [`configure`] cannot cover the commands that
/// create the repository: `git clone` fires `reference-transaction` and
/// `post-checkout` before there is a repository to pin, and `git init`
/// fires `reference-transaction` too on a git whose `init` writes `HEAD`
/// through a ref transaction (2.46 and later), so a contributor's global
/// `core.hooksPath` used to reach them. `core.excludesFile` and
/// `core.attributesFile` are pinned as well: their defaults
/// (`$XDG_CONFIG_HOME/git/ignore` and `…/attributes`) are not
/// configuration files, so the first two variables leave them live, and
/// an `ignore` of `*` makes `git add -A` stage nothing (#1771). Git for
/// Windows maps `/dev/null` to `NUL`.
fn run_git(dir: &Path, args: &[&str], env: &[(&str, &str)]) {
    let status = command(dir, args, env).status().expect("spawn git");
    assert!(status.success(), "git {args:?} failed");
}

/// Assert that `git config --get user.name`, run in `dir` under `env`
/// through the same isolation every fixture `git` gets, finds no value.
///
/// The one probe that sees host configuration without anything failing:
/// a hostile host writes a `[user]` sentinel, and a fixture whose
/// isolation dropped `GIT_CONFIG_GLOBAL` or `GIT_CONFIG_NOSYSTEM` would
/// read it back. The status is pinned to exactly 1 — git's "key not
/// set" — because a config file git cannot parse exits 128, and a bare
/// "non-zero" would let a malformed hostile config read as isolated.
pub fn assert_no_user_name(dir: &Path, env: &[(&str, &str)]) {
    let out = command(dir, &["config", "--get", "user.name"], env)
        .output()
        .expect("spawn git config");
    assert_eq!(
        out.status.code(),
        Some(1),
        "git config --get user.name: expected exit 1 (unset); stdout {:?}, stderr {:?}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stdout.is_empty(),
        "the fixture's git read the host's user.name: {:?}",
        String::from_utf8_lossy(&out.stdout)
    );
}

fn command(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Command {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(dir);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_COUNT", "2")
        .env("GIT_CONFIG_KEY_0", "core.excludesFile")
        .env("GIT_CONFIG_VALUE_0", "/dev/null")
        .env("GIT_CONFIG_KEY_1", "core.attributesFile")
        .env("GIT_CONFIG_VALUE_1", "/dev/null");
    cmd
}
