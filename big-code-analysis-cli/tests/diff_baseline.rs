//! Integration tests for the `bca diff-baseline` subcommand (issue
//! #382). These drive the `bca` binary against on-disk baseline files
//! and verify the end-to-end contract: a structured diff regardless of
//! emptiness, all three output formats, the `--*-only` filters, and the
//! clear-error path on an unsupported version.

use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

mod common;

fn cli() -> Command {
    common::bca_command()
}

const OLD: &str = r#"version = 4
[[entry]]
path = "src/foo.rs"
qualified = "do_thing"
start_line = 10
metric = "cognitive"
value = 25.0
[[entry]]
path = "src/bar.rs"
qualified = "act_on_file"
start_line = 500
metric = "cognitive"
value = 60.0
[[entry]]
path = "src/gone.rs"
qualified = "old_fn"
start_line = 1
metric = "nargs"
value = 9.0
"#;

const NEW: &str = r#"version = 4
[[entry]]
path = "src/foo.rs"
qualified = "do_thing"
start_line = 10
metric = "cognitive"
value = 27.0
[[entry]]
path = "src/bar.rs"
qualified = "act_on_file"
start_line = 506
metric = "cognitive"
value = 63.0
[[entry]]
path = "src/new.rs"
qualified = "shiny"
start_line = 1
metric = "cognitive"
value = 30.0
"#;

/// Write `old.toml` / `new.toml` into a fresh tempdir and return it.
fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("old.toml"), OLD).unwrap();
    fs::write(dir.path().join("new.toml"), NEW).unwrap();
    dir
}

#[test]
fn tty_diff_reports_all_buckets_and_exits_zero() {
    let dir = fixture();
    cli()
        .current_dir(dir.path())
        .args(["diff-baseline", "old.toml", "new.toml"])
        .assert()
        .success()
        // 1 added (shiny), 1 removed (old_fn), 2 worsened (do_thing,
        // act_on_file), 0 improved.
        .stdout(predicate::str::starts_with(
            "1 added, 1 removed, 2 worsened, 0 improved\n",
        ))
        .stdout(predicate::str::contains("## Worsened"))
        .stdout(predicate::str::contains("src/bar.rs::act_on_file"))
        .stdout(predicate::str::contains("60 \u{2192} 63"))
        .stdout(predicate::str::contains("src/new.rs::shiny"))
        .stdout(predicate::str::contains("src/gone.rs::old_fn"));
}

#[test]
fn empty_diff_exits_zero_with_summary() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.toml"), "version = 4\n").unwrap();
    cli()
        .current_dir(dir.path())
        .args(["diff-baseline", "a.toml", "a.toml"])
        .assert()
        .success()
        .stdout("0 added, 0 removed, 0 worsened, 0 improved\n");
}

#[test]
fn markdown_format_fences_each_section() {
    let dir = fixture();
    cli()
        .current_dir(dir.path())
        .args([
            "diff-baseline",
            "old.toml",
            "new.toml",
            "--format",
            "markdown",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("## Worsened"))
        .stdout(predicate::str::contains("```text"));
}

#[test]
fn json_format_is_valid_and_carries_summary() {
    let dir = fixture();
    let out = cli()
        .current_dir(dir.path())
        .args(["diff-baseline", "old.toml", "new.toml", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let parsed: serde_json::Value = serde_json::from_slice(&out).expect("valid JSON");
    assert_eq!(parsed["summary"]["added"], 1);
    assert_eq!(parsed["summary"]["worsened"], 2);
    assert_eq!(parsed["removed"][0]["qualified"], "old_fn");
}

#[test]
fn worsened_only_filter_hides_other_sections() {
    let dir = fixture();
    cli()
        .current_dir(dir.path())
        .args(["diff-baseline", "old.toml", "new.toml", "--worsened-only"])
        .assert()
        .success()
        .stdout(predicate::str::contains("## Worsened"))
        .stdout(predicate::str::contains("## Added").not())
        .stdout(predicate::str::contains("## Removed").not())
        // Summary line still reports the full counts.
        .stdout(predicate::str::starts_with(
            "1 added, 1 removed, 2 worsened, 0 improved\n",
        ));
}

#[test]
fn unsupported_version_is_a_clear_error() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("ok.toml"), "version = 4\n").unwrap();
    fs::write(dir.path().join("future.toml"), "version = 99\n").unwrap();
    cli()
        .current_dir(dir.path())
        .args(["diff-baseline", "ok.toml", "future.toml"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("version 99 is not supported"));
}

#[test]
fn legacy_v2_emits_deprecation_warning_and_still_diffs() {
    // A v2 file stores a bare `function` name and a pre-canonical path.
    // Diffing it against a v4 file must migrate on read, warn once, and
    // still pair on the (here-identical) bare/qualified name.
    let dir = TempDir::new().unwrap();
    let legacy = r#"version = 2
[[entry]]
path = "src/foo.rs"
function = "do_thing"
start_line = 10
metric = "cognitive"
value = 20.0
"#;
    fs::write(dir.path().join("legacy.toml"), legacy).unwrap();
    fs::write(dir.path().join("new.toml"), NEW).unwrap();
    cli()
        .current_dir(dir.path())
        .args(["diff-baseline", "legacy.toml", "new.toml"])
        .assert()
        .success()
        .stderr(predicate::str::contains("baseline is v2"))
        .stdout(predicate::str::contains("src/foo.rs::do_thing"))
        .stdout(predicate::str::contains("20 \u{2192} 27"));
}
