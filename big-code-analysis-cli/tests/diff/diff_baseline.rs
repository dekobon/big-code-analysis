//! Integration tests for the `bca diff-baseline` subcommand (issue
//! #382). These drive the `bca` binary against on-disk baseline files
//! and verify the end-to-end contract: a structured diff regardless of
//! emptiness, all three output formats, the `--*-only` filters, and the
//! clear-error path on an unsupported version.

use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

use crate::common;

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

/// #692: `--exit-code` over a non-empty diff exits with the metric-gate
/// code (2), so grammar-bump CI can branch on "anything changed".
#[test]
fn exit_code_flag_returns_two_on_non_empty_diff() {
    let dir = fixture();
    cli()
        .current_dir(dir.path())
        .args(["diff-baseline", "old.toml", "new.toml", "--exit-code"])
        .assert()
        .code(2)
        .stdout(predicate::str::starts_with(
            "1 added, 1 removed, 2 worsened, 0 improved\n",
        ));
}

/// #692: `--exit-code` over an identical pair exits 0 — the filtered
/// diff is empty.
#[test]
fn exit_code_flag_returns_zero_on_empty_diff() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.toml"), "version = 4\n").unwrap();
    cli()
        .current_dir(dir.path())
        .args(["diff-baseline", "a.toml", "a.toml", "--exit-code"])
        .assert()
        .success();
}

/// #692: `--exit-code` honors the active `--*-only` filter. The fixture
/// has no improved entries, so `--improved-only` filters the diff to
/// empty and the run exits 0 despite real added/removed/worsened deltas.
#[test]
fn exit_code_flag_respects_section_filter() {
    let dir = fixture();
    cli()
        .current_dir(dir.path())
        .args([
            "diff-baseline",
            "old.toml",
            "new.toml",
            "--exit-code",
            "--improved-only",
        ])
        .assert()
        .success();
}

/// #692: without `--exit-code`, a non-empty diff still exits 0 — the
/// default behavior is unchanged.
#[test]
fn non_empty_diff_without_flag_still_exits_zero() {
    let dir = fixture();
    cli()
        .current_dir(dir.path())
        .args(["diff-baseline", "old.toml", "new.toml"])
        .assert()
        .success();
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
fn output_flag_writes_to_file_and_stdout_stays_empty() {
    let dir = fixture();
    let out_path = dir.path().join("report.txt");
    cli()
        .current_dir(dir.path())
        .args([
            "diff-baseline",
            "old.toml",
            "new.toml",
            "--output",
            "report.txt",
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
    let written = fs::read_to_string(&out_path).expect("output file written");
    assert!(
        written.starts_with("1 added, 1 removed, 2 worsened, 0 improved\n"),
        "file content: {written}"
    );
    assert!(written.contains("src/bar.rs::act_on_file"));
}

#[test]
fn short_output_flag_is_accepted() {
    let dir = fixture();
    cli()
        .current_dir(dir.path())
        .args(["diff-baseline", "old.toml", "new.toml", "-o", "out.txt"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
    assert!(dir.path().join("out.txt").exists());
}

#[test]
fn strip_prefix_trims_displayed_paths() {
    let dir = fixture();
    cli()
        .current_dir(dir.path())
        .args([
            "diff-baseline",
            "old.toml",
            "new.toml",
            "--strip-prefix",
            "src/",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("bar.rs::act_on_file"))
        .stdout(predicate::str::contains("src/bar.rs").not());
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

/// #901: a paired value that *falls* for a higher-is-worse metric must
/// populate the `improved` bucket end-to-end — rendering a non-empty
/// `## Improved` section and an `X -> Y` row. The shared OLD/NEW
/// fixtures only ever raise values, so this uses a dedicated pair.
#[test]
fn falling_value_populates_improved_bucket() {
    let dir = TempDir::new().unwrap();
    let old = r#"version = 4
[[entry]]
path = "src/foo.rs"
qualified = "do_thing"
start_line = 10
metric = "cognitive"
value = 40.0
"#;
    let new = r#"version = 4
[[entry]]
path = "src/foo.rs"
qualified = "do_thing"
start_line = 10
metric = "cognitive"
value = 30.0
"#;
    fs::write(dir.path().join("old.toml"), old).unwrap();
    fs::write(dir.path().join("new.toml"), new).unwrap();
    cli()
        .current_dir(dir.path())
        .args(["diff-baseline", "old.toml", "new.toml"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "0 added, 0 removed, 0 worsened, 1 improved\n",
        ))
        .stdout(predicate::str::contains("## Improved"))
        .stdout(predicate::str::contains("src/foo.rs::do_thing"))
        .stdout(predicate::str::contains("40 \u{2192} 30"));
}

/// #825 / #901 end-to-end direction guard: `mi.original` is
/// lower-is-worse, so an MI *drop* is a regression (Worsened) and an MI
/// *rise* an improvement (Improved) — the opposite sentiment of the
/// numeric direction. Before #825 the drop landed under Improved and the
/// rise under Worsened; this test pins the corrected contract through the
/// binary.
#[test]
fn mi_family_direction_is_inverted_end_to_end() {
    let dir = TempDir::new().unwrap();
    let old = r#"version = 4
[[entry]]
path = "src/drop.rs"
qualified = "regressing"
start_line = 1
metric = "mi.original"
value = 70.0
[[entry]]
path = "src/rise.rs"
qualified = "improving"
start_line = 1
metric = "mi.original"
value = 50.0
"#;
    let new = r#"version = 4
[[entry]]
path = "src/drop.rs"
qualified = "regressing"
start_line = 1
metric = "mi.original"
value = 60.0
[[entry]]
path = "src/rise.rs"
qualified = "improving"
start_line = 1
metric = "mi.original"
value = 65.0
"#;
    fs::write(dir.path().join("old.toml"), old).unwrap();
    fs::write(dir.path().join("new.toml"), new).unwrap();
    let out = cli()
        .current_dir(dir.path())
        .args(["diff-baseline", "old.toml", "new.toml"])
        .assert()
        .success()
        // One worsened (the 70 -> 60 drop), one improved (the 50 -> 65
        // rise) — inverted relative to the numeric direction.
        .stdout(predicate::str::starts_with(
            "0 added, 0 removed, 1 worsened, 1 improved\n",
        ))
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).expect("utf8 stdout");
    // Assert section *membership*, not just substring presence: with the
    // #825 bug the counts stay "1 worsened, 1 improved" but the rows swap
    // sections, so a bare `contains("## Worsened")` + `contains("drop")`
    // would still pass. Split on the section headers and check each row
    // lands under the correct heading.
    let (worsened_block, improved_block) = stdout
        .split_once("## Improved")
        .expect("an Improved section is rendered");
    let worsened_block = worsened_block
        .split_once("## Worsened")
        .expect("a Worsened section is rendered")
        .1;
    assert!(
        worsened_block.contains("src/drop.rs::regressing")
            && worsened_block.contains("70 \u{2192} 60"),
        "the mi.* drop must render under Worsened, got:\n{stdout}"
    );
    assert!(
        !worsened_block.contains("src/rise.rs::improving"),
        "the mi.* rise must NOT render under Worsened, got:\n{stdout}"
    );
    assert!(
        improved_block.contains("src/rise.rs::improving")
            && improved_block.contains("50 \u{2192} 65"),
        "the mi.* rise must render under Improved, got:\n{stdout}"
    );
    assert!(
        !improved_block.contains("src/drop.rs::regressing"),
        "the mi.* drop must NOT render under Improved, got:\n{stdout}"
    );
}
