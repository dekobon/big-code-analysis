// Sibling-file unit tests for the diff-aware resolver.
//
// Wired via `#[path = "diff_tests.rs"] mod tests;` so the production
// `diff.rs` stays under the per-file metric caps. Matched by the
// `./**/*_tests.rs` rule in `.bcaignore`, so the self-scan walker
// skips this file.

use super::*;
use std::sync::{Mutex, MutexGuard};

/// Serializes every test that mutates the process's environment.
/// `cargo test` runs each test on its own thread by default and the
/// `BCA_DIFF_BASE` / `GITHUB_BASE_REF` / `GITHUB_EVENT_BEFORE` vars
/// are process-global — without this lock, two parallel tests would
/// stomp each other's setup. Each affected test acquires this mutex
/// for the duration of its body.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Acquire the global env mutex AND the per-test save/restore guard
/// as one returned value. The lock MUST outlive the guard: when
/// `EnvGuard::drop` calls `unsafe { env::set_var }` to restore the
/// original env, a peer test re-acquiring the lock concurrently
/// would race against the restore. Rust drops tuple fields in
/// declaration order, so `(EnvGuard, MutexGuard)` drops the guard
/// FIRST — performing the restore while the lock is still held —
/// then releases the lock. (`(MutexGuard, EnvGuard)` would release
/// the lock first and is wrong.) Recovers from a poisoned lock so a
/// panicking peer test does not cascade-fail every subsequent run.
fn lock_env(names: &[&str]) -> (EnvGuard, MutexGuard<'static, ()>) {
    let lock = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let guard = EnvGuard::capture(names);
    (guard, lock)
}

/// `DiffSource::label` is part of the footer banner string; pin every
/// variant so renaming a label fails the test rather than silently
/// shifting the user-visible footer text.
#[test]
fn diff_source_labels_match_env_var_names() {
    assert_eq!(DiffSource::Explicit.label(), "--since");
    assert_eq!(DiffSource::EnvOverride.label(), "BCA_DIFF_BASE");
    assert_eq!(DiffSource::GithubPr.label(), "GITHUB_BASE_REF");
    assert_eq!(DiffSource::GithubPush.label(), "GITHUB_EVENT_BEFORE");
}

/// Canonicalization of an extant file is byte-identical for repeated
/// calls; this is the load-bearing property `collect_changed` and
/// `canonicalize_for_match` rely on for `HashSet` lookups.
#[test]
fn canonicalize_for_match_is_idempotent_on_extant_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("a.txt");
    std::fs::write(&file, "x").expect("write");
    let a = canonicalize_for_match(&file);
    let b = canonicalize_for_match(&a);
    assert_eq!(a, b);
}

/// A missing path falls back to the input `PathBuf` so callers that
/// canonicalize both sides still get a deterministic answer (both
/// sides return the same fallback for the same input).
#[test]
fn canonicalize_for_match_falls_back_for_missing_paths() {
    let dir = tempfile::tempdir().expect("tempdir");
    let missing = dir.path().join("never-existed.rs");
    let out = canonicalize_for_match(&missing);
    assert_eq!(out, missing);
}

/// `auto_detect_base` consults env vars in a fixed precedence order;
/// running the four single-variable scenarios in parallel would race
/// on the process-global env, so this single test asserts the full
/// precedence ladder serially. Each scenario clears all four vars
/// before mutating one, so the prior scenario cannot leak through.
#[test]
fn auto_detect_base_precedence_ladder() {
    // `env::set_var` is unsafe in Rust 2024 (concurrent-use footgun);
    // `lock_env` serializes us against peer env-mutation tests and
    // installs an `EnvGuard` that restores the original values on
    // drop, so unrelated tests in the same binary are unaffected.
    let _env = lock_env(&[
        BCA_DIFF_BASE_ENV,
        GITHUB_BASE_REF_ENV,
        GITHUB_EVENT_BEFORE_ENV,
    ]);

    // Scenario 1: no env vars set → no auto-detected base. The ladder
    // tests the negative case first because the rest mutate the same
    // env and run serially; a per-scenario `assert!` message names
    // the scenario so a failure pinpoints which step regressed
    // instead of aborting at the wrong assertion.
    clear_diff_envs();
    assert!(
        auto_detect_base().is_none(),
        "scenario 1 (no env): expected None, got Some"
    );

    // Scenario 2: BCA_DIFF_BASE wins over both GitHub Actions vars.
    clear_diff_envs();
    set_env(BCA_DIFF_BASE_ENV, "abc123");
    set_env(GITHUB_BASE_REF_ENV, "main");
    set_env(GITHUB_EVENT_BEFORE_ENV, "deadbeef");
    let (base, source) = auto_detect_base().expect("scenario 2 (override wins): expected Some");
    assert_eq!(base, "abc123", "scenario 2: base");
    assert_eq!(source, DiffSource::EnvOverride, "scenario 2: source");

    // Scenario 3: GITHUB_BASE_REF wins over GITHUB_EVENT_BEFORE (PR
    // events take precedence over push events).
    clear_diff_envs();
    set_env(GITHUB_BASE_REF_ENV, "main");
    set_env(GITHUB_EVENT_BEFORE_ENV, "deadbeef");
    let (base, source) = auto_detect_base().expect("scenario 3 (PR wins over push): expected Some");
    assert_eq!(base, "origin/main", "scenario 3: base");
    assert_eq!(source, DiffSource::GithubPr, "scenario 3: source");

    // Scenario 4: GITHUB_EVENT_BEFORE is the last fallback.
    clear_diff_envs();
    set_env(GITHUB_EVENT_BEFORE_ENV, "deadbeef");
    let (base, source) =
        auto_detect_base().expect("scenario 4 (push falls through): expected Some");
    assert_eq!(base, "deadbeef", "scenario 4: base");
    assert_eq!(source, DiffSource::GithubPush, "scenario 4: source");

    // Scenario 5: all-zeroes SHA suppresses GithubPush detection (the
    // documented "no previous commit" sentinel for force-pushed or
    // brand-new branches).
    clear_diff_envs();
    set_env(GITHUB_EVENT_BEFORE_ENV, NULL_SHA);
    assert!(
        auto_detect_base().is_none(),
        "scenario 5 (NULL_SHA): all-zeroes SHA should not auto-detect"
    );

    // Scenario 6: empty-string GITHUB_BASE_REF is the documented
    // non-PR state and must be treated as absent, not as an empty
    // base ref (which would diff against `origin/`).
    clear_diff_envs();
    set_env(GITHUB_BASE_REF_ENV, "");
    assert!(
        auto_detect_base().is_none(),
        "scenario 6 (empty GITHUB_BASE_REF): empty string should be absent"
    );
}

/// `resolve_scope` honours `--since` over every env signal. We don't
/// run git here; the test only verifies that an explicit `since`
/// produces an `Explicit` source — failure to collect a diff (no git
/// checkout at the test temp dir) is the expected outcome and yields
/// `ResolveOutcome::Failed`.
#[test]
fn resolve_scope_explicit_since_uses_explicit_source() {
    let _env = lock_env(&[
        BCA_DIFF_BASE_ENV,
        GITHUB_BASE_REF_ENV,
        GITHUB_EVENT_BEFORE_ENV,
    ]);
    clear_diff_envs();
    set_env(BCA_DIFF_BASE_ENV, "would-be-override");
    // The test process's CWD is the repository root in
    // `cargo test`, so collect_changed will attempt a real git diff.
    // The point of the assertion is that the resolver does not switch
    // to `EnvOverride` when `--since` is set.
    let outcome = resolve_scope(Some("HEAD"));
    match outcome {
        ResolveOutcome::Ok(scope) => assert_eq!(scope.source, DiffSource::Explicit),
        ResolveOutcome::Failed { source, .. } => assert_eq!(source, DiffSource::Explicit),
        ResolveOutcome::Disabled => panic!("--since should never be Disabled"),
    }
}

/// `--since` absent + every env var absent → `Disabled`, which is
/// the fall-back-to-today's-behaviour signal `run_check` relies on.
#[test]
fn resolve_scope_disabled_when_no_signal() {
    let _env = lock_env(&[
        BCA_DIFF_BASE_ENV,
        GITHUB_BASE_REF_ENV,
        GITHUB_EVENT_BEFORE_ENV,
    ]);
    clear_diff_envs();
    assert!(matches!(resolve_scope(None), ResolveOutcome::Disabled));
}

/// `non_empty_env` distinguishes set-but-empty from unset; the former
/// is GitHub Actions' state for `GITHUB_BASE_REF` on non-PR runs and
/// must be treated as absent so the caller falls through to the next
/// signal in the ladder.
#[test]
fn non_empty_env_treats_empty_as_absent() {
    let _env = lock_env(&[BCA_DIFF_BASE_ENV]);
    clear_one(BCA_DIFF_BASE_ENV);
    assert_eq!(non_empty_env(BCA_DIFF_BASE_ENV), None);
    set_env(BCA_DIFF_BASE_ENV, "");
    assert_eq!(non_empty_env(BCA_DIFF_BASE_ENV), None);
    set_env(BCA_DIFF_BASE_ENV, "abc");
    assert_eq!(non_empty_env(BCA_DIFF_BASE_ENV), Some("abc".to_string()));
}

// --- env-mutation helpers (test-only) ---

/// Saves a snapshot of named env vars and restores them on drop so
/// per-test mutations don't bleed into peer tests in the same binary.
/// Rust 2024 marks `env::set_var` / `env::remove_var` as `unsafe`
/// (data-race footgun if another thread reads concurrently); cargo
/// runs unit tests multi-threaded *across* tests but each test body
/// is single-threaded, and the precedence test above is the only one
/// that mutates these specific names — so the guard is sufficient.
struct EnvGuard {
    saved: Vec<(String, Option<String>)>,
}

impl EnvGuard {
    fn capture(names: &[&str]) -> Self {
        let saved = names
            .iter()
            .map(|n| ((*n).to_string(), env::var(n).ok()))
            .collect();
        Self { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (name, val) in &self.saved {
            // SAFETY: tests are single-threaded inside one test body;
            // see EnvGuard docstring.
            unsafe {
                match val {
                    Some(v) => env::set_var(name, v),
                    None => env::remove_var(name),
                }
            }
        }
    }
}

fn set_env(name: &str, value: &str) {
    // SAFETY: see EnvGuard.
    unsafe {
        env::set_var(name, value);
    }
}

fn clear_one(name: &str) {
    // SAFETY: see EnvGuard.
    unsafe {
        env::remove_var(name);
    }
}

fn clear_diff_envs() {
    clear_one(BCA_DIFF_BASE_ENV);
    clear_one(GITHUB_BASE_REF_ENV);
    clear_one(GITHUB_EVENT_BEFORE_ENV);
}
