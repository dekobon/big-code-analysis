#![allow(missing_docs)]
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::{NamedTempFile, TempDir};

use crate::common;

/// Hermetic `bca` builder rooted at a `.git`-free tempdir, returned
/// alongside its guard. These tests analyse an absolute `--paths`
/// tempfile, so without a cwd anchor `bca`'s cwd-relative manifest
/// discovery would climb to the repo's own `bca.toml` and merge its
/// walk-scope / exit-code globals into the run (#491, #918). The
/// returned `TempDir` must outlive the command spawn.
fn cli() -> (TempDir, Command) {
    common::cli_hermetic()
}

/// Since #663 an *explicitly-named* file with an unrecognized language
/// warns unconditionally (not gated behind `-w`) and — being the sole
/// input that produced no output — exits 1, mirroring the #596
/// nonexistent-explicit-path rule. The `-w` walk-warning gate now only
/// covers directory-discovered files (see `paths_discovery.rs`).
#[test]
fn explicit_unrecognized_warns_and_exits_one() {
    let tmp = NamedTempFile::with_suffix(".unknownlang123").unwrap();
    std::fs::write(tmp.path(), "some content\n").unwrap();

    let (_cwd, mut cmd) = cli();
    cmd.args(["metrics", "--paths", tmp.path().to_str().unwrap()])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "warning: skipping explicitly-named file with unrecognized language:",
        ));
}

/// The explicit-file warning fires with or without `-w`: it is a user
/// error (the user named one file and got nothing), not a noisy
/// directory-walk skip.
#[test]
fn explicit_unrecognized_warns_even_with_warning_flag() {
    let tmp = NamedTempFile::with_suffix(".unknownlang123").unwrap();
    std::fs::write(tmp.path(), "some content\n").unwrap();

    let (_cwd, mut cmd) = cli();
    cmd.args(["metrics", "-w", "--paths", tmp.path().to_str().unwrap()])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "warning: skipping explicitly-named file with unrecognized language:",
        ));
}

#[test]
fn warning_flag_emits_empty_file() {
    let tmp = NamedTempFile::with_suffix(".rs").unwrap();
    // File is already empty by default.

    let (_cwd, mut cmd) = cli();
    cmd.args(["metrics", "-w", "--paths", tmp.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains("warning: skipping empty file:"));
}

/// #1287: the walk-skip warning names the gate that fired. Until the
/// library gained a classified reader, every declined file — a one-byte
/// source, a multi-kilobyte binary, a UTF-16 document — was announced as
/// "skipping empty file", which is false for all three. Each case below
/// asserts its own reason *and* that the emptiness claim is absent, so
/// reverting the CLI to the single message fails every one of them.
#[test]
fn warning_flag_names_the_skip_reason_for_a_tiny_file() {
    let tmp = NamedTempFile::with_suffix(".rs").unwrap();
    // One byte: a valid identifier, and nothing like empty.
    std::fs::write(tmp.path(), b"x").unwrap();

    let (_cwd, mut cmd) = cli();
    cmd.args(["metrics", "-w", "--paths", tmp.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(
            predicate::str::contains(
                "warning: skipping file too small to analyze (3 bytes or fewer):",
            )
            .and(predicate::str::contains("empty").not()),
        );
}

#[test]
fn warning_flag_names_the_skip_reason_for_a_binary_file() {
    let tmp = NamedTempFile::with_suffix(".rs").unwrap();
    // A stray 0xFF (never a valid UTF-8 lead byte) inside the 64-byte
    // probe, padded well past the probe so the too-small gate cannot
    // claim the file first.
    let mut bytes = b"\x00\x01\xFF binary payload".to_vec();
    bytes.extend_from_slice(&[0xAB; 200]);
    std::fs::write(tmp.path(), &bytes).unwrap();

    let (_cwd, mut cmd) = cli();
    cmd.args(["metrics", "-w", "--paths", tmp.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(
            predicate::str::contains("warning: skipping file with non-UTF-8 contents:")
                .and(predicate::str::contains("empty").not()),
        );
}

#[test]
fn warning_flag_names_the_skip_reason_for_a_utf16_file() {
    let tmp = NamedTempFile::with_suffix(".rs").unwrap();
    // UTF-16-LE BOM plus an interleaved-NUL ASCII body (#803): every body
    // byte is valid single-byte UTF-8, so only the BOM distinguishes this
    // from the non-UTF-8 case.
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(
        "fn main() {} // padding to exceed the 64-byte probe window\n"
            .bytes()
            .flat_map(|b| [b, 0x00]),
    );
    std::fs::write(tmp.path(), &bytes).unwrap();

    let (_cwd, mut cmd) = cli();
    cmd.args(["metrics", "-w", "--paths", tmp.path().to_str().unwrap()])
        .assert()
        .success()
        .stderr(
            predicate::str::contains("warning: skipping UTF-16 file (unsupported encoding):")
                .and(predicate::str::contains("empty").not()),
        );
}

/// Hermeticity guard for the `cli()` switch to `cli_hermetic` (#918): a
/// `bca.toml` discoverable from the inherited process cwd must not reach
/// an explicit `--paths` run. Walk-scope keys (`exclude`) don't apply to
/// explicitly-named files, but `cyclomatic_count_try` does change their
/// metric values, so this drives the process cwd into a `.git`-rooted
/// manifest that sets `cyclomatic_count_try = false` and asserts the
/// hermetic builder still reports the default count (the `?` operator
/// counts). Reverting `cli()` to `bca_command()` inherits the leaky cwd,
/// picks up the manifest, drops the `?` from the count, and fails this
/// test (test-via-revert; see `.claude/rules/testing.md`).
#[test]
fn hermetic_cwd_ignores_discoverable_manifest() {
    let probe = NamedTempFile::with_suffix(".rs").unwrap();
    // `g()?` contributes one cyclomatic edge only when `?` is counted
    // (the default). A discovered `cyclomatic_count_try = false` would
    // drop it from 4 to 3.
    std::fs::write(
        probe.path(),
        "fn f() -> Result<i32,()> { let x = g()?; Ok(x) }\nfn g() -> Result<i32,()> { Ok(1) }\n",
    )
    .unwrap();
    let probe_path = probe.path().to_str().unwrap();

    let manifest_dir = TempDir::new().unwrap();
    std::fs::create_dir(manifest_dir.path().join(".git")).unwrap();
    std::fs::write(
        manifest_dir.path().join("bca.toml"),
        "cyclomatic_count_try = false\n",
    )
    .unwrap();

    // Control: rooted at the manifest dir, `?` is not counted → sum 3.
    // Without this the hermetic assertion below would be vacuous.
    Command::cargo_bin("bca")
        .unwrap()
        .current_dir(manifest_dir.path())
        .args(["metrics", "--paths", probe_path, "-O", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"cyclomatic\":{\"sum\":3"));

    // Drive the *process* cwd into the leaky dir so a non-hermetic
    // `bca_command()` would inherit it.
    let _cwd = common::CwdGuard::enter(manifest_dir.path());

    // Hermetic: `cli_hermetic` anchors at a clean tempdir, so the
    // manifest is not discovered and the default count (`?` counted →
    // sum 4) holds.
    let (_guard, mut cmd) = cli();
    cmd.args(["metrics", "--paths", probe_path, "-O", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"cyclomatic\":{\"sum\":4"));
}
