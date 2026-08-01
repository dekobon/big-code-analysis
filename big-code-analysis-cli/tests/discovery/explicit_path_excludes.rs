//! Integration tests for the explicitly-named-path exclude rule (#1146).
//!
//! A path named directly on the command line overrides the walker's
//! deny-set — `--exclude`, `--exclude-from`, `.bcaignore`, and a
//! manifest `exclude` list alike — matching the ripgrep convention that
//! an explicit path is a direct request. That is deliberate, but it used
//! to be silent, so any caller that names paths one at a time (the
//! shipped per-edit agent hooks) reported offenders in files the project
//! had put out of scope.
//!
//! Three contracts are pinned here, and the asymmetry between them is
//! the point — a future change must not quietly unify them:
//!
//! | surface | overridden by an explicit path? |
//! | --- | --- |
//! | `exclude` / `exclude_from` / `.bcaignore` | yes, with a warning |
//! | `-I` / `--include` | no |
//! | `[check] exclude` | no |

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

use crate::common;

fn cli(dir: &Path) -> Command {
    common::cli_in(dir)
}

/// Cyclomatic == 4 (three decision points plus one), so a
/// `--threshold cyclomatic=1` run always finds it. The function name
/// embeds its file so an assertion can say *which* offender survived.
fn branchy(fn_name: &str) -> String {
    format!(
        "pub fn {fn_name}(n: i32) -> i32 {{ if n < 0 {{ 1 }} else if n == 0 {{ 2 }} \
         else if n < 10 {{ 3 }} else {{ 4 }} }}\n"
    )
}

/// Two offenders — `skipme/a.rs`, which every exclude below covers, and
/// `kept.rs`, which none of them do — plus a `bca.toml` carrying
/// `exclude_body` (an `exclude` key for the walker deny-set, a
/// `[check] exclude` table for the gate-exemption set, or nothing).
///
/// `kept.rs` exists so the directory-walk case can assert a *positive*:
/// without a surviving offender, "the excluded file is absent" is
/// indistinguishable from a run that found nothing at all. The `.git`
/// marker halts manifest discovery here rather than at the repo root
/// (#491).
fn fixture(exclude_body: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::create_dir(dir.path().join("skipme")).unwrap();
    fs::write(
        dir.path().join("skipme").join("a.rs"),
        branchy("skipme_offender"),
    )
    .unwrap();
    fs::write(dir.path().join("kept.rs"), branchy("kept_offender")).unwrap();
    fs::write(
        dir.path().join("bca.toml"),
        format!("paths = [\".\"]\n{exclude_body}\n[thresholds]\ncyclomatic = 1\n"),
    )
    .unwrap();
    dir
}

/// The walker deny-set does not reach a path the caller named, so the
/// offender is still reported — and the override is announced on stderr
/// naming the exact glob it overrode, in the glob's configured spelling
/// so the reader can find the line to edit.
#[test]
fn explicit_path_overrides_manifest_exclude_and_warns_naming_the_glob() {
    let dir = fixture("exclude = [\"./skipme/**\"]\n");

    cli(dir.path())
        .args(["check", "skipme/a.rs", "--no-summary", "--no-remediation"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("skipme_offender"))
        .stderr(predicate::str::contains(
            "bca: warning: skipme/a.rs matches an exclude pattern (./skipme/**) \
             but was named explicitly; analyzing anyway",
        ));
}

/// The counterpart: the same manifest, reached through the *walk*,
/// excludes the file outright. Without this the test above could pass
/// against a build where `exclude` did nothing at all.
///
/// `kept_offender` is the positive half. Asserting only the absence of
/// `skipme_offender` would hold for a run that resolved no files, or one
/// that failed before analysing anything.
#[test]
fn same_manifest_exclude_still_drops_the_file_on_a_directory_walk() {
    let dir = fixture("exclude = [\"./skipme/**\"]\n");

    cli(dir.path())
        .args(["check", "--no-summary", "--no-remediation"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("kept_offender"))
        .stderr(predicate::str::contains("skipme_offender").not())
        // No override happened, so no warning — the diagnostic must not
        // fire for files the walk selected.
        .stderr(predicate::str::contains("named explicitly").not());
}

/// The `-X` / `--exclude` CLI surface behaves as the manifest key does,
/// and the reported glob is the *matching* pattern rather than whichever
/// one happens to sit first.
///
/// The pattern list is deliberately shaped so every wrong lookup names
/// a *different* glob, which no single-pattern fixture can detect:
///
/// | index | caller's list | compiled deny-set |
/// | --- | --- | --- |
/// | 0 | `./` (drops — empty once normalised) | `kept.rs` |
/// | 1 | `kept.rs` | `./skipme/**` |
/// | 2 | `./skipme/**` | — |
///
/// The match is at deny-set index 1. Reading the caller's original list
/// at that index yields `kept.rs` — the off-by-one-per-dropped-pattern
/// misattribution `mk_globset_retaining` exists to prevent — and simply
/// taking the first configured pattern yields `kept.rs` too.
#[test]
fn override_warning_names_the_matching_glob_not_a_neighbour() {
    let dir = fixture("");

    cli(dir.path())
        .args([
            "check",
            "skipme/a.rs",
            "-X",
            "./",
            "-X",
            "kept.rs",
            "-X",
            "./skipme/**",
            "--no-summary",
            "--no-remediation",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "matches an exclude pattern (./skipme/**)",
        ));
}

/// The third walker surface: a `.bcaignore`-style file reached through
/// `--exclude-from`. It unions into the same deny-set, so it must warn
/// identically — this is the surface a project's ignore rules actually
/// live in.
#[test]
fn explicit_path_overrides_exclude_from_file_and_warns() {
    let dir = fixture("");
    fs::write(dir.path().join(".bcaignore"), "# ignored\n\n./skipme/**\n").unwrap();

    cli(dir.path())
        .args([
            "check",
            "skipme/a.rs",
            "--exclude-from",
            ".bcaignore",
            "--no-summary",
            "--no-remediation",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("skipme_offender"))
        .stderr(predicate::str::contains(
            "matches an exclude pattern (./skipme/**)",
        ));
}

/// A named file no language claims produces no analysis, so calling it
/// "analyzed anyway" would be both noisy and false. The advertised
/// `git diff --name-only | bca metrics --paths-from -` pipeline feeds in
/// whole changesets, where such files are the majority.
///
/// The `.rs` sibling in the same excluded directory *does* warn in the
/// same run, so a build that simply stopped warning altogether fails
/// here rather than passing.
#[test]
fn override_warning_is_silent_for_a_seed_no_language_claims() {
    let dir = fixture("exclude = [\"./skipme/**\"]\n");
    fs::write(dir.path().join("skipme").join("notes.md"), "# notes\n").unwrap();

    cli(dir.path())
        .args([
            "metrics",
            "skipme/notes.md",
            "skipme/a.rs",
            "--format",
            "json",
            "--output",
            dir.path().join("out.json").to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("skipme/a.rs matches an exclude"))
        .stderr(predicate::str::contains("notes.md matches an exclude").not());
}

/// The seed's language must be resolved through the library's own
/// `get_language_for_file`, ASCII case-fold included (#1111). A local
/// `extension()` + `get_from_ext` pair — what this shipped as — skips
/// that fold, so `SKIPME/A.RS` resolved to no language, the guard above
/// returned early, and the file was analyzed with the override left
/// silent: the exact asymmetry #1146 removes, surviving for every
/// mixed-case extension.
///
/// Both spellings are named in one invocation. The lowercase half is the
/// control: a build that stopped warning altogether fails on it instead
/// of passing this test for the wrong reason.
#[test]
fn override_warning_survives_a_mixed_case_extension() {
    let dir = fixture("exclude = [\"./skipme/**\"]\n");
    fs::write(
        dir.path().join("skipme").join("B.RS"),
        branchy("upper_offender"),
    )
    .unwrap();

    cli(dir.path())
        .args([
            "check",
            "skipme/B.RS",
            "skipme/a.rs",
            "--no-summary",
            "--no-remediation",
        ])
        .assert()
        .code(2)
        // Reported as an offender, so the file really was analyzed and
        // the unannounced override was a live one.
        .stderr(predicate::str::contains("upper_offender"))
        .stderr(predicate::str::contains(
            "bca: warning: skipme/B.RS matches an exclude pattern (./skipme/**) \
             but was named explicitly; analyzing anyway",
        ))
        .stderr(predicate::str::contains("skipme/a.rs matches an exclude"));
}

/// `[check] exclude` is the surface that survives an explicit path: the
/// file is analysed, its violation is dropped, and the existing
/// `[check.exclude]` skip line reports the drop. Pinned deliberately
/// against the case above so a future change cannot unify the two
/// exclude surfaces without a test failing.
#[test]
fn explicit_path_does_not_override_check_exclude() {
    let dir = fixture("[check]\nexclude = [\"./skipme/**\"]\n");

    cli(dir.path())
        .args(["check", "skipme/a.rs", "--no-summary", "--no-remediation"])
        .assert()
        .success()
        .stderr(predicate::str::contains("skipme_offender").not())
        // The skip line proves the offender existed and was dropped;
        // without it a clean exit could mean the fixture stopped
        // offending.
        .stderr(predicate::str::contains(
            "skipped 1 violations via [check.exclude]",
        ))
        // ... and no override warning, because nothing was overridden.
        .stderr(predicate::str::contains("named explicitly").not());
}

/// The same, with the path spelled absolutely — the shape both shipped
/// agent hooks use, and the one that failed before #1146: an explicit
/// file seed is its own only seed, so the violation path reached the
/// `[check.exclude]` filter unanchored and a `./`-anchored glob never
/// matched it.
#[test]
fn absolute_explicit_path_does_not_override_check_exclude() {
    let dir = fixture("[check]\nexclude = [\"./skipme/**\"]\n");
    let abs = dir.path().join("skipme").join("a.rs");

    cli(dir.path())
        .args([
            "check",
            abs.to_str().unwrap(),
            "--no-summary",
            "--no-remediation",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("skipme_offender").not())
        .stderr(predicate::str::contains(
            "skipped 1 violations via [check.exclude]",
        ));
}

/// `--include` is an allow-list, not a deny-set, and an explicit path
/// does not override it: a named file the allow-list does not admit is
/// filtered out and the run has no input at all (`check`'s hard error,
/// exit 1). Pins that the override widened `passes` to `includes` and
/// no further.
#[test]
fn explicit_path_does_not_override_include() {
    let dir = fixture("");

    cli(dir.path())
        .args([
            "check",
            "skipme/a.rs",
            "--include",
            "*.py",
            "--no-summary",
            "--no-remediation",
        ])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("skipme_offender").not())
        .stderr(predicate::str::contains("no input files matched"));
}

/// The include allow-list admitting the file is what makes the test
/// above a statement about `--include` rather than about any narrowing
/// glob: with a matching pattern the same invocation reports the
/// offender.
#[test]
fn explicit_path_is_analyzed_when_include_admits_it() {
    let dir = fixture("");

    cli(dir.path())
        .args([
            "check",
            "skipme/a.rs",
            "--include",
            "*.rs",
            "--no-summary",
            "--no-remediation",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("skipme_offender"));
}
