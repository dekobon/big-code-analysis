#![allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]

//! Format-validity helpers for the CLI integration suite.
//!
//! Submodule `validators` carries the same three helpers as
//! `tests/common/validators.rs` in the lib crate (validate_sarif,
//! assert_checkstyle_well_formed_and_structural, assert_html_well_formed).
//! Cargo `[dev-dependencies]` and shared modules do not propagate
//! across workspace members, so the duplication is unavoidable
//! without a separate test-helpers crate. Three small helpers don't
//! merit that indirection today.

use std::path::Path;

use assert_cmd::Command;

#[allow(dead_code)]
pub mod fixtures;

#[allow(dead_code)]
pub mod validators;

/// Workspace-relative root of the integration corpora. Every entry
/// under it is a git submodule, so all of it is absent from a fresh
/// clone or a fresh `git worktree` until it is checked out.
const CORPUS_ROOT: &str = "tests/repositories";

/// The corpus holding [`FIXTURE_FILE`].
const FIXTURE_CORPUS: &str = "DeepSpeech";

/// A small real-source file, relative to [`FIXTURE_CORPUS`]. Nineteen
/// tests in this crate analyse it, which is what makes its absence
/// worth a named diagnostic.
const FIXTURE_FILE: &str = "stats.py";

/// Absolute path to the workspace root, derived from this crate's
/// manifest directory rather than the process cwd (which the tests
/// move around).
fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("manifest dir has parent")
        .to_path_buf()
}

/// Absolute path to the shared real-source fixture, panicking with a
/// message that names its own cause when the corpus is not checked out.
///
/// Without the submodule these nineteen tests failed with `bca`'s
/// generic `error: path does not exist: …`, which reads as a bug in
/// whatever the author was changing rather than as missing setup — the
/// papercut #1171 is about.
#[allow(dead_code)]
pub fn corpus_fixture_path() -> String {
    let root = workspace_root();
    if let Some(hint) = corpus_checkout_hint(&root, FIXTURE_CORPUS, FIXTURE_FILE) {
        panic!("{hint}");
    }
    root.join(CORPUS_ROOT)
        .join(FIXTURE_CORPUS)
        .join(FIXTURE_FILE)
        .into_os_string()
        .into_string()
        .expect("fixture path is utf-8")
}

/// `Some(diagnostic)` when `file` is missing from the `corpus`
/// submodule under `workspace_root`; `None` when it is present.
///
/// Split from [`corpus_fixture_path`] so the diagnostic is testable
/// against a synthetic tree. The real corpus is checked out in any tree
/// where this suite runs, so a test that waited for its absence would
/// never execute — see `.claude/rules/testing.md`.
#[allow(dead_code)]
pub fn corpus_checkout_hint(workspace_root: &Path, corpus: &str, file: &str) -> Option<String> {
    let corpus_dir = workspace_root.join(CORPUS_ROOT).join(corpus);
    if corpus_dir.join(file).exists() {
        return None;
    }
    // A submodule git has started checking out always has its `.git`
    // file, so ignore that entry when deciding whether any content
    // landed. Everything else distinguishes "never initialized" from
    // "initialized and then interrupted", and only the second needs the
    // `--force`.
    let has_content = std::fs::read_dir(&corpus_dir).is_ok_and(|mut entries| {
        entries.any(|entry| entry.is_ok_and(|entry| entry.file_name() != ".git"))
    });
    let state = if has_content {
        "partially checked out"
    } else {
        "not checked out"
    };
    Some(format!(
        "integration corpus {state}: {} is missing. Run `make worktree-setup` \
         from the repository root. By hand it is `git submodule update --init \
         --force -- {CORPUS_ROOT}/{corpus}`, and the `--force` is \
         load-bearing: after an interrupted checkout the submodule HEAD \
         already matches the recorded SHA, so a plain re-run is a silent \
         no-op.",
        corpus_dir.join(file).display(),
    ))
}

/// Scrub CI-side env vars that `bca check` auto-detects from a
/// freshly-built `Command`. On a GitHub Actions runner the parent
/// process exports `GITHUB_STEP_SUMMARY` pointing to the runner's
/// real step-summary file, and `GITHUB_ACTIONS=true` enables
/// `::error` annotations. `assert_cmd::Command` inherits the
/// parent environment by default, so without this scrub every
/// test-driven `bca check` invocation appends a TempDir-fixture
/// digest to the runner's UI panel — and because the digest is
/// bounded by fixed sentinels, the last test wins, replacing every
/// earlier block (see #388).
///
/// The diff-scope auto-detection vars (`diff.rs::auto_detect_base`)
/// are scrubbed for the same reason: on a `pull_request` event GitHub
/// sets `GITHUB_BASE_REF` (and a push sets `GITHUB_EVENT_BEFORE`), so a
/// hermetic-tempdir `bca check` would auto-enable a `--since` scope,
/// fail to resolve `origin/<base>` (the tempdir is not a git checkout),
/// and emit a "proceeding without diff scope" warning to stderr —
/// breaking every test that asserts clean stderr. This only surfaces on
/// `pull_request` CI events (push runs leave `GITHUB_BASE_REF` unset),
/// which is why it stayed latent until the first PR. A test that
/// *wants* a diff scope sets the var explicitly after construction.
///
/// The artifact-link vars (`GITHUB_REPOSITORY`, `GITHUB_RUN_ID`) are
/// scrubbed for the same local/CI-divergence reason: a failing
/// `bca check` always emits a remediation block whose "Detailed reports"
/// line reads both vars (`commands.rs::artifact_link`) and, when both are
/// present (every GitHub Actions runner), points at the uploaded
/// `bca-reports` artifact URL — otherwise it falls back to
/// `run bca report to see them locally`. Leaving them inherited makes
/// remediation-block output environment-dependent, so a test asserting
/// the local-fallback wording would pass locally and fail on CI (#900).
/// A test that *wants* the CI artifact link sets both explicitly after
/// construction.
///
/// CLI-crate call sites name their builder `cli()`; each routes
/// through [`bca_command`] / [`cli_in`], which delegate here so a
/// future new env-leak only needs to be patched once. The
/// `big-code-analysis-web` smoke tests have their own `bin()` builder
/// that does *not* delegate here — Cargo does not share test modules
/// across workspace members, and `bca-web` reads no `GITHUB_*` vars
/// (only `BCA_MAX_ORPHANED_TASKS`), so it has nothing to scrub.
#[allow(dead_code)]
pub fn scrub_ci_env(cmd: &mut Command) -> &mut Command {
    for var in SCRUBBED_CI_ENV {
        cmd.env_remove(var);
    }
    cmd
}

/// The variables [`scrub_ci_env`] removes. Named once so the plain
/// `std::process::Command` builder below cannot drift from it.
const SCRUBBED_CI_ENV: [&str; 7] = [
    "GITHUB_STEP_SUMMARY",
    "GITHUB_ACTIONS",
    "GITHUB_BASE_REF",
    "BCA_DIFF_BASE",
    "GITHUB_EVENT_BEFORE",
    "GITHUB_REPOSITORY",
    "GITHUB_RUN_ID",
];

/// The same hermetic, env-scrubbed `bca` as [`cli_in`], but as a plain
/// [`std::process::Command`] so the caller can redirect the child's
/// stdout somewhere of its choosing.
///
/// `assert_cmd::Command::assert` runs the child through
/// `std::process::Command::output`, which unconditionally replaces
/// stdout and stderr with pipes — any redirection configured beforehand
/// is silently discarded. Tests that must point stdout at a real file
/// descriptor (`/dev/full`, a pipe they close early) therefore cannot go
/// through `assert_cmd` at all.
#[allow(dead_code)]
pub fn std_bca_command_in(dir: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new(assert_cmd::cargo::cargo_bin("bca"));
    cmd.current_dir(dir);
    for var in SCRUBBED_CI_ENV {
        cmd.env_remove(var);
    }
    cmd
}

/// Build a `bca` `Command` with CI-side env vars scrubbed. The
/// per-test `cli()` helpers delegate here so the env-isolation
/// policy lives in one place.
#[allow(dead_code)]
pub fn bca_command() -> Command {
    let mut cmd = Command::cargo_bin("bca").expect("bca binary builds");
    scrub_ci_env(&mut cmd);
    cmd
}

/// Build a `bca` `Command` whose working directory is `dir`, scrubbing
/// CI-side env vars first.
///
/// `bca check` discovers its `bca.toml` (and the `baseline` it names)
/// by climbing parents until it finds the directory containing `.git`.
/// The integration suite runs from inside this repo, so a `Command`
/// left at the inherited cwd auto-discovers the repo's own
/// `bca.toml` + `.bca-baseline.toml` and silently filters/scales each
/// fixture run against repo state. Anchoring the cwd at a
/// `tempfile::tempdir()` — which has no `.git` ancestor — makes
/// discovery find nothing, so the run is hermetic. This is the default
/// builder for every test that does not *itself* exercise manifest
/// auto-discovery (see #491).
#[allow(dead_code)]
pub fn cli_in(dir: &Path) -> Command {
    let mut cmd = bca_command();
    cmd.current_dir(dir);
    cmd
}

/// Build a hermetic `bca` `Command` rooted at a fresh, empty
/// `tempfile::tempdir()`, returning the guard alongside it.
///
/// Use this for tests that have no fixture tempdir of their own (e.g.
/// they analyse a repo-relative fixture via an absolute `--paths`) but
/// still must not inherit the repo's discovered `bca.toml` / baseline.
/// The returned [`tempfile::TempDir`] must be kept alive until the
/// command has been spawned — drop it too early and the cwd vanishes
/// before `bca` reads it. See [`cli_in`] for the discovery rationale
/// (#491).
#[allow(dead_code)]
pub fn cli_hermetic() -> (tempfile::TempDir, Command) {
    let dir = tempfile::tempdir().expect("create tempdir for hermetic cwd");
    let cmd = cli_in(dir.path());
    (dir, cmd)
}

/// Serialize every test that mutates process-global environment state and
/// then spawns a child `bca`. Spawning snapshots the *whole* parent
/// environment, so a concurrent `set_var` from an unrelated env-mutating
/// test in the same binary can tear that snapshot and leak its variable
/// into the wrong child — the cross-test race that let
/// `ArtifactEnvGuard`'s `GITHUB_REPOSITORY`/`GITHUB_RUN_ID` surface in
/// `cli_helper_does_not_leak_to_github_step_summary`'s child even though
/// the two tests touch different variables. Holding one shared lock across
/// the mutate-and-spawn critical section makes those sections mutually
/// exclusive regardless of which variable each one touches.
///
/// `set_var` / `remove_var` are `unsafe` in Rust 2024 for exactly this
/// concurrency hazard; this lock is the binary-wide serialization point
/// that makes the test-only mutations sound.
#[allow(dead_code)]
pub fn process_env_lock() -> std::sync::MutexGuard<'static, ()> {
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// RAII guard that sets the process working directory and restores the
/// prior one on drop. `set_current_dir` mutates process-global state, so
/// a static mutex serializes this guard against itself.
///
/// Used by hermeticity regression tests that must reproduce the
/// non-hermetic leak: a pre-#491 helper inherits the process cwd, so
/// these drive the cwd into a `.git`-rooted manifest dir and prove the
/// hermetic builders ignore it (the assertion then fails if a builder is
/// reverted to the un-anchored `bca_command()` — see
/// `.claude/rules/testing.md`).
#[allow(dead_code)]
pub struct CwdGuard {
    prior: std::path::PathBuf,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl CwdGuard {
    #[allow(dead_code)]
    pub fn enter(dir: &Path) -> Self {
        static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let lock = CWD_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let prior = std::env::current_dir().expect("read current dir");
        std::env::set_current_dir(dir).expect("enter guard dir");
        Self { prior, _lock: lock }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.prior);
    }
}

/// Strip every permission bit from `path` so reading it fails with
/// `EACCES`, returning whether the denial actually took effect.
///
/// `false` means this process can read the file regardless (root
/// ignores mode bits), so the scenario the caller wants to stage does
/// not exist here and the test should skip rather than fail. The
/// capability is probed rather than inferred from the uid.
#[cfg(unix)]
#[allow(dead_code)]
pub fn deny_all_access(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).expect("chmod 000");
    std::fs::read(path).is_err()
}

/// Strip every permission bit from the *directory* `path` so listing it
/// fails with `EACCES`, returning whether the denial actually took
/// effect.
///
/// The directory counterpart to [`deny_all_access`], and not a
/// convenience wrapper: that function probes with `fs::read`, which
/// fails with `EISDIR` on **every** directory regardless of its mode, so
/// it reports the denial as effective even where it is not. A caller
/// that used it to guard a "skip when privileged" branch (root ignores
/// mode bits) would never take that branch and would fail instead. The
/// probe here is `read_dir` — the operation the walk actually performs.
#[cfg(unix)]
#[allow(dead_code)]
pub fn deny_dir_listing(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).expect("chmod 000");
    std::fs::read_dir(path).is_err()
}

/// Write `body` to `name` under `dir` and lock it with
/// [`deny_all_access`], returning the path — or `None` when the lock
/// could not be made to bite.
///
/// Five tests across the suite stage this same "one file the walk
/// cannot read" scenario; they share the helper so the capability probe
/// cannot drift out of one of them and turn a privileged run into a
/// spurious failure.
#[cfg(unix)]
#[allow(dead_code)]
pub fn unreadable_fixture(dir: &Path, name: &str, body: &str) -> Option<std::path::PathBuf> {
    let path = dir.join(name);
    std::fs::write(&path, body).expect("write fixture");
    deny_all_access(&path).then_some(path)
}

/// Create `name` under `dir` holding one source file, then strip every
/// permission bit so the directory cannot be *listed* — the #1131
/// scenario, where a whole subtree drops out of the walk before any file
/// is selected. Returns the directory path, or `None` when the denial
/// does not bite.
///
/// The lock goes through [`deny_dir_listing`], whose probe is `read_dir`
/// rather than the `read` that [`deny_all_access`] uses: reading a
/// directory fails with `EISDIR` regardless of its mode, so
/// `read`-probing would report the denial as effective for *every*
/// directory, readable ones included, and the caller would stage a
/// scenario that does not exist.
///
/// The caller must call [`restore_dir_access`] before the enclosing
/// `TempDir` drops, or its recursive delete fails on the locked
/// directory.
#[cfg(unix)]
#[allow(dead_code)]
pub fn unlistable_dir(
    dir: &Path,
    name: &str,
    file: &str,
    body: &str,
) -> Option<std::path::PathBuf> {
    let path = dir.join(name);
    std::fs::create_dir_all(&path).expect("create fixture dir");
    std::fs::write(path.join(file), body).expect("write fixture");
    deny_dir_listing(&path).then_some(path)
}

/// Give a mode-stripped fixture directory its bits back so `TempDir`'s
/// recursive delete can remove it. The counterpart to
/// [`unlistable_dir`], and to the mode-555 output directories the
/// write-failure tests stage.
#[cfg(unix)]
#[allow(dead_code)]
pub fn restore_dir_access(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod 755");
}
