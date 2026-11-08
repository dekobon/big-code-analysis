//! `git` for throwaway fixture repositories, isolated from the host.
//!
//! Every CLI suite that needs a repository builds it through the `git`
//! CLI. Each used to spawn `git` on its own, and none of them kept the
//! host's global or system configuration away from it, so a
//! contributor's `~/.gitconfig` reached the fixture: a `core.hooksPath`
//! whose hook fails, a `core.excludesFile` that ignores the fixture's
//! sources, an `init.defaultBranch`. The per-repository pins a fixture
//! writes after `git init` (`commit.gpgsign`, `core.hooksPath`) cannot
//! cover `init` itself, and most fixtures pin nothing but `gpgsign`
//! (#1771). This mirrors `tests/common/vcs_fixture.rs::run_git` in the
//! library crate; `[dev-dependencies]` and shared test modules do not
//! cross workspace members, so the twelve lines are repeated here
//! rather than published.
//!
//! Each call first drops every `GIT_*` variable the test process
//! inherited: git exports `GIT_DIR` and `GIT_INDEX_FILE` to its hooks,
//! and `GIT_DIR` to `git rebase --exec` in a linked worktree, and
//! inherited they outrank `current_dir`, so a fixture's `init`, `add`
//! and `commit` wrote into the checkout running the test (#1846). Then
//! it applies the caller's environment, and then, *after* it so no
//! caller can switch them back on:
//!
//! - `GIT_CONFIG_GLOBAL=/dev/null` — no `~/.gitconfig`, and no
//!   `$XDG_CONFIG_HOME/git/config` either, since git reads the XDG file
//!   only when that variable is unset;
//! - `GIT_CONFIG_NOSYSTEM=1` — no system file;
//! - `GIT_CONFIG_COUNT=2` pinning `core.excludesFile` and
//!   `core.attributesFile` to `/dev/null` — the two per-user files that
//!   are *not* configuration. Their defaults are
//!   `$XDG_CONFIG_HOME/git/ignore` and `…/git/attributes`
//!   (`~/.config/git/…` when the variable is unset), and git reads them
//!   whatever the first two variables say. An `ignore` of `*` leaves
//!   `git add .` staging nothing, so the first commit fails.
//!
//! Git for Windows maps `/dev/null` to `NUL`.

use std::path::Path;
use std::process::{Command, Stdio};

/// Reference "now" for fixtures that pin commit dates. Commit
/// timestamps are expressed as offsets before this instant.
pub const FIXED_NOW: i64 = 1_700_000_000;

/// One day in seconds, for expressing commit ages.
pub const DAY: i64 = 86_400;

/// The author and committer every fixture commit carries.
pub const IDENTITY: [(&str, &str); 4] = [
    ("GIT_AUTHOR_NAME", "Ada"),
    ("GIT_AUTHOR_EMAIL", "ada@example.com"),
    ("GIT_COMMITTER_NAME", "Ada"),
    ("GIT_COMMITTER_EMAIL", "ada@example.com"),
];

/// Run `git <args>` in `dir` with extra environment, asserting success.
pub fn run_git(dir: &Path, args: &[&str], env: &[(&str, &str)]) {
    let status = command(dir, args, env).status().expect("spawn git");
    assert!(status.success(), "git {args:?} failed");
}

/// Run `git <args>` in `dir` with extra environment, asserting success
/// and returning its trimmed standard output.
///
/// For reading a fixture back — `git log`, `git rev-parse` — through
/// the same isolation that built it. A read-back spawned bare inherits
/// the host's configuration: a `log.decorate = full` turns `%D` into
/// `HEAD -> refs/heads/main`.
pub fn git_output(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> String {
    let out = command(dir, args, env).output().expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// Run `git <args>` in `dir` as [`IDENTITY`] at `secs` (UNIX seconds),
/// asserting success. Both author and committer dates are pinned, so a
/// commit-producing command lands at exactly `secs`.
pub fn git_at(dir: &Path, secs: i64, args: &[&str]) {
    git_at_with_env(dir, secs, args, &[]);
}

/// [`git_at`] with extra environment applied before the identity and
/// dates, so a fixture can take a caller environment and still commit
/// as [`IDENTITY`] at `secs`.
pub fn git_at_with_env(dir: &Path, secs: i64, args: &[&str], env: &[(&str, &str)]) {
    let date = format!("@{secs} +0000");
    let dated = [
        ("GIT_AUTHOR_DATE", date.as_str()),
        ("GIT_COMMITTER_DATE", date.as_str()),
    ];
    let env: Vec<(&str, &str)> = env.iter().chain(&IDENTITY).chain(&dated).copied().collect();
    run_git(dir, args, &env);
}

/// Run `git <args>` in `dir` with extra environment and both stdio
/// streams discarded, reporting whether it succeeded.
///
/// For the one call that may legitimately fail: a `git init` on a host
/// without git, which the caller treats as a skip. Everything after it
/// operates on a repository the test owns, so use [`run_git`] there.
pub fn try_git(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> bool {
    command(dir, args, env)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Assert that `git config --get user.name`, run in `dir` under `env`
/// through the same isolation every fixture `git` gets, finds no value.
///
/// The one probe that sees host configuration without anything
/// failing: [`HostileGitHost`] writes a `[user]` sentinel, and a fixture
/// whose isolation dropped `GIT_CONFIG_GLOBAL` or `GIT_CONFIG_NOSYSTEM`
/// would read it back. The status is pinned to exactly 1 — git's "key
/// not set" — because a config file git cannot parse exits 128, and a
/// bare "non-zero" would let a malformed hostile config read as
/// isolated.
pub fn assert_no_user_name(dir: &Path, env: &[(&str, &str)]) {
    let out = command(dir, &["config", "--get", "user.name"], env)
        .output()
        .expect("spawn git config");
    assert_eq!(
        out.status.code(),
        Some(1),
        "git config --get user.name: expected exit 1 (unset), got {:?}; stdout {:?}, stderr {:?}",
        out.status.code(),
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
    for (key, _) in std::env::vars_os() {
        if is_git_var(&key) {
            cmd.env_remove(key);
        }
    }
    cmd.envs(env.iter().copied());
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_COUNT", "2")
        .env("GIT_CONFIG_KEY_0", "core.excludesFile")
        .env("GIT_CONFIG_VALUE_0", "/dev/null")
        .env("GIT_CONFIG_KEY_1", "core.attributesFile")
        .env("GIT_CONFIG_VALUE_1", "/dev/null");
    cmd
}

/// Whether `key` names a `GIT_*` variable. Byte-wise on the encoded
/// name, so a non-UTF-8 name is still read; ASCII-case-insensitive on
/// Windows, where environment names are, so `git_dir` reaches the same
/// variable `GIT_DIR` does.
pub fn is_git_var(key: &std::ffi::OsStr) -> bool {
    let bytes = key.as_encoded_bytes();
    let Some(prefix) = bytes.get(..4) else {
        return false;
    };
    if cfg!(windows) {
        prefix.eq_ignore_ascii_case(b"GIT_")
    } else {
        prefix == b"GIT_"
    }
}

/// A host whose git configuration breaks every fixture that reads it.
///
/// The global file (`HOME/.gitconfig`, reached only through the
/// `GIT_CONFIG_GLOBAL` in [`env`](Self::env) — git ignores both
/// `~/.gitconfig` and `XDG_CONFIG_HOME/git/config` once that variable
/// is set) and the system file (`GIT_CONFIG_SYSTEM`) both name a
/// `core.excludesFile` that ignores every path and, on unix, a
/// `core.hooksPath` whose every hook fails; `XDG_CONFIG_HOME/git/ignore`
/// — read without any configuration naming it, which is what `HOME` and
/// `XDG_CONFIG_HOME` are in the environment for — ignores every path the
/// same way. Each isolation variable has its own discriminator. The
/// `git/ignore` guards the `GIT_CONFIG_COUNT` pin: without it `git add`
/// stages nothing and the first commit fails. The hooks guard
/// `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_NOSYSTEM` on unix, where
/// `reference-transaction` reaches even `git init` on a git whose `init`
/// writes `HEAD` through a ref transaction (2.46 and later). The
/// `core.excludesFile` guards neither — the pin's command-scope value
/// overrides a global or system one — so everywhere, and on Windows
/// alone, the `[user]` sentinel does: [`assert_no_user_name`] reads it
/// back through `git config`, which is the one place host configuration
/// is visible without failing anything. The sentinel does not decide a
/// commit's author, since every fixture sets `GIT_AUTHOR_NAME`, and the
/// `[init] defaultBranch` is likewise overridden by the `-b main` every
/// fixture passes.
///
/// Paths in the configuration are written with forward slashes, which
/// git accepts on Windows too; a backslash inside a config value is an
/// escape, and a value git cannot parse exits `git config` with 128 —
/// which the sentinel's exact-status check would otherwise have to tell
/// apart from "isolated".
///
/// [`env`](Self::env) is the environment that points a `git` at it,
/// including `GIT_CONFIG_GLOBAL` itself, so passing it through a
/// helper's caller-environment parameter also pins that the isolation
/// is applied after the caller's variables.
///
/// Three copies of this host are kept byte-identical in the
/// configuration they write, since shared test modules do not cross
/// crates: `HostileGitHost` in `big-code-analysis-cli/src/vcs_command.rs`
/// and `big-code-analysis-web/src/web/server_tests.rs`, and
/// `_hostile_git_host` in `big-code-analysis-py/tests/test_vcs.py`.
pub struct HostileGitHost {
    dir: tempfile::TempDir,
    global: String,
    system: String,
}

impl Default for HostileGitHost {
    fn default() -> Self {
        Self::new()
    }
}

impl HostileGitHost {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let hooks = dir.path().join("hooks");
        std::fs::create_dir(&hooks).expect("mkdir hooks");
        let hooks_pin = install_hooks(&hooks);
        let excludes = dir.path().join("excludes");
        std::fs::write(&excludes, "*\n").expect("write excludes");
        let xdg_git = dir.path().join("git");
        std::fs::create_dir(&xdg_git).expect("mkdir xdg git");
        std::fs::write(xdg_git.join("ignore"), "*\n").expect("write xdg ignore");
        let hostile = format!(
            "[core]\n{hooks_pin}\texcludesFile = {}\n\
             [init]\n\tdefaultBranch = hostile\n\
             [user]\n\tname = HOSTILE\n\temail = hostile@example.invalid\n",
            config_path(&excludes)
        );
        let global = dir.path().join(".gitconfig");
        std::fs::write(&global, &hostile).expect("write global config");
        let system = dir.path().join("system.cfg");
        std::fs::write(&system, &hostile).expect("write system config");
        let utf8 = |path: &Path| {
            path.to_str()
                .expect("tempdir path is valid UTF-8")
                .to_owned()
        };
        let (global, system) = (utf8(&global), utf8(&system));
        Self {
            dir,
            global,
            system,
        }
    }

    /// Environment under which a `git` that reads host configuration
    /// reads this host's.
    pub fn env(&self) -> [(&str, &str); 4] {
        let home = self
            .dir
            .path()
            .to_str()
            .expect("tempdir path is valid UTF-8");
        [
            ("HOME", home),
            ("XDG_CONFIG_HOME", home),
            ("GIT_CONFIG_GLOBAL", &self.global),
            ("GIT_CONFIG_SYSTEM", &self.system),
        ]
    }
}

/// `path` as a git config value: forward slashes only, since a
/// backslash in a value is an escape and git rejects an unknown one.
fn config_path(path: &Path) -> String {
    path.to_str()
        .expect("tempdir path is valid UTF-8")
        .replace('\\', "/")
}

/// Write the failing hooks into `hooks` and return the `core.hooksPath`
/// line that names them. The hooks are POSIX shell scripts, so on other
/// platforms the line is empty and the `git/ignore` and `[user]`
/// sentinel alone discriminate.
#[cfg(unix)]
fn install_hooks(hooks: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;

    for hook in ["pre-commit", "post-commit", "reference-transaction"] {
        let path = hooks.join(hook);
        std::fs::write(&path, "#!/bin/sh\nexit 1\n").expect("write hook");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod hook");
    }
    format!("\thooksPath = {}\n", config_path(hooks))
}

#[cfg(not(unix))]
fn install_hooks(_hooks: &Path) -> String {
    String::new()
}
