//! Integration tests for `[[thresholds.path]]` path-scoped threshold
//! overrides.
//!
//! The fixture tree puts the same C function under two directories and
//! beside a Rust twin that scores identically, so an outcome that
//! differs between files can only come from the path or the language,
//! never from the code. Every metric value asserted below was measured
//! with `bca check --threshold <m>=0` against these exact fixtures.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

mod common;

fn cli(dir: &Path) -> Command {
    common::cli_in(dir)
}

/// The offender rows from a `bca check` stdout stream (#1167 put them
/// there; the summary and remediation blocks stay on stderr). The
/// ` (limit ` filter is the shape assertion these tests depend on.
fn offenders(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter(|line| line.contains(" (limit "))
        .map(str::to_owned)
        .collect()
}

fn stdout_of(assert: &assert_cmd::assert::Assert) -> String {
    String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout")
}

fn stderr_of(assert: &assert_cmd::assert::Assert) -> String {
    String::from_utf8(assert.get_output().stderr.clone()).expect("utf8 stderr")
}

/// C: `cognitive = 6`, `cyclomatic = 7`.
const BRANCHY_C: &str = "int classify(int n) {
    if (n < 0) { return -1; }
    if (n == 0) { return 0; }
    if (n < 10) { return 1; }
    if (n < 100) { return 2; }
    if (n < 1000) { return 3; }
    if (n < 10000) { return 4; }
    return 5;
}
";

/// Rust: `cognitive = 6`, `cyclomatic = 7` — the same scores as
/// `BRANCHY_C`, so only the language and the path differ.
const BRANCHY_RUST: &str = "pub fn classify(n: i32) -> i32 {
    if n < 0 { return -1; }
    if n == 0 { return 0; }
    if n < 10 { return 1; }
    if n < 100 { return 2; }
    if n < 1000 { return 3; }
    if n < 10000 { return 4; }
    5
}
";

/// Elixir: the `Sample` module is a Container space with `nom = 4`.
const MODULE_ELIXIR: &str = "defmodule Sample do
  def one(a), do: a + 1
  def two(a), do: a + 2
  def three(a), do: a + 3
  def four(a), do: a + 4
end
";

/// Write the fixture tree — `src/branchy.c`, `src/gen/branchy.c`,
/// `src/branchy.rs`, `lib/branchy.c`, `src/sample.ex` — plus a
/// `bca.toml` carrying `manifest`, and return the directory.
fn tree(manifest: &str) -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    for (name, body) in [
        ("src/branchy.c", BRANCHY_C),
        ("src/gen/branchy.c", BRANCHY_C),
        ("src/branchy.rs", BRANCHY_RUST),
        ("lib/branchy.c", BRANCHY_C),
        ("src/sample.ex", MODULE_ELIXIR),
    ] {
        let path = dir.path().join(name);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, body).expect("write fixture");
    }
    fs::write(dir.path().join("bca.toml"), manifest).expect("write manifest");
    dir
}

/// An entry applies to the files its glob matches and to no other.
/// Both C files score `cyclomatic = 7`; only the directory differs.
#[test]
fn an_entry_re_limits_the_files_its_glob_matches_only() {
    let dir = tree(
        "paths = [\"src/branchy.c\", \"lib/branchy.c\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cyclomatic = 10\n",
    );

    let assert = cli(dir.path()).arg("check").assert().code(2);
    let offenders = offenders(&stdout_of(&assert));
    assert_eq!(offenders.len(), 1, "exactly one offender: {offenders:?}");
    assert!(
        offenders[0].contains("lib/branchy.c")
            && offenders[0].ends_with("cyclomatic = 7 (limit 5)"),
        "lib/ keeps the global limit; src/ is raised to 10: {offenders:?}"
    );
}

/// Where two entries match one file the *last* wins, per metric: the
/// later entry's `cyclomatic` replaces the earlier one's, while the
/// earlier entry's `cognitive` still applies because the later one does
/// not restate it.
#[test]
fn the_last_matching_entry_wins_per_metric() {
    let dir = tree(
        "paths = [\"src/gen/branchy.c\"]\n\
         [thresholds]\n\
         cyclomatic = 100\n\
         cognitive = 100\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cyclomatic = 10\n\
         cognitive = 4\n\
         [[thresholds.path]]\n\
         glob = \"src/gen/**\"\n\
         cyclomatic = 6\n",
    );

    let assert = cli(dir.path()).arg("check").assert().code(2);
    let offenders = offenders(&stdout_of(&assert));
    assert_eq!(offenders.len(), 2, "both metrics fire: {offenders:?}");
    assert!(
        offenders
            .iter()
            .any(|row| row.ends_with("classify: cyclomatic = 7 (limit 6)")),
        "cyclomatic takes the later entry's 6, not the earlier 10: {offenders:?}"
    );
    assert!(
        offenders
            .iter()
            .any(|row| row.ends_with("classify: cognitive = 6 (limit 4)")),
        "cognitive keeps the earlier entry's 4, which the later one did not restate: {offenders:?}"
    );
}

/// Declaration order decides, not glob specificity: the same two
/// entries written the other way round give the other answer.
#[test]
fn declaration_order_decides_not_specificity() {
    let dir = tree(
        "paths = [\"src/gen/branchy.c\"]\n\
         [thresholds]\n\
         cyclomatic = 100\n\
         [[thresholds.path]]\n\
         glob = \"src/gen/**\"\n\
         cyclomatic = 6\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cyclomatic = 10\n",
    );

    let assert = cli(dir.path()).arg("check").assert().success();
    assert!(
        offenders(&stdout_of(&assert)).is_empty(),
        "the broader entry is declared last and raises the limit to 10"
    );
}

/// A language table and a path entry compose in the stated order: the
/// path entry overrides what it names, and the language table supplies
/// what the entry does not — not the global table.
#[test]
fn a_language_table_and_a_path_entry_compose_in_order() {
    let dir = tree(
        "paths = [\"src/branchy.c\", \"src/branchy.rs\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n\
         cognitive = 5\n\
         [thresholds.lang.c]\n\
         cognitive = 100\n\
         cyclomatic = 100\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cyclomatic = 6\n",
    );

    let assert = cli(dir.path()).arg("check").assert().code(2);
    let offenders = offenders(&stdout_of(&assert));
    // C: cyclomatic from the path entry (6 < 7 fires), cognitive from
    // the language table (100, silent). Rust: cyclomatic from the path
    // entry (fires), cognitive from the global table (5 < 6, fires).
    assert_eq!(offenders.len(), 3, "{offenders:?}");
    assert!(
        offenders
            .iter()
            .any(|row| row.contains("branchy.c") && row.ends_with("cyclomatic = 7 (limit 6)")),
        "C's cyclomatic comes from the path entry: {offenders:?}"
    );
    assert!(
        !offenders
            .iter()
            .any(|row| row.contains("branchy.c") && row.contains("cognitive")),
        "C's cognitive comes from its language table, not the global 5: {offenders:?}"
    );
    assert!(
        offenders
            .iter()
            .any(|row| row.contains("branchy.rs") && row.ends_with("cognitive = 6 (limit 5)")),
        "Rust has no language table, so its cognitive is the global 5: {offenders:?}"
    );
}

/// `--threshold` is applied last and absolutely, so it outranks a path
/// entry too.
#[test]
fn cli_threshold_outranks_a_path_entry() {
    let dir = tree(
        "paths = [\"src/branchy.c\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cyclomatic = 100\n",
    );

    cli(dir.path())
        .args(["check", "--threshold", "cyclomatic=6"])
        .assert()
        .code(2)
        .stdout(predicate::str::contains(
            "classify: cyclomatic = 7 (limit 6)",
        ));
}

/// An unknown metric in an entry is a tool error (exit 1) naming the
/// entry by position and glob, with the did-you-mean hint.
#[test]
fn an_unknown_metric_names_the_entry() {
    let dir = tree(
        "paths = [\"src/branchy.c\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n\
         [[thresholds.path]]\n\
         glob = \"vendor/**\"\n\
         cyclomatic = 5\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cyclomatick = 10\n",
    );

    cli(dir.path())
        .arg("check")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "[[thresholds.path]] #2 (glob \"src/**\") in bca.toml: unknown threshold metric \
             \"cyclomatick\"",
        ))
        .stderr(predicate::str::contains("did you mean `cyclomatic`?"));
}

/// The three parse-time rejections, each naming the entry.
#[test]
fn an_empty_glob_a_missing_glob_and_tokens_are_rejected() {
    for (entry, expected) in [
        (
            "glob = \"./\"\ncyclomatic = 5\n",
            "[[thresholds.path]] #1 (glob \"./\"): the glob is empty",
        ),
        (
            "cyclomatic = 5\n",
            "[[thresholds.path]] #1: missing the `glob` key",
        ),
        (
            "glob = \"src/**\"\ntokens = 5000\n",
            "[[thresholds.path]] #1 (glob \"src/**\"): `tokens` cannot be overridden per path",
        ),
    ] {
        let dir = tree(&format!(
            "paths = [\"src/branchy.c\"]\n[thresholds]\ncyclomatic = 5\n[[thresholds.path]]\n{entry}"
        ));
        cli(dir.path())
            .arg("check")
            .assert()
            .code(1)
            .stderr(predicate::str::contains("bca.toml: "))
            .stderr(predicate::str::contains(expected));
    }
}

/// A metric only a path entry names is still computed and gated.
///
/// The check walk computes only the metric families its thresholds read
/// (#1113). Derive that selection from the global and language sets
/// alone and `nom` is never computed here, so the Elixir module's
/// `nom = 4` reads as the zero default and the gate passes silently.
/// The global limit is `cyclomatic`, whose dependency set is empty, for
/// the reason the sibling `[thresholds.lang]` test gives.
#[test]
fn a_metric_only_a_path_entry_gates_is_still_computed() {
    let dir = tree(
        "paths = [\"src/sample.ex\"]\n\
         [thresholds]\n\
         cyclomatic = 20\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         nom = 3\n",
    );

    let assert = cli(dir.path()).arg("check").assert().code(2);
    let offenders = offenders(&stdout_of(&assert));
    assert_eq!(offenders.len(), 1, "exactly one offender: {offenders:?}");
    assert!(
        offenders[0].ends_with("Sample: nom = 4 (limit 3)"),
        "nom must be computed although no global limit names it: {offenders:?}"
    );
}

/// The soft tier scales the *overridden* limit. With a global
/// `cognitive = 4`, an entry raising it to 10, and `--tier=soft=0.5`,
/// the file's soft band is 5: a function at cognitive 6 breaches that
/// band while sitting under the entry's hard limit — an encroachment,
/// exit 2, not the exit 5 it would be against the global ceiling of 4.
#[test]
fn soft_tier_scales_the_path_overridden_limit() {
    let dir = tree(
        "paths = [\"src/branchy.c\"]\n\
         [thresholds]\n\
         cognitive = 4\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cognitive = 10\n",
    );

    cli(dir.path())
        .args(["check", "--tier=soft=0.5", "--exit-codes=tiered"])
        .assert()
        .code(2)
        .stdout(predicate::str::contains(
            "classify: cognitive = 6 (limit 5)",
        ));
}

/// The manifest `[check] headroom` key — `BCA_HEADROOM`'s manifest
/// spelling, which `make self-scan-headroom` drives — is the same
/// ratio, so it scales the overridden limit the same way.
#[test]
fn manifest_headroom_scales_the_path_overridden_limit() {
    let dir = tree(
        "paths = [\"src/branchy.c\"]\n\
         [check]\n\
         headroom = 0.5\n\
         [thresholds]\n\
         cognitive = 4\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cognitive = 10\n",
    );

    cli(dir.path())
        .args(["check", "--tier=soft", "--exit-codes=tiered"])
        .assert()
        .code(2)
        .stdout(predicate::str::contains(
            "classify: cognitive = 6 (limit 5)",
        ));
}

/// The exit-5 ceiling is the overridden limit: a value past *it* is a
/// hard breach. Without this the test above would pass equally against
/// a build that never escalates.
#[test]
fn the_hard_ceiling_is_the_path_overridden_limit() {
    let dir = tree(
        "paths = [\"src/branchy.c\"]\n\
         [thresholds]\n\
         cognitive = 2\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cognitive = 5\n",
    );

    cli(dir.path())
        .args(["check", "--tier=soft=0.5", "--exit-codes=tiered"])
        .assert()
        .code(5)
        .stdout(predicate::str::contains(
            "classify: cognitive = 6 (limit 2.5)",
        ));
}

/// An absolute `[thresholds.soft]` entry applies to a path-overridden
/// metric as it applies to every table: it is the soft limit, and the
/// entry's hard value is the ceiling.
#[test]
fn an_absolute_soft_entry_applies_to_a_path_overridden_metric() {
    let dir = tree(
        "paths = [\"src/branchy.c\"]\n\
         [thresholds]\n\
         cognitive = 3\n\
         [thresholds.soft]\n\
         cognitive = 2\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cognitive = 10\n",
    );

    // cognitive 6: above the soft 2, under the entry's hard 10 — an
    // encroachment. Against the global ceiling of 3 it would be exit 5.
    cli(dir.path())
        .args(["check", "--tier=soft", "--exit-codes=tiered"])
        .assert()
        .code(2)
        .stdout(predicate::str::contains(
            "classify: cognitive = 6 (limit 2)",
        ));
}

/// And an absolute soft entry looser than an entry's hard limit is
/// rejected — before the walk, naming the entry — as it is for a
/// language table.
#[test]
fn a_soft_limit_looser_than_a_path_hard_limit_is_rejected_before_the_walk() {
    let dir = tree(
        "paths = [\"src/branchy.c\"]\n\
         [thresholds]\n\
         cognitive = 15\n\
         [thresholds.soft]\n\
         cognitive = 12\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cognitive = 4\n",
    );

    let assert = cli(dir.path())
        .args(["check", "--tier=soft"])
        .assert()
        .code(1);
    let stderr = stderr_of(&assert);
    assert!(
        stderr.contains("[[thresholds.path]] #1 (glob \"src/**\")")
            && stderr.contains("soft limit 12 is looser than the hard limit 4"),
        "{stderr}"
    );
    assert!(
        offenders(&stdout_of(&assert)).is_empty(),
        "rejected at resolution, not mid-walk"
    );
}

/// A scale-relative soft entry whose only hard base is a path entry
/// resolves against it rather than failing the run.
#[test]
fn scale_relative_soft_resolves_against_a_path_only_hard_limit() {
    let dir = tree(
        "paths = [\"src/sample.ex\"]\n\
         [thresholds]\n\
         cyclomatic = 20\n\
         [thresholds.soft]\n\
         nom = \"0.5x\"\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         nom = 6\n",
    );

    cli(dir.path())
        .args(["check", "--tier=soft"])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("Sample: nom = 4 (limit 3)"));
}

/// The glob is anchored at the manifest's directory, so it keeps
/// matching when the manifest is above the working directory and the
/// run is invoked from a subdirectory with a relative path.
#[test]
fn glob_anchors_at_the_manifest_above_a_subdirectory_invocation() {
    let dir = tree(
        "[thresholds]\n\
         cyclomatic = 5\n\
         [[thresholds.path]]\n\
         glob = \"src/gen/**\"\n\
         cyclomatic = 10\n",
    );

    let assert = cli(&dir.path().join("src"))
        .args(["check", "--paths", "gen/branchy.c", "--paths", "branchy.c"])
        .assert()
        .code(2);
    let offenders = offenders(&stdout_of(&assert));
    assert_eq!(offenders.len(), 1, "{offenders:?}");
    assert!(
        offenders[0].starts_with("branchy.c:")
            && offenders[0].ends_with("cyclomatic = 7 (limit 5)"),
        "src/branchy.c keeps the global limit; src/gen/branchy.c is raised, although spelled \
         `gen/branchy.c` from inside src/: {offenders:?}"
    );
}

/// An absolute file seed — the shape the per-edit agent hooks use —
/// still matches a manifest-relative glob.
#[test]
fn an_absolute_file_seed_matches_a_manifest_relative_glob() {
    let dir = tree(
        "[thresholds]\n\
         cyclomatic = 5\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cyclomatic = 10\n",
    );
    let seed = dir.path().join("src/branchy.c");

    cli(dir.path())
        .args(["check", "--paths", seed.to_str().expect("utf8 path")])
        .assert()
        .success()
        .stdout(predicate::str::contains("(limit").not());
}

/// A project reached through a symlinked root still matches: the
/// canonical cwd and the as-spelled manifest directory share no lexical
/// prefix, and the origin's canonical form is what rescues the match.
#[cfg(unix)]
#[test]
fn a_symlinked_project_root_still_matches() {
    let dir = tree(
        "paths = [\"src/branchy.c\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cyclomatic = 10\n",
    );
    let outer = TempDir::new().expect("tempdir");
    let link = outer.path().join("project");
    std::os::unix::fs::symlink(dir.path(), &link).expect("symlink");

    cli(&link)
        .arg("check")
        .assert()
        .success()
        .stdout(predicate::str::contains("(limit").not());
}

/// A seeded file that is a symlink into the project from outside it is
/// gated at the entry's limit: the link resolves to `project/src/…`,
/// which the `--config`'s `src/**` matches (#1746). The link's own
/// spelling, `<outside>/alias.c`, matches nothing.
#[cfg(unix)]
#[test]
fn a_seeded_symlink_into_the_project_is_gated_at_the_entry_s_limit() {
    let outer = TempDir::new().expect("tempdir");
    let project = outer.path().join("project");
    fs::create_dir_all(project.join("src")).expect("mkdir");
    fs::write(project.join("src/target.c"), BRANCHY_C).expect("write");
    fs::write(
        project.join("config.toml"),
        "[thresholds]\ncyclomatic = 5\n[[thresholds.path]]\nglob = \"src/**\"\ncyclomatic = 10\n",
    )
    .expect("write config");
    std::os::unix::fs::symlink(project.join("src/target.c"), outer.path().join("alias.c"))
        .expect("symlink");

    cli(outer.path())
        .args(["check", "-p", "alias.c", "--config", "project/config.toml"])
        .assert()
        .success()
        .stdout(predicate::str::contains("(limit").not());
}

/// A non-UTF-8 path is matched by bytes; nothing in the lookup needs a
/// string. Unix-only: there is no stable byte→path view elsewhere.
#[cfg(unix)]
#[test]
fn a_non_utf8_path_is_matched() {
    use std::os::unix::ffi::OsStrExt;

    let dir = tree(
        "[thresholds]\n\
         cyclomatic = 5\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cyclomatic = 10\n",
    );
    let mut name = std::ffi::OsString::from("bad");
    name.push(std::ffi::OsStr::from_bytes(b"\x80"));
    name.push(".c");
    let bad = dir.path().join("src").join(&name);
    if fs::write(&bad, BRANCHY_C).is_err() {
        // Some filesystems (macOS APFS) reject non-UTF-8 names outright.
        return;
    }

    cli(dir.path())
        .arg("check")
        .arg("--paths")
        .arg(&bad)
        .assert()
        .success()
        .stdout(predicate::str::contains("(limit").not());
}

/// A file outside the manifest's tree has no manifest-relative form and
/// is matched by its seed-relative form instead — here a file seed, so
/// the path as the caller spelled it — so a glob that would match that
/// spelling still applies, and one written for the project tree does
/// not. The directory-seed shape is the test after the `--config` ones.
#[test]
fn a_file_outside_the_project_is_matched_by_its_seed_form() {
    let dir = tree(
        "[thresholds]\n\
         cyclomatic = 5\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cyclomatic = 10\n\
         [[thresholds.path]]\n\
         glob = \"**/elsewhere/*.c\"\n\
         cognitive = 100\n",
    );
    let outside = TempDir::new().expect("tempdir");
    let elsewhere = outside.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).expect("mkdir");
    fs::write(elsewhere.join("src_branchy.c"), BRANCHY_C).expect("write");
    let seed = elsewhere.join("src_branchy.c");

    let assert = cli(dir.path())
        .args([
            "check",
            "--paths",
            seed.to_str().expect("utf8 path"),
            "--threshold",
            "cognitive=100",
        ])
        .assert()
        .code(2);
    let offenders = offenders(&stdout_of(&assert));
    assert_eq!(offenders.len(), 1, "{offenders:?}");
    assert!(
        offenders[0].ends_with("cyclomatic = 7 (limit 5)"),
        "`src/**` is a project glob and does not match a file outside the project: {offenders:?}"
    );
}

/// The outside-origin fallback is the *seed-relative* form, exactly
/// what a `[check] exclude` glob matches — not the file's path from
/// the working directory. The test above passes with a file seed,
/// whose seed-relative form is its cwd-relative path; this one seeds
/// the directory, so the walk anchors each file at `src/` and an
/// outside-origin `src/**` sees `gen/branchy.c`, never `src/gen/…`.
#[test]
fn a_file_outside_the_origin_is_matched_by_its_seed_relative_form_not_its_cwd_path() {
    let dir = tree(
        "paths = [\"src\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n",
    );
    let elsewhere = TempDir::new().expect("tempdir");
    let overlay = elsewhere.path().join("overlay.toml");
    fs::write(
        &overlay,
        "[[thresholds.path]]\nglob = \"src/**\"\ncyclomatic = 100\n\
         [[thresholds.path]]\nglob = \"gen/**\"\ncyclomatic = 100\n",
    )
    .expect("write overlay");

    let assert = cli(dir.path())
        .args(["check", "--config", overlay.to_str().expect("utf8 path")])
        .assert()
        .code(2);
    let offenders = offenders(&stdout_of(&assert));
    // `src/**` matches nothing: under the `src` seed every file's match
    // form starts at `src/`'s children. `gen/**` matches `gen/branchy.c`
    // by that same form, so only `src/branchy.c` and `src/branchy.rs`
    // fire.
    assert_eq!(offenders.len(), 2, "{offenders:?}");
    assert!(
        offenders.iter().all(|row| !row.contains("gen/")),
        "src/gen/branchy.c is re-limited through its seed form `gen/branchy.c`: {offenders:?}"
    );
}

/// A `--config` entry's glob anchors at the config file's own
/// directory, not the manifest's or the working directory: an overlay
/// in `sub/` with `gen/**` reaches `sub/gen/x.c` only under that anchor.
#[test]
fn a_config_entry_anchors_at_the_config_file_s_directory() {
    let dir = tree(
        "paths = [\"src/gen/branchy.c\", \"src/branchy.c\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n",
    );
    let overlay = dir.path().join("src/overlay.toml");
    fs::write(
        &overlay,
        "[[thresholds.path]]\nglob = \"gen/**\"\ncyclomatic = 100\n",
    )
    .expect("write overlay");

    let assert = cli(dir.path())
        .args(["check", "--config", "src/overlay.toml"])
        .assert()
        .code(2);
    let offenders = offenders(&stdout_of(&assert));
    assert_eq!(offenders.len(), 1, "{offenders:?}");
    assert!(
        offenders[0].contains("src/branchy.c") && !offenders[0].contains("gen/"),
        "`gen/**` anchored at src/ reaches src/gen/branchy.c and not src/branchy.c; anchored \
         at the project root or the cwd it would reach neither: {offenders:?}"
    );
}

/// A `--config` entry is named by its position in its *own* file and
/// by that file, not by its position in the merged list after the
/// manifest's entries.
#[test]
fn a_config_entry_s_diagnostic_names_its_own_file_and_ordinal() {
    let dir = tree(
        "paths = [\"src/branchy.c\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n\
         [[thresholds.path]]\n\
         glob = \"vendor/**\"\n\
         cyclomatic = 5\n\
         [[thresholds.path]]\n\
         glob = \"lib/**\"\n\
         cyclomatic = 5\n",
    );
    let elsewhere = TempDir::new().expect("tempdir");
    let overlay = elsewhere.path().join("overlay.toml");
    fs::write(
        &overlay,
        "[[thresholds.path]]\nglob = \"src/**\"\ncyclomatick = 10\n",
    )
    .expect("write overlay");
    let overlay = overlay.to_str().expect("utf8 path");

    cli(dir.path())
        .args(["check", "--config", overlay])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(format!(
            "[[thresholds.path]] #1 (glob \"src/**\") in {overlay}: unknown threshold metric"
        )));
}

/// A `--config` entry rejected at *parse* time names its file too, as
/// the build-time error above does and as the manifest's errors carry
/// `bca.toml:` — the two surfaces must not differ on which file (#1746).
#[test]
fn a_config_entry_s_parse_error_names_its_file() {
    let dir = tree(
        "paths = [\"src/branchy.c\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n",
    );
    let elsewhere = TempDir::new().expect("tempdir");
    let overlay = elsewhere.path().join("overlay.toml");
    fs::write(
        &overlay,
        "[[thresholds.path]]\nglob = \"src/**\"\ncyclomatic = \"oops\"\n",
    )
    .expect("write overlay");
    let overlay = overlay.to_str().expect("utf8 path");

    cli(dir.path())
        .args(["check", "--config", overlay])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(format!(
            "error: {overlay}: [[thresholds.path]] #1 (glob \"src/**\") \"cyclomatic\": expected \
             a number, got string"
        )));
}

/// `--config` entries carry their own file's directory as the anchor,
/// and are appended after the manifest's so they win on overlap.
#[test]
fn config_entries_anchor_at_the_config_file_and_follow_the_manifest_entries() {
    let dir = tree(
        "paths = [\"src/branchy.c\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cyclomatic = 10\n",
    );
    let elsewhere = TempDir::new().expect("tempdir");
    // Relative to the config file's own directory this glob names
    // nothing in the project; the entry is inert unless it is matched
    // against the seed form of a file *outside* that directory — which
    // every project file is.
    let overlay = elsewhere.path().join("overlay.toml");
    fs::write(
        &overlay,
        "[[thresholds.path]]\nglob = \"src/**\"\ncyclomatic = 6\n",
    )
    .expect("write overlay");

    cli(dir.path())
        .args(["check", "--config", overlay.to_str().expect("utf8 path")])
        .assert()
        .code(2)
        .stdout(predicate::str::contains(
            "classify: cyclomatic = 7 (limit 6)",
        ));
}

/// Gating only by path, with no global `[thresholds]`, is legal and
/// says so out loud.
#[test]
fn a_path_only_manifest_warns_that_nothing_else_is_gated() {
    let dir = tree(
        "paths = [\"src/branchy.c\", \"lib/branchy.c\"]\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cyclomatic = 100\n",
    );

    let assert = cli(dir.path()).arg("check").assert().success();
    let stderr = stderr_of(&assert);
    assert!(
        stderr.contains(
            "no global [thresholds] table: only the files matching a [[thresholds.path]] glob \
             are gated"
        ),
        "{stderr}"
    );
    let offenders = offenders(&stdout_of(&assert));
    assert!(
        offenders.is_empty(),
        "nothing breaches a limit of 100: {offenders:?}"
    );
}

/// `--print-effective-config` prints every entry's declared metrics —
/// at their resolved limit, after the per-language tables — and the
/// printed file reads back through `--config` to the same gate result.
#[test]
fn print_effective_config_shows_the_path_entries_and_round_trips() {
    let dir = tree(
        "paths = [\"src/branchy.c\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n\
         [thresholds.lang.c]\n\
         cognitive = 30\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cyclomatic = 6\n\
         ploc = 900\n\
         [[thresholds.path]]\n\
         glob = \"./src/gen/**\"\n\
         \"halstead.effort\" = 150000\n",
    );

    let assert = cli(dir.path())
        .args(["check", "--print-effective-config", "toml"])
        .assert()
        .success();
    let printed = stdout_of(&assert);
    let parsed: toml::Table = toml::from_str(&printed).expect("effective config is valid TOML");
    let thresholds = parsed["thresholds"]
        .as_table()
        .expect("[thresholds] is a table");
    assert_eq!(thresholds["cyclomatic"].as_float(), Some(5.0));
    assert_eq!(thresholds["lang"]["c"]["cognitive"].as_float(), Some(30.0));
    let path = thresholds["path"]
        .as_array()
        .expect("[[thresholds.path]] is an array");
    assert_eq!(path.len(), 2);
    assert_eq!(path[0]["glob"].as_str(), Some("src/**"));
    assert_eq!(path[0]["cyclomatic"].as_float(), Some(6.0));
    assert_eq!(
        path[0]["loc.ploc"].as_float(),
        Some(900.0),
        "the alias is printed under its canonical id"
    );
    assert_eq!(
        path[0].as_table().map(toml::Table::len),
        Some(3),
        "the glob and the two metrics the entry declares, nothing inherited: {path:?}"
    );
    assert_eq!(
        path[1]["glob"].as_str(),
        Some("./src/gen/**"),
        "echoed as written"
    );
    assert_eq!(path[1]["halstead.effort"].as_float(), Some(150_000.0));

    let json = cli(dir.path())
        .args(["check", "--print-effective-config", "json"])
        .assert()
        .success();
    let json: serde_json::Value =
        serde_json::from_str(&stdout_of(&json)).expect("effective config is valid JSON");
    assert_eq!(json["thresholds"]["path"][1]["glob"], "./src/gen/**");

    // Written into the project so the printed globs keep their anchor.
    // Both runs name the fixture the same way so the offender lines are
    // comparable verbatim: the manifest's own `paths` resolve against
    // the manifest directory and come back absolute.
    let echoed = dir.path().join("effective.toml");
    fs::write(&echoed, &printed).expect("write effective config");
    let direct = cli(dir.path())
        .args(["check", "--paths", "src/branchy.c"])
        .assert()
        .code(2);
    let round_tripped = cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            "src/branchy.c",
            "--config",
            echoed.to_str().expect("utf8 path"),
        ])
        .assert()
        .code(2);
    let direct = stdout_of(&direct);
    let direct = offenders(&direct);
    assert_eq!(direct.len(), 1, "{direct:?}");
    assert!(
        direct[0].ends_with("cyclomatic = 7 (limit 6)"),
        "{direct:?}"
    );
    let round_tripped = stdout_of(&round_tripped);
    assert_eq!(
        direct,
        offenders(&round_tripped),
        "the printed config must gate identically to the config it was printed from"
    );
}

/// Print the effective config of `check <args>` in `dir`, write it into
/// the project, and return the path.
fn print_effective(dir: &Path, args: &[&str]) -> std::path::PathBuf {
    let printed = cli(dir)
        .arg("check")
        .args(args)
        .args(["--print-effective-config", "toml"])
        .assert()
        .success();
    let echoed = dir.join("effective.toml");
    fs::write(&echoed, stdout_of(&printed)).expect("write effective config");
    echoed
}

/// The round trip holds with a `--threshold` override in effect (#1746).
///
/// The global row prints the CLI value, so an entry printed *as
/// declared* beside it read back with the entry's declared limit over
/// the CLI one: direct run exit 2 against `cyclomatic = 3`, printed
/// config loaded back exit 0 against the entry's 10. The entry's
/// metrics now print at the limit they resolve to.
#[test]
fn print_effective_config_round_trips_a_cli_override_over_a_path_entry() {
    let dir = tree(
        "paths = [\"src/branchy.c\"]\n\
         [thresholds]\n\
         cyclomatic = 2\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cyclomatic = 10\n",
    );
    let override_ = ["--threshold", "cyclomatic=3"];
    let echoed = print_effective(dir.path(), &override_);
    let printed: toml::Table =
        toml::from_str(&fs::read_to_string(&echoed).expect("read")).expect("valid TOML");
    assert_eq!(
        printed["thresholds"]["path"][0]["cyclomatic"].as_float(),
        Some(3.0),
        "the entry's cyclomatic prints at the CLI value, not its declared 10"
    );

    let direct = cli(dir.path())
        .arg("check")
        .args(override_)
        .args(["--paths", "src/branchy.c"])
        .assert()
        .code(2);
    let round_tripped = cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            "src/branchy.c",
            "--config",
        ])
        .arg(&echoed)
        .assert()
        .code(2);
    let direct = offenders(&stdout_of(&direct));
    assert_eq!(direct.len(), 1, "{direct:?}");
    assert!(
        direct[0].ends_with("cyclomatic = 7 (limit 3)"),
        "{direct:?}"
    );
    assert_eq!(direct, offenders(&stdout_of(&round_tripped)));
}

/// The same reversal at the soft tier with tiered exit codes: an entry
/// raising `cognitive` to 10 over a global 2 and a `--threshold
/// cognitive=3` makes cognitive 6 a hard breach (exit 5); the declared
/// entry read back put it under the entry's 10 (exit 0).
///
/// The path row prints the hard limit the band derives from, so the
/// read-back run passes the *same* `--tier` the original did. The exit
/// code is the contract here: the CLI's absolute 3 is a table value in
/// the printout, so the read-back scales it to a 1.5 band under a
/// ceiling of 3 exactly as it scales the global row — the same
/// function is still past the ceiling, exit 5.
#[test]
fn print_effective_config_round_trips_a_cli_override_at_the_soft_tier() {
    let dir = tree(
        "paths = [\"src/branchy.c\"]\n\
         [thresholds]\n\
         cognitive = 2\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cognitive = 10\n",
    );
    let flags = [
        "--threshold",
        "cognitive=3",
        "--tier=soft=0.5",
        "--exit-codes=tiered",
    ];
    let echoed = print_effective(dir.path(), &flags);

    let direct = cli(dir.path())
        .arg("check")
        .args(flags)
        .args(["--paths", "src/branchy.c"])
        .assert()
        .code(5);
    let round_tripped = cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            "src/branchy.c",
            "--config",
        ])
        .arg(&echoed)
        .args(["--tier=soft=0.5", "--exit-codes=tiered"])
        .assert()
        .code(5);
    let direct = offenders(&stdout_of(&direct));
    assert_eq!(direct.len(), 1, "{direct:?}");
    assert!(direct[0].ends_with("cognitive = 6 (limit 3)"), "{direct:?}");
    let round_tripped = offenders(&stdout_of(&round_tripped));
    assert_eq!(round_tripped.len(), 1, "{round_tripped:?}");
    assert!(
        round_tripped[0].ends_with("cognitive = 6 (limit 1.5)"),
        "{round_tripped:?}"
    );
}

/// Under `--tier=soft=R` a path row prints its *hard* limit — the
/// number the soft band derives from — beside the scaled global row,
/// so loading the printout with the same `--tier` replays the gate.
///
/// A row printed at the band could not: with the entry at `7.5` and
/// `--tier=soft=0.9` the run warns at `6.75` under a ceiling of `7.5`
/// (cyclomatic 7 is an encroachment, exit 2); a row printed as `6.75`
/// read back under the same tier is scaled again to `6.075` and
/// ceilinged at `6.75`, which makes the same function a hard breach
/// (exit 5), and no `--tier` on the read-back side recovers the
/// ceiling the printout dropped (#1746).
#[test]
fn print_effective_config_prints_path_rows_at_the_hard_tier_and_replays_under_the_same_tier() {
    let dir = tree(
        "paths = [\"src/branchy.c\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cyclomatic = 7.5\n",
    );
    let flags = ["--tier=soft=0.9", "--exit-codes=tiered"];
    let echoed = print_effective(dir.path(), &flags);
    let printed: toml::Table =
        toml::from_str(&fs::read_to_string(&echoed).expect("read")).expect("valid TOML");
    assert_eq!(printed["thresholds"]["cyclomatic"].as_float(), Some(4.5));
    assert_eq!(
        printed["thresholds"]["path"][0]["cyclomatic"].as_float(),
        Some(7.5),
        "the entry's cyclomatic prints at the hard limit, not the 6.75 band"
    );

    let direct = cli(dir.path())
        .arg("check")
        .args(flags)
        .args(["--paths", "src/branchy.c"])
        .assert()
        .code(2);
    let round_tripped = cli(dir.path())
        .args([
            "check",
            "--no-config",
            "--paths",
            "src/branchy.c",
            "--config",
        ])
        .arg(&echoed)
        .args(flags)
        .assert()
        .code(2);
    let direct = offenders(&stdout_of(&direct));
    assert_eq!(direct.len(), 1, "{direct:?}");
    assert!(
        direct[0].ends_with("cyclomatic = 7 (limit 6.75)"),
        "{direct:?}"
    );
    assert_eq!(direct, offenders(&stdout_of(&round_tripped)));
}

/// The same replay where the soft band is a property of the *match
/// set*, which no per-entry row could carry: a second entry naming only
/// `cognitive` puts the file's fold into `[thresholds.soft]` merge mode,
/// so its `cyclomatic` keeps the first entry's hard `7.5` with no band
/// at all (cyclomatic 7 passes, exit 0), while that entry alone would
/// resolve to a `6.75` band. Both rows print their hard values and the
/// read-back run re-derives the merge mode itself (#1746).
#[test]
fn print_effective_config_replays_an_overlap_that_switches_the_soft_mode() {
    let dir = tree(
        "paths = [\"src/branchy.c\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n\
         [thresholds.soft]\n\
         cognitive = \"0.9x\"\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cyclomatic = 7.5\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cognitive = 50\n",
    );
    let flags = ["--tier=soft=0.9", "--exit-codes=tiered"];
    let echoed = print_effective(dir.path(), &flags);
    let printed: toml::Table =
        toml::from_str(&fs::read_to_string(&echoed).expect("read")).expect("valid TOML");
    assert_eq!(
        printed["thresholds"]["path"][0]["cyclomatic"].as_float(),
        Some(7.5)
    );
    assert_eq!(
        printed["thresholds"]["path"][1]["cognitive"].as_float(),
        Some(50.0)
    );

    cli(dir.path())
        .arg("check")
        .args(flags)
        .args(["--paths", "src/branchy.c"])
        .assert()
        .success()
        .stdout(predicate::str::contains("(limit").not());
    // Over the manifest rather than `--no-config`: the printout is the
    // resolved view and carries no `[thresholds.soft]` table of its own,
    // so the merge mode the run was in comes from the manifest that is
    // still loaded, with the printed rows layered on top. A row printed
    // at the `6.75` band read back this way was the ceiling too, and
    // exit 5.
    cli(dir.path())
        .args(["check", "--paths", "src/branchy.c", "--config"])
        .arg(&echoed)
        .args(flags)
        .assert()
        .success()
        .stdout(predicate::str::contains("(limit").not());
}

/// `--explain-threshold` names each entry that declares the explained
/// metric, so the reported counts are read against the right
/// population. It reports the *declaration*, not a limit: entries
/// overlap, and a file two of them match is gated at the later one's
/// value — so the earlier entry's number is not what its files are
/// counted against, and a per-entry soft limit would be a claim the
/// overlap contradicts.
#[test]
fn explain_threshold_reports_the_path_entries_declaring_the_metric() {
    let dir = tree(
        "paths = [\"src/branchy.c\", \"lib/branchy.c\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cyclomatic = 10\n\
         [[thresholds.path]]\n\
         glob = \"vendor/**\"\n\
         cognitive = 50\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cyclomatic = 6\n",
    );

    let assert = cli(dir.path())
        .args(["check", "--explain-threshold", "cyclomatic=8"])
        .assert()
        .success();
    let stdout = stdout_of(&assert);
    for expected in [
        "note: [[thresholds.path]] #1 (glob \"src/**\") in bca.toml declares cyclomatic = 10; \
         a file it matches is gated at the last matching entry's value, not the candidate",
        "note: [[thresholds.path]] #3 (glob \"src/**\") in bca.toml declares cyclomatic = 6; \
         a file it matches is gated at the last matching entry's value, not the candidate",
    ] {
        assert!(stdout.contains(expected), "{stdout}");
    }
    assert!(
        !stdout.contains("vendor/**"),
        "an entry naming only other metrics earns no line: {stdout}"
    );
    assert!(
        !stdout.contains("keeps cyclomatic") && !stdout.contains("(soft "),
        "no per-entry limit is claimed: {stdout}"
    );
    // src/branchy.c is gated at 6 (entry #3, the last match), not the
    // 10 entry #1 declares: the gate fires on it at 6, and the preview
    // — which counts each file against the limit its own match set
    // resolved to — reports it as the one hard offender, while
    // lib/branchy.c sits under the candidate 8. A report that priced
    // entry #1's 10 as the limit for `src/**` would have counted none.
    let direct = cli(dir.path()).arg("check").assert().code(2);
    let offenders = offenders(&stdout_of(&direct));
    assert!(
        offenders
            .iter()
            .any(|row| row.contains("src/branchy.c") && row.ends_with("cyclomatic = 7 (limit 6)")),
        "{offenders:?}"
    );
    assert!(
        stdout.contains("hard tier (limit 8): 1 offenders"),
        "{stdout}"
    );
}

/// A second entry naming only *another* metric can still change what
/// the first entry's files are gated at: with a scale-relative
/// `[thresholds.soft] cognitive` the second entry's `cognitive` puts
/// the file's fold into merge mode, so its `cyclomatic` keeps the first
/// entry's hard 10 with no band, where the first entry alone resolves
/// to a ratio band. The report must not claim either band for
/// `cyclomatic`: each entry's line is its declaration, the same
/// whichever other entries overlap it, and the cognitive-only entry
/// earns no cyclomatic line at all.
#[test]
fn explain_threshold_declaration_lines_do_not_depend_on_overlapping_entries() {
    let dir = tree(
        "paths = [\"src/branchy.c\"]\n\
         [thresholds]\n\
         cyclomatic = 5\n\
         cognitive = 5\n\
         [thresholds.soft]\n\
         cognitive = \"0.9x\"\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cyclomatic = 10\n\
         [[thresholds.path]]\n\
         glob = \"src/**\"\n\
         cognitive = 50\n",
    );

    let assert = cli(dir.path())
        .args(["check", "--explain-threshold", "cyclomatic=6"])
        .assert()
        .success();
    let stdout = stdout_of(&assert);
    assert!(
        stdout.contains(
            "note: [[thresholds.path]] #1 (glob \"src/**\") in bca.toml declares \
             cyclomatic = 10; a file it matches is gated at the last matching entry's value, \
             not the candidate"
        ),
        "{stdout}"
    );
    assert!(
        !stdout.contains("[[thresholds.path]] #2"),
        "the cognitive-only entry earns no cyclomatic line: {stdout}"
    );
    assert!(
        !stdout.contains("(soft ") && !stdout.contains("keeps cyclomatic"),
        "no per-entry band is claimed for cyclomatic: {stdout}"
    );

    let assert = cli(dir.path())
        .args(["check", "--explain-threshold", "cognitive=4"])
        .assert()
        .success();
    let stdout = stdout_of(&assert);
    assert!(
        stdout.contains(
            "note: [[thresholds.path]] #2 (glob \"src/**\") in bca.toml declares \
             cognitive = 50; a file it matches is gated at the last matching entry's value, \
             not the candidate"
        ),
        "{stdout}"
    );
    assert!(
        !stdout.contains("[[thresholds.path]] #1"),
        "the cyclomatic-only entry earns no cognitive line: {stdout}"
    );
}
