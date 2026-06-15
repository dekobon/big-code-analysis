//! Regression tests for issue #605: the human-readable `text` dumps
//! must not emit ANSI color escapes when stdout is not a terminal
//! (every `assert_cmd` invocation pipes stdout, so the default `auto`
//! mode resolves to `never` here), must honor the `NO_COLOR`
//! convention, and must obey an explicit `--color` flag whose `always`
//! value overrides both tty detection and `NO_COLOR`.
//!
//! Before #605 all four terminal writers hard-coded
//! `ColorChoice::Always`, so `bca metrics > file` wrote raw `ESC[…`
//! sequences into the file regardless of context.

use assert_cmd::Command;
use tempfile::TempDir;

mod common;

/// The byte that opens every ANSI escape sequence (`ESC`, `0x1b`).
const ESC: u8 = 0x1b;

/// Build a `bca` command with CI env scrubbed and `NO_COLOR` removed,
/// so a `NO_COLOR` exported by the test runner cannot mask the
/// default-`auto` assertions. Tests that exercise `NO_COLOR` set it
/// back explicitly.
fn cli() -> Command {
    let mut cmd = common::bca_command();
    cmd.env_remove("NO_COLOR");
    cmd
}

/// Write a small C source file with one function and return its path
/// (kept alive by the returned `TempDir`).
fn fixture() -> (TempDir, String) {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("sample.c");
    std::fs::write(&src, "int add(int a, int b) {\n    return a + b;\n}\n").unwrap();
    let path = src.to_str().unwrap().to_string();
    (dir, path)
}

fn stdout_of(cmd: &mut Command) -> Vec<u8> {
    let out = cmd.output().expect("bca runs");
    assert!(out.status.success(), "bca exited non-zero: {out:?}");
    out.stdout
}

fn has_escape(bytes: &[u8]) -> bool {
    bytes.contains(&ESC)
}

#[test]
fn metrics_default_piped_has_no_ansi_escapes() {
    let (_dir, path) = fixture();
    let out = stdout_of(cli().args(["metrics", "--paths", &path]));
    assert!(
        !has_escape(&out),
        "piped default metrics output must be escape-free, got: {out:?}"
    );
}

#[test]
fn metrics_format_text_default_piped_has_no_ansi_escapes() {
    // The explicit `text` spelling (issue #604) must behave identically
    // to the no-`--format` default: normalize_text_format maps it back
    // to the colored tree path, which must still respect color mode.
    let (_dir, path) = fixture();
    let out = stdout_of(cli().args(["metrics", "--paths", &path, "--format", "text"]));
    assert!(
        !has_escape(&out),
        "piped `--format text` output must be escape-free, got: {out:?}"
    );
}

#[test]
fn metrics_color_always_forces_ansi_escapes_when_piped() {
    let (_dir, path) = fixture();
    let out = stdout_of(cli().args(["metrics", "--color", "always", "--paths", &path]));
    assert!(
        has_escape(&out),
        "`--color always` must emit escapes even when piped, got: {out:?}"
    );
}

#[test]
fn metrics_color_never_strips_ansi_escapes() {
    let (_dir, path) = fixture();
    let out = stdout_of(cli().args(["metrics", "--color", "never", "--paths", &path]));
    assert!(
        !has_escape(&out),
        "`--color never` must never emit escapes, got: {out:?}"
    );
}

#[test]
fn metrics_no_color_env_strips_escapes_in_auto_mode() {
    // `NO_COLOR` set (to any value) must not produce escapes under the
    // default `auto` mode. Note this end-to-end check cannot isolate the
    // `NO_COLOR` signal: `assert_cmd` always pipes stdout, so suppression
    // is over-determined (the pipe alone would suppress). The
    // `NO_COLOR`-deciding-on-a-terminal case is pinned by the
    // `color_auto_no_color_set_resolves_to_never_even_on_terminal` unit
    // test (#895); this test only guards that `NO_COLOR` does not somehow
    // *force* color on.
    let (_dir, path) = fixture();
    let out = stdout_of(
        cli()
            .env("NO_COLOR", "1")
            .args(["metrics", "--paths", &path]),
    );
    assert!(
        !has_escape(&out),
        "`NO_COLOR` must strip escapes in auto mode, got: {out:?}"
    );
}

#[test]
fn metrics_color_always_overrides_no_color_env() {
    // Precedence chain: an explicit `--color always` is the strongest
    // signal and wins over `NO_COLOR` (documented behavior, matching
    // cargo / ripgrep).
    let (_dir, path) = fixture();
    let out = stdout_of(
        cli()
            .env("NO_COLOR", "1")
            .args(["metrics", "--color", "always", "--paths", &path]),
    );
    assert!(
        has_escape(&out),
        "`--color always` must override NO_COLOR, got: {out:?}"
    );
}

#[test]
fn ops_default_piped_has_no_ansi_escapes() {
    let (_dir, path) = fixture();
    let out = stdout_of(cli().args(["ops", "--paths", &path]));
    assert!(!has_escape(&out), "piped ops output must be escape-free");
}

#[test]
fn ops_color_always_forces_ansi_escapes() {
    let (_dir, path) = fixture();
    let out = stdout_of(cli().args(["ops", "--color", "always", "--paths", &path]));
    assert!(has_escape(&out), "`ops --color always` must emit escapes");
}

#[test]
fn dump_default_piped_has_no_ansi_escapes() {
    let (_dir, path) = fixture();
    let out = stdout_of(cli().args(["dump", "--paths", &path]));
    assert!(!has_escape(&out), "piped dump output must be escape-free");
}

#[test]
fn dump_color_always_forces_ansi_escapes() {
    let (_dir, path) = fixture();
    let out = stdout_of(cli().args(["dump", "--color", "always", "--paths", &path]));
    assert!(has_escape(&out), "`dump --color always` must emit escapes");
}

#[test]
fn functions_default_piped_has_no_ansi_escapes() {
    let (_dir, path) = fixture();
    let out = stdout_of(cli().args(["functions", "--paths", &path]));
    assert!(
        !has_escape(&out),
        "piped functions output must be escape-free"
    );
}

#[test]
fn functions_color_always_forces_ansi_escapes() {
    let (_dir, path) = fixture();
    let out = stdout_of(cli().args(["functions", "--color", "always", "--paths", &path]));
    assert!(
        has_escape(&out),
        "`functions --color always` must emit escapes"
    );
}

#[test]
fn find_color_always_forces_ansi_escapes() {
    let (_dir, path) = fixture();
    let out = stdout_of(cli().args([
        "find",
        "--color",
        "always",
        "--paths",
        &path,
        "-t",
        "function_definition",
    ]));
    assert!(has_escape(&out), "`find --color always` must emit escapes");
}

#[test]
fn invalid_color_value_is_a_usage_error() {
    // An unknown `--color` value is a clap usage error → exit 1 (the
    // tool-error code, #594), not a silent fallthrough.
    let (_dir, path) = fixture();
    cli()
        .args(["metrics", "--color", "rainbow", "--paths", &path])
        .assert()
        .code(1);
}
