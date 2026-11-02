//! Bakes build provenance into `bca` as `BCA_BUILD_*` environment
//! variables read by `src/build_info/mod.rs` through `env!` (#1773).
//!
//! The one field that *decides* anything is `BCA_BUILD_INPUTS`, the
//! content digest of everything compiled into the binary; the
//! self-scan resolver compares it against the live tree to tell
//! whether an installed `bca` implements the checkout it is about to
//! measure. The rest — git revision, build time, toolchain — is
//! forensic: it lets a baseline, report or bug report be traced to a
//! build, and nothing reads it to make a choice.
//!
//! Two layouts reach this script. From a workspace checkout every root
//! in `INPUT_ROOTS` exists beside this crate and the digest is taken.
//! From a registry tarball (`cargo install big-code-analysis-cli`, or
//! `cargo package`'s verification copy) the crate stands alone, so the
//! digest is reported `unavailable` and — this matters — *no*
//! `rerun-if-changed` is emitted for a path that does not exist, since
//! cargo treats a listed-but-absent path as permanently dirty.

use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "src/build_info/digest.rs"]
#[allow(dead_code)]
mod digest;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");

    let manifest_dir = PathBuf::from(env_or_empty("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir.parent().map(Path::to_path_buf);
    let inputs = workspace.as_deref().and_then(emit_inputs_digest);

    let git = workspace
        .as_deref()
        .filter(|_| inputs.is_some())
        .and_then(git_revision)
        .unwrap_or_else(|| "unavailable".to_owned());

    set(
        "BCA_BUILD_INPUTS",
        &inputs.unwrap_or_else(|| "unavailable".to_owned()),
    );
    set("BCA_BUILD_GIT", &git);
    set("BCA_BUILD_TIME", &build_time());
    set("BCA_BUILD_RUSTC", &rustc_version());
    set("BCA_BUILD_TARGET", &env_or_empty("TARGET"));
    set("BCA_BUILD_PROFILE", &env_or_empty("PROFILE"));
}

fn set(key: &str, value: &str) {
    println!("cargo:rustc-env={key}={value}");
}

fn env_or_empty(key: &str) -> String {
    std::env::var(key).unwrap_or_default()
}

/// The digest of `workspace`, after registering every hashed file with
/// cargo so an edit reruns this script. Only files, never a directory:
/// cargo's directory watch is recursive and fires on the newest mtime
/// beneath it, which under a vendored grammar includes its untracked
/// `target/` and under `src/` every `.snap` — so `make release-check`
/// or an `insta` accept would otherwise force the very relink this
/// digest exists to avoid. The price is that a *new* input file is not
/// seen until something already watched changes; an added file with no
/// edit to any existing one is rare enough that the fresh-walk test in
/// `build_info_tests.rs` is the backstop rather than a watch.
///
/// `None` when the layout is not a workspace checkout; nothing is
/// registered then except `build.rs` itself.
fn emit_inputs_digest(workspace: &Path) -> Option<String> {
    let walked = digest::input_files(workspace).and_then(|files| {
        for rel in &files {
            println!("cargo:rerun-if-changed={}", workspace.join(rel).display());
        }
        digest::digest_of(workspace, &files)
    });
    match walked {
        Ok(digest) => Some(digest),
        // The one silent case: no workspace beside the crate, which is
        // what a crates.io build looks like and not a defect.
        Err(digest::DigestError::MissingRoot(_)) => None,
        // Anything else is a real checkout the digest cannot describe —
        // a symlink, a non-UTF-8 name, an unreadable file. The binary
        // still builds, reporting `unavailable`, but the cause is said
        // at the build that produced it rather than at the next test.
        Err(e) => {
            println!("cargo:warning=build-inputs digest unavailable: {e}");
            None
        }
    }
}

/// Run `git` with the repository-selecting variables scrubbed and
/// discovery fenced so that only a `.git` *in* `workspace` is found:
/// a packaged copy under `target/package/`, or a tree exported into
/// another checkout, then reports `unavailable` rather than stamping
/// the enclosing repository's revision, and a `pre-commit` hook's
/// temporary `GIT_INDEX_FILE` cannot leak in. The ceiling is the
/// workspace's *parent* — git never excludes the directory it starts
/// in, only the ones it would climb into. `None` on any failure.
fn git(workspace: &Path, args: &[&str]) -> Option<String> {
    let ceiling = workspace.parent().unwrap_or(workspace);
    let out = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env("GIT_CEILING_DIRECTORIES", ceiling)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// `<sha>` or `<sha>-dirty` for the checkout `workspace` lies in, or
/// `None` outside one. The dirtiness pathspec is the digest's own
/// roots — not its extension filter, so an untracked `.snap.new` reads
/// `-dirty` while the digest stands; the field is informational and
/// that over-approximation is the cheaper side to err on.
///
/// Nothing under `.git/` is watched, on purpose. `BCA_BUILD_GIT` is an
/// `env!` the library reads, so a changed value invalidates the
/// library's fingerprint and costs a recompile plus the `bca` relink —
/// ~4 s dev, ~100 s release. Watching `HEAD` would charge that on
/// every commit, and on every step of the `git rebase --exec` loop
/// #1773 was opened to make cheap, for a step that compiled nothing.
/// So the field is exact as of the last build whose *inputs* changed;
/// `build inputs`, the one field anything decides on, is exact always.
fn git_revision(workspace: &Path) -> Option<String> {
    let sha = git(workspace, &["rev-parse", "HEAD"])?;
    let mut status = vec![
        "status",
        "--porcelain",
        "--untracked-files=all",
        "--ignore-submodules=all",
        "--",
    ];
    status.extend(digest::INPUT_ROOTS);
    let dirty = git(workspace, &status).is_some_and(|s| !s.is_empty());
    Some(if dirty { format!("{sha}-dirty") } else { sha })
}

/// RFC 3339 UTC, from `SOURCE_DATE_EPOCH` when set (the
/// reproducible-builds convention the report footer already honours)
/// and the wall clock otherwise.
fn build_time() -> String {
    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs())
        })
        .unwrap_or(0);
    format_rfc3339(secs)
}

/// Civil-from-days, Howard Hinnant's algorithm; the same one
/// `src/provenance.rs` uses for the report footer date. The names are
/// the algorithm's own.
#[allow(clippy::many_single_char_names)]
fn format_rfc3339(secs: u64) -> String {
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (hour, min, sec) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

/// `rustc 1.99.0 (b940084d7 2026-09-28)` from the compiler cargo is
/// driving (`RUSTC`), not whatever `rustc` is first on PATH.
fn rustc_version() -> String {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    Command::new(rustc)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map_or_else(
            || "unavailable".to_owned(),
            |o| String::from_utf8_lossy(&o.stdout).trim().to_owned(),
        )
}
