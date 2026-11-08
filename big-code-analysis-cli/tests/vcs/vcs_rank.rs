//! End-to-end tests for `bca vcs` and `bca metrics --vcs` (issue #328).
//!
//! Each builds a throwaway git repo with fixed author identities and
//! commit timestamps dated relative to wall-clock now, so the per-window
//! counts are deterministic across runs.

#![allow(clippy::doc_markdown)]

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

use crate::common;
use crate::common::git_fixture::{DAY, git_at, git_at_with_env};

fn cli() -> Command {
    common::bca_command()
}

/// Wall-clock "now" in Unix seconds. Both `bca vcs` and `bca metrics
/// --vcs` default to wall-clock time, so fixture commits are dated
/// relative to this; the *relative* offsets (and hence the counts) stay
/// deterministic across runs.
fn now() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_secs(),
    )
    .expect("now fits i64")
}

/// A repo with `src/work.rs` committed twice: once 200 days ago (long
/// window only) and once 5 days ago (recent window).
fn repo_two_commits() -> TempDir {
    repo_two_commits_with_env(&[])
}

/// [`repo_two_commits`] with extra environment on every `git`, so a
/// test can point the fixture at a hostile host configuration and check
/// it is not read.
fn repo_two_commits_with_env(env: &[(&str, &str)]) -> TempDir {
    let now = now();
    let dir = tempfile::tempdir().expect("tempdir");
    let git = |secs: i64, args: &[&str]| git_at_with_env(dir.path(), secs, args, env);
    git(now, &["init", "-q", "-b", "main"]);
    git(now, &["config", "commit.gpgsign", "false"]);
    std::fs::create_dir(dir.path().join("src")).expect("mkdir");
    std::fs::write(dir.path().join("src/work.rs"), "fn a() {}\n").expect("write");
    git(now - 200 * DAY, &["add", "."]);
    git(now - 200 * DAY, &["commit", "-q", "-m", "add work"]);
    std::fs::write(dir.path().join("src/work.rs"), "fn a() {}\nfn b() {}\n").expect("write");
    git(now - 5 * DAY, &["commit", "-aqm", "fix bug in work"]);
    dir
}

/// The shared fixture `git` must not read the host's configuration,
/// and must not let a caller's environment switch it back on (#1771).
///
/// Every `git` here is handed the hostile host through the helper's
/// caller-environment parameter — `HOME`, `XDG_CONFIG_HOME` and a
/// `GIT_CONFIG_GLOBAL` / `GIT_CONFIG_SYSTEM` pair naming its files.
/// The helper's own assertions do the discriminating: read, the host's
/// `XDG_CONFIG_HOME/git/ignore` makes `git add .` stage nothing so the
/// first commit fails, and on unix its `core.hooksPath` aborts the
/// commit besides. The `user.name` sentinel is what guards
/// `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_NOSYSTEM` where there are no
/// hooks: the pinned `core.excludesFile` overrides the host's, so
/// `git config` is the one place host configuration would be visible
/// without failing anything. A commit's author comes from
/// `GIT_AUTHOR_NAME`, which every fixture sets, and `-b main` overrides
/// `init.defaultBranch`, so the closing `%an %D` assertion only pins
/// that the fixture produced its commits on `main`.
#[test]
fn fixture_git_ignores_the_hosts_global_and_system_config() {
    use crate::common::git_fixture::{
        HostileGitHost, IDENTITY, assert_no_user_name, git_output, run_git,
    };

    let host = HostileGitHost::new();
    let env = host.env();
    let repo = repo_two_commits_with_env(&env);
    assert_no_user_name(repo.path(), &env);
    std::fs::write(repo.path().join("src/more.rs"), "fn c() {}\n").expect("write");
    run_git(repo.path(), &["add", "."], &env);
    let commit_env: Vec<(&str, &str)> = env.iter().chain(&IDENTITY).copied().collect();
    run_git(
        repo.path(),
        &["commit", "-q", "-m", "add more"],
        &commit_env,
    );
    assert_eq!(
        git_output(repo.path(), &["log", "-1", "--format=%an %D"], &env),
        "Ada HEAD -> main"
    );
}

/// The fixture alone, read back through its own `git`: two commits on
/// `main`, the file tracked. The target the child-process tests below
/// re-run, so it spawns no `bca` — what a redirected `bca` would read is
/// the production walker's question, not the fixture's.
#[test]
fn fixture_git_builds_the_two_commit_repo() {
    use crate::common::git_fixture::git_output;

    let repo = repo_two_commits();
    assert_eq!(
        git_output(repo.path(), &["rev-list", "--count", "HEAD"], &[]),
        "2"
    );
    assert_eq!(git_output(repo.path(), &["ls-files"], &[]), "src/work.rs");
    assert_eq!(
        git_output(repo.path(), &["log", "-1", "--format=%an %D"], &[]),
        "Ada HEAD -> main"
    );
}

/// The `git` that builds and reads the victim repository: its own
/// removal loop over the inherited `GIT_*` variables, so a defect in
/// the fixture's loop cannot reach it (the two share only the
/// `is_git_var` prefix test), and the host's configuration kept away.
#[cfg(unix)]
fn clean_git(dir: &Path, args: &[&str]) -> String {
    let mut cmd = std::process::Command::new("git");
    for (key, _) in std::env::vars_os() {
        if crate::common::git_fixture::is_git_var(&key) {
            cmd.env_remove(key);
        }
    }
    let out = cmd
        .arg("-C")
        .arg(dir)
        .args(CLEAN_GIT_CONFIG)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// `-c` pins for [`clean_git`]: an identity for its commits, and the
/// per-user ignore and attributes files off, since those are read
/// whatever the global and system variables say and a host
/// `XDG_CONFIG_HOME/git/ignore` of `*.rs` would fail the victim's `git
/// add` before the child under test ever ran.
#[cfg(unix)]
const CLEAN_GIT_CONFIG: [&str; 8] = [
    "-c",
    "user.name=t",
    "-c",
    "user.email=t@x",
    "-c",
    "core.excludesFile=/dev/null",
    "-c",
    "core.attributesFile=/dev/null",
];

/// Re-runs [`fixture_git_builds_the_two_commit_repo`] in a child test
/// binary with `vars` exported, and asserts that it passed. A test
/// cannot set its own process's environment without `unsafe`, and the
/// fixture reads it through `Command`, so the variables are set on a
/// child instead. The name is module-qualified: libtest's `--exact`
/// matches the full path.
#[cfg(unix)]
fn rerun_fixture_test_with(vars: &[(&str, std::path::PathBuf)]) {
    let child = format!(
        "{}::fixture_git_builds_the_two_commit_repo",
        module_path!()
            .strip_prefix(concat!(env!("CARGO_CRATE_NAME"), "::"))
            .expect("the test module is nested under the crate root")
    );
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", &child, "--format", "pretty"])
        .envs(vars.iter().map(|(k, v)| (k, v)))
        .output()
        .expect("spawn the child");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("test result: ok. 1 passed"),
        "{child} must pass with {vars:?} inherited; stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A hook, and `git rebase --exec` in a linked worktree, export `GIT_DIR`
/// and `GIT_INDEX_FILE`, and both outrank `current_dir` (#1846). The
/// fixture test is re-run in a child with that pair naming another
/// repository: the child must pass, and that repository must come
/// through untouched. Verified by revert: without the scrub in
/// `git_fixture::command` the child's `init`, `add` and `commit` land in
/// the victim — its HEAD moves and its index lists `src/work.rs`.
#[cfg(unix)]
#[test]
fn fixture_git_ignores_an_inherited_repository() {
    let victim = tempfile::tempdir().expect("tempdir");
    std::fs::write(victim.path().join("keep.rs"), "fn keep() {}\n").expect("write");
    clean_git(victim.path(), &["init", "-q", "-b", "main"]);
    clean_git(victim.path(), &["add", "keep.rs"]);
    clean_git(victim.path(), &["commit", "-q", "-m", "victim"]);
    let state = || {
        (
            clean_git(victim.path(), &["rev-parse", "HEAD"]),
            clean_git(victim.path(), &["ls-files", "-s"]),
        )
    };
    let before = state();
    assert!(before.1.contains("keep.rs"), "the victim tracks keep.rs");
    let git_dir = victim.path().join(".git");
    rerun_fixture_test_with(&[
        ("GIT_DIR", git_dir.clone()),
        ("GIT_INDEX_FILE", git_dir.join("index")),
    ]);
    assert_eq!(state(), before, "the inherited repository was written to");
}

/// `is_git_var` reads the name's first four bytes, so a short name, a
/// name that merely starts `GIT` (`GITHUB_TOKEN`) and an empty one are
/// not git's; a lowercase spelling is on Windows alone, where
/// environment names are case-insensitive. Host-independent, so the
/// Windows branch runs there while the child reruns stay unix-only.
#[test]
fn is_git_var_reads_the_prefix_as_the_host_does() {
    use crate::common::git_fixture::is_git_var;
    use std::ffi::OsStr;
    assert!(is_git_var(OsStr::new("GIT_DIR")));
    assert!(is_git_var(OsStr::new("GIT_")));
    assert!(!is_git_var(OsStr::new("GITHUB_TOKEN")));
    assert!(!is_git_var(OsStr::new("GIT")));
    assert!(!is_git_var(OsStr::new("")));
    assert_eq!(is_git_var(OsStr::new("git_dir")), cfg!(windows));
}

/// `GIT_OBJECT_DIRECTORY`, which a `receive-pack` quarantine hook
/// exports, replaces the object store git reads; once the quarantine it
/// names is gone, discovery itself fails with `not a git repository`.
/// The fixture test is re-run in a child with it naming a directory
/// whose parent does not exist either: `git init` creates one missing
/// level of an object directory, so a path one level down would let the
/// unscrubbed fixture succeed. Kept apart from the repository case
/// above: a fixture that failed to start would leave that case's victim
/// untouched for the wrong reason. Verified by revert: without the
/// scrub the child's `git init` fails.
#[cfg(unix)]
#[test]
fn fixture_git_ignores_an_inherited_object_directory() {
    let gone = tempfile::tempdir().expect("tempdir");
    rerun_fixture_test_with(&[(
        "GIT_OBJECT_DIRECTORY",
        gone.path().join("gone").join("quarantine"),
    )]);
    // Pins the path, not only the child's success: a git that created
    // both levels, or a scrub that dropped the variable only after
    // `init`, would pass the rerun and leave the directory behind.
    assert!(
        !gone.path().join("gone").exists(),
        "the fixture's git used the inherited object directory"
    );
}

#[test]
fn vcs_json_ranks_the_tracked_file() {
    let repo = repo_two_commits();
    let assert = cli()
        .current_dir(repo.path())
        .args(["vcs", "--paths", ".", "--format", "json"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json");

    let files = doc["files"].as_array().expect("files array");
    let work = files
        .iter()
        .find(|f| f["path"] == "src/work.rs")
        .expect("src/work.rs ranked");
    // The per-file metrics nest under a `vcs` key (#684); the constant
    // stamps live once on the envelope, never per row (#635).
    let work_vcs = &work["vcs"];
    assert_eq!(work_vcs["commits_long"], 2);
    assert_eq!(
        work_vcs["commits_recent"], 1,
        "only the 5-day commit is recent"
    );
    assert_eq!(
        work_vcs["bug_fix_commits"], 1,
        "the second commit message says 'fix bug'"
    );
    assert_eq!(work_vcs["authors_long"], 1);
    assert_eq!(doc["long_window_days"], 365);
    for constant in [
        "vcs_schema_version",
        "risk_score_version",
        "long_window_days",
        "recent_window_days",
    ] {
        assert!(
            work.get(constant).is_none() && work_vcs.get(constant).is_none(),
            "constant `{constant}` must not be repeated per row (#635)"
        );
    }
}

/// The bus-factor `by_directory[].directory` keys are rendered by the
/// library and `files[].path` by the CLI, in one report. Each directory
/// must be a prefix of the file paths it groups, spelled the same way, or
/// a consumer joining the two tables pairs nothing. The expected set is
/// derived from `files[].path`, so the test pins agreement rather than
/// either spelling alone; the literal pins keep it from passing on two
/// empty sides.
#[test]
fn vcs_json_directory_keys_match_the_file_paths_they_group() {
    let now = now();
    let dir = tempfile::tempdir().expect("tempdir");
    git_at(dir.path(), now, &["init", "-q", "-b", "main"]);
    git_at(dir.path(), now, &["config", "commit.gpgsign", "false"]);
    std::fs::create_dir_all(dir.path().join("src/nested")).expect("mkdir");
    std::fs::write(dir.path().join("src/top.rs"), "fn a() {}\n").expect("write");
    std::fs::write(dir.path().join("src/nested/deep.rs"), "fn b() {}\n").expect("write");
    git_at(dir.path(), now - 5 * DAY, &["add", "."]);
    git_at(dir.path(), now - 5 * DAY, &["commit", "-q", "-m", "add"]);

    let assert = cli()
        .current_dir(dir.path())
        .args(["vcs", "--paths", ".", "--format", "json"])
        .assert()
        .success();
    let doc: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).expect("json");

    let paths: Vec<&str> = doc["files"]
        .as_array()
        .expect("files array")
        .iter()
        .map(|f| f["path"].as_str().expect("path is a string"))
        .collect();
    let directories: std::collections::BTreeSet<&str> =
        doc["vcs_aggregate"]["bus_factor"]["by_directory"]
            .as_array()
            .expect("by_directory array")
            .iter()
            .map(|d| d["directory"].as_str().expect("directory is a string"))
            .collect();
    // Bus factor groups by the top-level directory and the one beneath it.
    let from_paths: std::collections::BTreeSet<&str> = paths
        .iter()
        .flat_map(|path| {
            let parent = path.rsplit_once('/').map_or("", |(parent, _)| parent);
            let depth2 = parent
                .match_indices('/')
                .nth(1)
                .map_or(parent, |(i, _)| &parent[..i]);
            let depth1 = depth2.split_once('/').map_or(depth2, |(top, _)| top);
            [depth1, depth2]
        })
        .filter(|d| !d.is_empty())
        .collect();

    assert_eq!(
        directories,
        ["src", "src/nested"].into_iter().collect(),
        "the fixture's two directory groups"
    );
    assert_eq!(
        directories, from_paths,
        "every by_directory key must be spelled as the files[].path it groups ({paths:?})"
    );
}

/// #659: `bca vcs --format text` renders the same human ranked table the
/// bare `bca vcs` prints — the human format is now named and selectable
/// here, not only the unnamed default.
#[test]
fn vcs_format_text_renders_the_human_table() {
    let repo = repo_two_commits();
    let default_out = cli()
        .current_dir(repo.path())
        .args(["vcs", "--paths", "."])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text_out = cli()
        .current_dir(repo.path())
        .args(["vcs", "--paths", ".", "--format", "text"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        text_out, default_out,
        "`vcs --format text` must match the default human table"
    );
    assert!(
        String::from_utf8(text_out).unwrap().contains("RISK"),
        "human table carries the RISK column header"
    );
}

/// Count `*.json` cache entries anywhere under `root`.
fn count_cache_entries(root: &Path) -> usize {
    let mut count = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "json") {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn vcs_cache_dir_persists_and_replays_identically() {
    let repo = repo_two_commits();
    let cache = tempfile::tempdir().expect("cache dir");
    let cache_arg = cache.path().to_str().expect("utf8 cache path");

    // No `--as-of`: an `--as-of` predating HEAD legitimately re-anchors and
    // bypasses the HEAD-keyed cache (issue #648), so a cache-persistence
    // test must run the live (HEAD-anchored) walk. The two runs share one
    // wall-clock second, so their output is byte-identical regardless.
    let run = || {
        let assert = cli()
            .current_dir(repo.path())
            .args([
                "vcs",
                "--paths",
                ".",
                "--format",
                "json",
                "--cache-dir",
                cache_arg,
            ])
            .assert()
            .success();
        String::from_utf8(assert.get_output().stdout.clone()).expect("utf8")
    };

    let first = run(); // miss: walks and writes
    assert_eq!(
        count_cache_entries(cache.path()),
        1,
        "an entry is persisted"
    );
    let second = run(); // hit: replays
    assert_eq!(
        first, second,
        "a cache hit is byte-identical to the first run"
    );

    // Equality alone cannot tell a hit from a second cold walk — both
    // produce the first run's bytes. Emptying the persisted event log
    // separates them: a *served* entry now replays zero commits, while a
    // walk would reproduce `first`. This is the only guard in the suite on
    // the fingerprint being stable **across processes**, which every
    // persisted entry depends on and which an in-process test cannot see
    // (the mailmap digest #1262 folded in is hashed from gix-owned values,
    // so it is a new way to break that old property).
    empty_cache_entry_events(cache.path());
    let third = run();
    assert_ne!(
        first, third,
        "the emptied entry was not served, so the second process computed a \
         different fingerprint and cold-walked instead of hitting"
    );
}

/// Rewrite every persisted `*.json` cache entry under `root` with an empty
/// `events` array, leaving it valid JSON so it still loads as a hit
/// candidate. Mirrors the library suite's helper of the same shape.
fn empty_cache_entry_events(root: &Path) {
    let mut stack = vec![root.to_path_buf()];
    let mut emptied = 0_usize;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "json") {
                let bytes = std::fs::read(&path).expect("read entry");
                let mut value: serde_json::Value =
                    serde_json::from_slice(&bytes).expect("entry is valid JSON");
                value["events"] = serde_json::Value::Array(Vec::new());
                std::fs::write(&path, serde_json::to_vec(&value).expect("reserialize"))
                    .expect("rewrite entry");
                emptied += 1;
            }
        }
    }
    assert!(
        emptied > 0,
        "no cache entry was emptied, so the run that follows proves nothing"
    );
}

#[test]
fn vcs_no_cache_writes_nothing() {
    let repo = repo_two_commits();
    let cache = tempfile::tempdir().expect("cache dir");
    cli()
        .current_dir(repo.path())
        .args([
            "vcs",
            "--paths",
            ".",
            "--no-cache",
            "--cache-dir",
            cache.path().to_str().expect("utf8"),
        ])
        .assert()
        .success();
    assert_eq!(
        count_cache_entries(cache.path()),
        0,
        "--no-cache neither reads nor writes the cache"
    );
}

#[test]
fn vcs_clear_cache_wipes_persisted_entries() {
    let repo = repo_two_commits();
    let cache = tempfile::tempdir().expect("cache dir");
    let cache_arg = cache.path().to_str().expect("utf8");

    cli()
        .current_dir(repo.path())
        .args(["vcs", "--paths", ".", "--cache-dir", cache_arg])
        .assert()
        .success();
    assert!(count_cache_entries(cache.path()) >= 1);

    cli()
        .current_dir(repo.path())
        .args([
            "vcs",
            "--paths",
            ".",
            "--no-cache",
            "--clear-cache",
            "--cache-dir",
            cache_arg,
        ])
        .assert()
        .success();
    assert_eq!(
        count_cache_entries(cache.path()),
        0,
        "cleared, not re-primed"
    );
}

#[test]
fn vcs_table_is_the_default_output() {
    let repo = repo_two_commits();
    cli()
        .current_dir(repo.path())
        .args(["vcs", "--paths", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("RANK").and(predicate::str::contains("src/work.rs")));
}

#[test]
fn metrics_vcs_attaches_a_block_with_hotspot() {
    let repo = repo_two_commits();
    let assert = cli()
        .current_dir(repo.path())
        .args([
            "metrics",
            "--vcs",
            "--paths",
            "src/work.rs",
            "--format",
            "json",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    let vcs = &doc["metrics"]["vcs"];
    assert!(vcs.is_object(), "metrics.vcs block present");
    assert_eq!(vcs["commits_long"], 2);
    // hotspot = cyclomatic_sum × churn_recent; both factors are positive
    // for this fixture, so the product must be strictly positive — a
    // zeroed or dropped-multiplier bug fails this, unlike `is_number`.
    let hotspot = vcs["hotspot_score"]
        .as_f64()
        .expect("hotspot_score is a number");
    assert!(hotspot > 0.0, "hotspot_score positive, got {hotspot}");
}

#[test]
fn metrics_vcs_per_function_attaches_nested_blocks() {
    // `--vcs-per-function` implies `--vcs`, so the file-level block is
    // still present, and every nested function space additionally carries
    // a blame-derived block. The fixture's `src/work.rs` ends with two
    // top-level functions, so the JSON tree must show two nested spaces,
    // each with its own `vcs` object.
    let repo = repo_two_commits();
    let assert = cli()
        .current_dir(repo.path())
        .args([
            "metrics",
            "--vcs-per-function",
            "--paths",
            "src/work.rs",
            "--format",
            "json",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json");

    // File-level block still attached (implied --vcs).
    assert!(
        doc["metrics"]["vcs"].is_object(),
        "file-level vcs block present under --vcs-per-function"
    );

    let spaces = doc["spaces"].as_array().expect("nested spaces array");
    let with_vcs: Vec<&serde_json::Value> = spaces
        .iter()
        .filter(|s| s["metrics"]["vcs"].is_object())
        .collect();
    assert_eq!(
        with_vcs.len(),
        2,
        "both nested functions carry a per-function vcs block"
    );
    // Every nested block must carry a per-function hotspot score derived
    // from that function's *own* cyclomatic complexity (not the subtree
    // `cyclomatic_sum`, so nested branches are not re-counted at every
    // enclosing level — issue #709).
    for space in with_vcs {
        assert!(
            space["metrics"]["vcs"]["hotspot_score"].is_number(),
            "per-function hotspot_score present"
        );
    }
}

/// One unblameable file must not abort the walk: `inject_per_function`
/// warns with the `--vcs-per-function: skipping <path>` line and leaves
/// the nested blocks unset, while the file's AST metrics still emit.
/// The fixture is an *untracked* file with two functions: it is inside
/// the work tree, so the engine opens and the file-level `inject` finds
/// no index row, and the blame fails because the file does not exist at
/// `HEAD`. Pins the `Err` arm of `inject_per_function`: a version that
/// dropped the warning passes the JSON assertions and fails the stderr
/// one; one that propagated the error fails `success()`.
#[test]
fn metrics_vcs_per_function_warns_and_skips_an_unblameable_file() {
    let repo = repo_two_commits();
    std::fs::write(
        repo.path().join("src/untracked.rs"),
        "fn c() {}\nfn d() {}\n",
    )
    .expect("write untracked file");
    let assert = cli()
        .current_dir(repo.path())
        .args([
            "metrics",
            "--vcs-per-function",
            "--paths",
            "src/untracked.rs",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .stderr(
            predicate::str::contains("warning: --vcs-per-function: skipping")
                .and(predicate::str::contains("untracked.rs")),
        );
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json still emitted");
    assert!(
        doc["metrics"].get("vcs").is_none(),
        "an untracked file has no file-level block either"
    );
    let spaces = doc["spaces"].as_array().expect("nested spaces array");
    assert_eq!(
        spaces.len(),
        2,
        "both functions still emit their AST metrics"
    );
    assert!(
        spaces.iter().all(|s| s["metrics"].get("vcs").is_none()),
        "the per-function blocks are left unset after the blame fails"
    );
}

#[test]
fn metrics_without_vcs_flag_has_no_block() {
    let repo = repo_two_commits();
    let assert = cli()
        .current_dir(repo.path())
        .args(["metrics", "--paths", "src/work.rs", "--format", "json"])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert!(
        doc["metrics"].get("vcs").is_none(),
        "no vcs block without --vcs"
    );
}

#[test]
fn vcs_outside_a_repo_errors_with_nonzero_exit() {
    let dir = tempfile::tempdir().expect("tempdir");
    cli()
        .current_dir(dir.path())
        .args(["vcs", "--paths", "."])
        .assert()
        .failure()
        .stderr(predicate::str::contains("version-control"));
}

#[test]
fn metrics_vcs_outside_repo_warns_and_still_emits() {
    // `--vcs` is an additive opt-in: outside a git repo it must NOT abort
    // the metrics run — it warns and omits the vcs block (issue #328).
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("work.rs"), "fn a() {}\n").expect("write");
    let assert = cli()
        .current_dir(dir.path())
        .args(["metrics", "--vcs", "--paths", "work.rs", "--format", "json"])
        .assert()
        .success()
        .stderr(predicate::str::contains("warning"));
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json still emitted");
    assert!(
        doc["metrics"].get("cyclomatic").is_some(),
        "AST metrics still present"
    );
    assert!(
        doc["metrics"].get("vcs").is_none(),
        "vcs block omitted outside a repo"
    );
}

#[test]
fn vcs_csv_format_emits_header() {
    let repo = repo_two_commits();
    cli()
        .current_dir(repo.path())
        .args(["vcs", "--paths", ".", "--format", "csv"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("path,risk_score,commits_long")
                .and(predicate::str::contains("src/work.rs")),
        );
}

#[test]
fn vcs_emit_author_details_controls_author_ids() {
    let repo = repo_two_commits();
    // Returns the `author_ids` field via the object accessor so an
    // *absent* key (serde-skipped) is distinguished from an explicit
    // `null` — indexing would conflate the two.
    let ids = |extra: &[&str]| -> Option<serde_json::Value> {
        let mut args = vec!["vcs", "--paths", ".", "--format", "json"];
        args.extend_from_slice(extra);
        let assert = cli().current_dir(repo.path()).args(args).assert().success();
        let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
        let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json");
        // `author_ids` lives inside the nested per-file `vcs` block (#684).
        doc["files"][0]["vcs"].get("author_ids").cloned()
    };
    // Off by default; opt-in surfaces a (sorted) array of hashed ids.
    assert!(ids(&[]).is_none(), "author_ids key absent by default");
    let with = ids(&["--emit-author-details"]).expect("author_ids present under the flag");
    let arr = with.as_array().expect("author_ids is an array");
    assert_ne!(
        arr,
        &[] as &[serde_json::Value; 0],
        "author_ids lists at least one id under the flag"
    );
    assert!(
        arr[0].as_str().is_some_and(|h| h.len() == 64),
        "SHA-256 hex"
    );
}

#[test]
fn vcs_author_hash_key_hardens_the_emitted_ids() {
    let repo = repo_two_commits();
    // First file's `author_ids` array for the given extra args / env.
    let ids = |extra: &[&str], env: Option<(&str, &str)>| -> Vec<String> {
        let mut args = vec!["vcs", "--paths", ".", "--format", "json"];
        args.extend_from_slice(extra);
        let mut cmd = cli();
        cmd.current_dir(repo.path()).args(args);
        if let Some((key, value)) = env {
            cmd.env(key, value);
        }
        let assert = cmd.assert().success();
        let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
        let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json");
        doc["files"][0]["vcs"]["author_ids"]
            .as_array()
            .expect("author_ids array")
            .iter()
            .map(|v| v.as_str().expect("hex string").to_owned())
            .collect()
    };

    let plain = ids(&["--emit-author-details"], None);
    let keyed = ids(
        &["--emit-author-details", "--author-hash-key", "team-secret"],
        None,
    );
    // The key hardens every id (HMAC) while keeping the SHA-256 hex width.
    assert_eq!(keyed.len(), plain.len());
    assert_ne!(keyed, plain);
    assert!(keyed.iter().all(|h| h.len() == 64));
    // Deterministic for a fixed key (stable cross-report pseudonyms).
    assert_eq!(
        keyed,
        ids(
            &["--emit-author-details", "--author-hash-key", "team-secret"],
            None
        )
    );
    // The `BCA_AUTHOR_HASH_KEY` env var is equivalent to the flag.
    assert_eq!(
        keyed,
        ids(
            &["--emit-author-details"],
            Some(("BCA_AUTHOR_HASH_KEY", "team-secret"))
        )
    );
}

#[test]
fn vcs_author_hash_key_requires_emit_author_details() {
    let repo = repo_two_commits();
    // The key only hardens *emitted* digests; supplying it without
    // `--emit-author-details` is a loud usage error, not a silent no-op.
    cli()
        .current_dir(repo.path())
        .args(["vcs", "--paths", ".", "--author-hash-key", "team-secret"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires --emit-author-details"));
}

#[test]
fn vcs_include_deleted_surfaces_removed_file() {
    let now = now();
    let repo = tempfile::tempdir().expect("tempdir");
    git_at(repo.path(), now, &["init", "-q", "-b", "main"]);
    git_at(repo.path(), now, &["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.path().join("gone.rs"), "fn g() {}\n").expect("write");
    git_at(repo.path(), now - 30 * DAY, &["add", "."]);
    git_at(repo.path(), now - 30 * DAY, &["commit", "-qm", "add gone"]);
    git_at(repo.path(), now - 10 * DAY, &["rm", "-q", "gone.rs"]);
    std::fs::write(repo.path().join("kept.rs"), "fn k() {}\n").expect("write");
    git_at(repo.path(), now - 10 * DAY, &["add", "."]);
    git_at(
        repo.path(),
        now - 10 * DAY,
        &["commit", "-qm", "remove gone"],
    );

    let paths = |extra: &[&str]| -> Vec<String> {
        let mut args = vec!["vcs", "--paths", ".", "--format", "json"];
        args.extend_from_slice(extra);
        let assert = cli().current_dir(repo.path()).args(args).assert().success();
        let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
        let doc: serde_json::Value = serde_json::from_str(&stdout).expect("json");
        doc["files"]
            .as_array()
            .expect("files")
            .iter()
            .map(|f| f["path"].as_str().unwrap_or_default().to_owned())
            .collect()
    };
    // Default ranking omits the deleted file (it is not on disk); the
    // opt-in pulls it from the index via `append_deleted_entries`.
    assert!(!paths(&[]).iter().any(|p| p == "gone.rs"));
    assert!(
        paths(&["--include-deleted"]).iter().any(|p| p == "gone.rs"),
        "deleted file surfaced with --include-deleted"
    );
}

#[test]
fn vcs_markdown_renders_a_ranked_table_to_stdout() {
    let repo = repo_two_commits();
    cli()
        .current_dir(repo.path())
        .args(["vcs", "--paths", ".", "--format", "markdown"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("# Change-history risk")
                .and(predicate::str::contains("| Rank |"))
                .and(predicate::str::contains("src/work.rs")),
        );
}

#[test]
fn vcs_html_writes_a_single_self_contained_file() {
    let repo = repo_two_commits();
    let out = repo.path().join("vcs.html");
    cli()
        .current_dir(repo.path())
        .args([
            "vcs",
            "--paths",
            ".",
            "--format",
            "html",
            "--output",
            out.to_str().expect("utf8 path"),
        ])
        .assert()
        .success();
    // The whole-repo report is a single document, so `--output` must name
    // one file — not the per-file *directory* `metrics`/`ops` write
    // (issue #573). Before the fix this path was a directory holding
    // `vcs.html/vcs.html`.
    assert!(out.is_file(), "--output must write a single file");
    let html = std::fs::read_to_string(&out).expect("read html");
    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains("<table class=\"hotspot\">"));
    assert!(html.contains("src/work.rs"));
}

/// The HTML risk-heat band grades a row against every file that was
/// ranked, not only the rows `--top` kept. With three tracked files and
/// `--top 1`, the one row shown is the repository's riskiest file and
/// must take the most-severe band; before the fix it was graded as the
/// last of one row and painted the least-severe one. Driven through the
/// binary so the pre-truncation count recorded in `rank()` is what is
/// tested, for both the standalone `vcs` page and the `report --vcs`
/// section.
#[test]
fn vcs_top_one_heats_the_riskiest_file_as_most_severe() {
    let now = now();
    let dir = tempfile::tempdir().expect("tempdir");
    let git = |secs: i64, args: &[&str]| git_at(dir.path(), secs, args);
    git(now, &["init", "-q", "-b", "main"]);
    git(now, &["config", "commit.gpgsign", "false"]);
    std::fs::create_dir(dir.path().join("src")).expect("mkdir");
    for name in ["a", "b", "c"] {
        std::fs::write(
            dir.path().join(format!("src/{name}.rs")),
            format!("fn {name}() {{}}\n"),
        )
        .expect("write");
    }
    git(now - 200 * DAY, &["add", "."]);
    git(now - 200 * DAY, &["commit", "-q", "-m", "add three"]);
    // Only `a.rs` changes recently, so it ranks first by risk.
    std::fs::write(
        dir.path().join("src/a.rs"),
        "fn a() {}\nfn a2() {}\nfn a3() {}\n",
    )
    .expect("write");
    git(now - 5 * DAY, &["commit", "-aqm", "fix bug in a"]);

    let risk_cell = |html: &str| -> String {
        let body = html.rsplit_once("</style>").map_or(html, |(_, b)| b);
        let row = body
            .split("<tr>")
            .find(|r| r.contains("src/a.rs"))
            .expect("a.rs row");
        let start = row.find("risk-heat-").expect("heated risk cell");
        row[start..start + "risk-heat-N".len()].to_owned()
    };

    let out = dir.path().join("vcs.html");
    cli()
        .current_dir(dir.path())
        .args([
            "vcs",
            "--paths",
            ".",
            "--top",
            "1",
            "--format",
            "html",
            "--output",
            out.to_str().expect("utf8 path"),
        ])
        .assert()
        .success();
    let html = std::fs::read_to_string(&out).expect("read html");
    assert!(!html.contains("src/b.rs"), "--top 1 shows one row:\n{html}");
    assert_eq!(risk_cell(&html), "risk-heat-0");

    let report = cli()
        .current_dir(dir.path())
        .args(["report", "html", "--vcs", "--top", "1", "--paths", "."])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report = String::from_utf8(report).expect("utf8 html");
    let section = report
        .split(">Change-history risk</h2>")
        .nth(1)
        .expect("change-history section");
    assert_eq!(risk_cell(section), "risk-heat-0");
}

#[test]
fn vcs_json_output_writes_a_single_file() {
    // Regression guard for the `--output` semantics fix: structured
    // formats also write one file now, not a `<dir>/vcs.json` tree.
    let repo = repo_two_commits();
    let out = repo.path().join("vcs.json");
    cli()
        .current_dir(repo.path())
        .args([
            "vcs",
            "--paths",
            ".",
            "--format",
            "json",
            "--output",
            out.to_str().expect("utf8 path"),
        ])
        .assert()
        .success();
    assert!(out.is_file(), "--output must write a single file");
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).expect("read json")).expect("json");
    // Non-empty + names the tracked file, so a regression that writes an
    // empty document to the single file still fails here.
    let files = doc["files"].as_array().expect("files array");
    assert!(
        files.iter().any(|f| f["path"] == "src/work.rs"),
        "single-file report ranks the tracked file"
    );
}

#[test]
fn vcs_output_creates_missing_parent_directory() {
    // Regression guard for #709 at the *command* level: the `vcs` emit
    // path must create a missing `--output` parent directory, not fail
    // with "No such file or directory". The `formats::write_text` unit
    // test covers the helper in isolation; this proves `vcs emit` actually
    // routes through it (text/markdown path). Before the dedup fix, `vcs`
    // wrote straight to `std::fs::write` and this command failed.
    let repo = repo_two_commits();
    let out = repo.path().join("missing").join("dir").join("vcs.md");
    assert!(
        !out.parent().expect("has parent").exists(),
        "parent must be absent before the run"
    );
    cli()
        .current_dir(repo.path())
        .args([
            "vcs",
            "--paths",
            ".",
            "--format",
            "markdown",
            "--output",
            out.to_str().expect("utf8 path"),
        ])
        .assert()
        .success();
    let md = std::fs::read_to_string(&out).expect("report written to created dir");
    assert!(
        md.contains("src/work.rs"),
        "the created file holds the rendered report"
    );

    // The CSV arm writes through a separate `File::create` sink, so it
    // needs its own parent-dir guard — pin it too (it was the one arm the
    // dedup pass initially missed).
    let csv_out = repo.path().join("csv").join("nested").join("vcs.csv");
    assert!(!csv_out.parent().expect("has parent").exists());
    cli()
        .current_dir(repo.path())
        .args([
            "vcs",
            "--paths",
            ".",
            "--format",
            "csv",
            "--output",
            csv_out.to_str().expect("utf8 path"),
        ])
        .assert()
        .success();
    assert!(
        std::fs::read_to_string(&csv_out)
            .expect("csv written to created dir")
            .contains("src/work.rs"),
        "the created CSV holds the rendered rows"
    );
}

#[test]
fn vcs_html_is_deterministic_across_runs() {
    let repo = repo_two_commits();
    let render = || {
        let assert = cli()
            .current_dir(repo.path())
            .args(["vcs", "--paths", ".", "--format", "html"])
            .assert()
            .success();
        assert.get_output().stdout.clone()
    };
    assert_eq!(render(), render(), "HTML output must be byte-stable");
}

#[test]
fn report_markdown_vcs_appends_change_history_section() {
    let repo = repo_two_commits();
    cli()
        .current_dir(repo.path())
        .args(["report", "markdown", "--vcs", "--paths", "."])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("## Change-history risk")
                .and(predicate::str::contains("| Rank |"))
                .and(predicate::str::contains("src/work.rs")),
        );
}

#[test]
fn report_markdown_vcs_fills_the_hotspot_column() {
    // Issue #615: `report --vcs` computes AST metrics in the same run, so
    // the change-history section must join a real `hotspot_score`
    // (complexity × recent churn) onto each file — not leave the trailing
    // column blank. The fixture file has a non-zero recent churn and a
    // cyclomatic sum > 0, so a join produces a positive value.
    let repo = repo_two_commits();
    let assert = cli()
        .current_dir(repo.path())
        .args(["report", "markdown", "--vcs", "--paths", "."])
        .assert()
        .success();
    let md = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    // The column header and legend entry are present (not omitted).
    assert!(md.contains("| Hotspot |"), "Hotspot header should render");
    // Locate the data row for the tracked file and read its trailing
    // Hotspot cell — the last `|`-delimited field. It must parse to a
    // positive number, proving the join happened.
    let row = md
        .lines()
        .find(|l| l.starts_with('|') && l.contains("src/work.rs"))
        .expect("a data row for src/work.rs");
    let hotspot_cell = row
        .trim_end_matches('|')
        .rsplit('|')
        .next()
        .expect("a trailing Hotspot cell")
        .trim();
    let value: f64 = hotspot_cell
        .parse()
        .unwrap_or_else(|_| panic!("Hotspot cell {hotspot_cell:?} should be a number, not blank"));
    assert!(
        value > 0.0,
        "joined hotspot score should be positive, got {value}"
    );
}

#[test]
fn report_html_vcs_appends_sortable_change_history_section() {
    let repo = repo_two_commits();
    cli()
        .current_dir(repo.path())
        .args(["report", "html", "--vcs", "--paths", "."])
        .assert()
        .success()
        .stdout(
            predicate::str::contains(">Change-history risk</h2>")
                .and(predicate::str::contains("<table class=\"hotspot\">"))
                .and(predicate::str::contains("src/work.rs")),
        );
}

#[test]
fn report_without_vcs_flag_has_no_change_history_section() {
    let repo = repo_two_commits();
    cli()
        .current_dir(repo.path())
        .args(["report", "markdown", "--paths", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("Change-history risk").not());
}

#[test]
fn report_vcs_outside_a_repo_warns_and_still_renders() {
    // `report --vcs` is additive like `metrics --vcs`: outside a git tree
    // it must warn and still emit the AST report, just without the
    // change-history section (issue #573).
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("work.rs"), "fn a() {}\n").expect("write");
    cli()
        .current_dir(dir.path())
        .args(["report", "markdown", "--vcs", "--paths", "work.rs"])
        .assert()
        .success()
        .stderr(predicate::str::contains("warning"))
        // The AST report must still render (the "still renders" claim) —
        // only the change-history section is omitted.
        .stdout(
            predicate::str::contains("Code Quality Metrics Summary")
                .and(predicate::str::contains("Change-history risk").not()),
        );
}

#[test]
fn bad_long_window_names_the_flag_and_echoes_the_input() {
    // Regression for #607: the failure used to name neither the flag nor
    // the offending input (it quoted the split-off empty magnitude as
    // `""`). The CLI must now identify `--long-window` and echo `bogus`,
    // plus the parser's accepted-format hint.
    let repo = repo_two_commits();
    cli()
        .current_dir(repo.path())
        .args(["vcs", "--paths", ".", "--long-window", "bogus"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("--long-window")
                .and(predicate::str::contains("\"bogus\""))
                .and(predicate::str::contains("expected")),
        );
}

#[test]
fn bad_recent_window_names_its_own_flag() {
    // The sibling flag must surface its own name, so a CI invocation
    // passing both windows points at the actual offender (#607).
    let repo = repo_two_commits();
    cli()
        .current_dir(repo.path())
        .args(["vcs", "--paths", ".", "--recent-window", "12parsec"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("--recent-window")
                .and(predicate::str::contains("\"12parsec\"")),
        );
}

/// #1131: `bca vcs` intersects the standard walk with the git index, so
/// a directory it cannot list silently shortens the ranking exactly as
/// it shortened a metrics document. `vcs` reads history rather than file
/// contents, so this traversal tally is the only I/O failure it can
/// observe — without it the run reported a complete ranking over a tree
/// it had not finished walking.
///
/// The readable control run is what makes the claim: it shows the same
/// invocation ranking `src/nested/inner.rs`, so the locked run's exit 1
/// is the guard and not an unrelated rejection.
#[cfg(unix)]
#[test]
fn vcs_exits_one_when_a_directory_cannot_be_listed() {
    let repo = repo_two_commits();
    let nested = repo.path().join("src/nested");
    std::fs::create_dir(&nested).expect("mkdir nested");
    std::fs::write(nested.join("inner.rs"), "fn c() {}\n").expect("write inner");
    let now = now();
    git_at(repo.path(), now - 3 * DAY, &["add", "."]);
    git_at(repo.path(), now - 3 * DAY, &["commit", "-qm", "add nested"]);

    cli()
        .current_dir(repo.path())
        .args(["vcs", "--paths", ".", "--format", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/nested/inner.rs"));

    if !common::deny_dir_listing(&nested) {
        eprintln!("skipping: this process can list a mode-000 directory");
        return;
    }

    cli()
        .current_dir(repo.path())
        .args(["vcs", "--paths", ".", "--format", "json"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "1 directory entry could not be read",
        ))
        .stdout(predicate::str::is_empty());

    common::restore_dir_access(&nested);
}
