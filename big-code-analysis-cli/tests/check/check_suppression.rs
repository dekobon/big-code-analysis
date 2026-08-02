//! Integration tests for in-source suppression markers wired through
//! `bca check` (#98).
//!
//! These tests drive the binary against tiny inline fixtures and
//! verify both the default "honor markers" behaviour and the
//! `--no-suppress` override that CI auditors use to see un-silenced
//! offender lists.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

use crate::common;

/// Hermetic `bca` builder: anchors the process cwd at `dir` (a
/// `tempfile::tempdir()` with no `.git` ancestor) so `bca check` cannot
/// auto-discover the repo's own `bca.toml` / `.bca-baseline.toml` and
/// filter the inline fixtures against repo state (#491).
fn cli(dir: &Path) -> Command {
    common::cli_in(dir)
}

/// Rust function with cyclomatic complexity > 1 plus an inline
/// `bca: suppress` marker silencing cyclomatic. Used to confirm the
/// honor / ignore paths.
const SUPPRESSED_RUST: &str = r#"
pub fn classify(n: i32) -> &'static str {
    // bca: suppress(cyclomatic)
    if n < 0 {
        "neg"
    } else if n == 0 {
        "zero"
    } else {
        "pos"
    }
}
"#;

/// Rust function carrying a Lizard-style marker. Confirms the compat
/// layer fires identically to the native marker. The `#` sigil is
/// part of the Lizard directive itself; `//` is the language comment
/// opener.
const LIZARD_RUST: &str = r#"
pub fn classify(n: i32) -> &'static str {
    // #lizard forgives
    if n < 0 {
        "neg"
    } else {
        "pos"
    }
}
"#;

/// Rust source with a file-level marker covering `cyclomatic`.
const FILE_SUPPRESSED_RUST: &str = r#"
// bca: suppress-file(cyclomatic)

pub fn classify(n: i32) -> &'static str {
    if n < 0 {
        "neg"
    } else {
        "pos"
    }
}
"#;

fn write_fixture(dir: &TempDir, name: &str, body: &str) -> String {
    let path = dir.path().join(name);
    fs::write(&path, body).expect("write fixture");
    path.to_str().expect("utf8 fixture path").to_string()
}

#[test]
fn suppression_marker_silences_violation_by_default() {
    // `classify` would exceed cyclomatic=1 by a wide margin, but the
    // inline `bca: suppress(cyclomatic)` marker should silence the
    // violation so the run exits 0 with empty stderr.
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", SUPPRESSED_RUST);

    cli(dir.path())
        .args(["check", "--paths", &path, "--threshold", "cyclomatic=1"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::is_empty());
}

#[test]
fn no_suppress_flag_re_enables_violation() {
    // `--no-suppress` is the audit toggle: every marker is ignored,
    // and the same fixture that exits 0 without the flag now exits 2.
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", SUPPRESSED_RUST);

    cli(dir.path())
        .args([
            "check",
            "--paths",
            &path,
            "--threshold",
            "cyclomatic=1",
            "--no-suppress",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("classify"))
        .stdout(predicate::str::contains("cyclomatic"));
}

#[test]
fn lizard_compat_marker_silences_violation() {
    // The `#lizard forgives` marker must produce the same exit-code
    // behaviour as the native `bca: suppress` form, so codebases coming
    // from Lizard migrate cleanly without rewriting comments.
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", LIZARD_RUST);

    cli(dir.path())
        .args(["check", "--paths", &path, "--threshold", "cyclomatic=1"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::is_empty());
}

#[test]
fn file_scoped_marker_silences_nested_function_violation() {
    // A file-scoped marker must silence violations on every nested
    // function, not just the top-level Unit space. The threshold
    // engine ORs the file-scope against each function's own scope.
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", FILE_SUPPRESSED_RUST);

    cli(dir.path())
        .args(["check", "--paths", &path, "--threshold", "cyclomatic=1"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::is_empty());
}

/// Regression fixture for #263. The verb `allow` was the old marker
/// spelling; after the hard rename it is no longer recognized and
/// must leave the cyclomatic violation visible.
const LEGACY_ALLOW_RUST: &str = r#"
pub fn classify(n: i32) -> &'static str {
    // bca: allow(cyclomatic)
    if n < 0 {
        "neg"
    } else if n == 0 {
        "zero"
    } else {
        "pos"
    }
}
"#;

#[test]
fn legacy_allow_marker_does_not_suppress() {
    // Hard-rename regression (#263): a `// bca: allow(...)` comment in
    // shipped source must NOT silence the violation. The parser
    // surfaces `allow` / `allow-file` as `UnknownVerb`, which the
    // walk-time scanner drops with a stderr warning — the threshold
    // checker then sees no marker and the violation fires normally.
    //
    // Three things must all be true; we pin each one independently so
    // a regression in any single half (e.g., walker silently swallows
    // the error, or warning text drifts without the violation firing)
    // surfaces clearly:
    //   1. exit code 2 — the violation is reported, the marker did not
    //      suppress it;
    //   2. stdout names the offender and metric — the violation line
    //      exists and is intelligible;
    //   3. stderr names the bad verb — the user gets a diagnostic
    //      pointing them at the rename, not a silent drop.
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", LEGACY_ALLOW_RUST);

    cli(dir.path())
        .args(["check", "--paths", &path, "--threshold", "cyclomatic=1"])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("classify"))
        .stdout(predicate::str::contains("cyclomatic"))
        .stderr(predicate::str::contains(
            "unknown bca directive verb 'allow'",
        ));
}

/// Fixture for the mixed known/unknown metric list. The marker names
/// one recognized metric (`cyclomatic`) beside one that does not exist
/// (`bogusmetric`).
const UNKNOWN_METRIC_RUST: &str = r#"
pub fn classify(n: i32) -> &'static str {
    // bca: suppress(cyclomatic, bogusmetric)
    if n < 0 {
        "neg"
    } else if n == 0 {
        "zero"
    } else {
        "pos"
    }
}
"#;

/// Fixture whose marker names *only* an unrecognized metric, so nothing
/// in it can suppress. Separates "skip the bad name" from "void the
/// marker": under both contracts the mixed fixture above differs, but
/// this one must behave identically — the violation still fires.
const ONLY_UNKNOWN_METRIC_RUST: &str = r#"
pub fn classify(n: i32) -> &'static str {
    // bca: suppress(bogusmetric)
    if n < 0 {
        "neg"
    } else if n == 0 {
        "zero"
    } else {
        "pos"
    }
}
"#;

#[test]
fn unknown_metric_is_skipped_and_the_rest_of_the_list_suppresses() {
    // Issue #1168 replaced the void-on-unknown contract (#896) with
    // skip-and-report: an unrecognized name costs its own name and
    // nothing else. `exit`-for-`nexits` is a typo `AGENTS.md` documents
    // people making, and voiding the marker wholesale turned it into a
    // suppression the author believed was active while the gate
    // disagreed — the exact failure #1168 is about.
    //
    // Skipping cannot widen scope: the honored set only ever shrinks.
    // Three things must all be true, pinned independently:
    //   1. exit code 0 — `cyclomatic` is genuinely suppressed;
    //   2. stdout carries no offender row for it (#1167 put offender
    //      rows on stdout, diagnostics on stderr);
    //   3. stderr still carries the unknown-metric diagnostic, so the
    //      typo is not silent.
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", UNKNOWN_METRIC_RUST);

    cli(dir.path())
        .args(["check", "--paths", &path, "--threshold", "cyclomatic=1"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("classify").not())
        .stderr(predicate::str::contains(
            "unknown metric 'bogusmetric' in bca suppression marker",
        ));
}

#[test]
fn a_marker_naming_only_unknown_metrics_suppresses_nothing() {
    // The other half of the #1168 contract: skipping every name in the
    // list leaves an empty one, which silences nothing. A regression
    // that treated an unusable list as a bare `suppress` (all metrics)
    // would swallow the violation — the most dangerous direction, and
    // the one the void-on-unknown rule was written to prevent. This
    // still fires the violation, so that protection survives the
    // relaxation.
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", ONLY_UNKNOWN_METRIC_RUST);

    cli(dir.path())
        .args(["check", "--paths", &path, "--threshold", "cyclomatic=1"])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("classify"))
        .stdout(predicate::str::contains("cyclomatic"))
        .stderr(predicate::str::contains(
            "unknown metric 'bogusmetric' in bca suppression marker",
        ));
}

/// The issue #1168 reproducer: two identical over-parameterised
/// functions, one marker bare and one carrying the rationale
/// `AGENTS.md` asks contributors to write.
const RATIONALE_RUST: &str = r"
pub fn many_bare(a: u8, b: u8, c: u8, d: u8, e: u8, f: u8, g: u8, h: u8) -> u8 {
    // bca: suppress(nargs)
    a + b + c + d + e + f + g + h
}

pub fn many_prose(a: u8, b: u8, c: u8, d: u8, e: u8, f: u8, g: u8, h: u8) -> u8 {
    // bca: suppress(nargs) — threaded context, not a god-function
    a + b + c + d + e + f + g + h
}
";

#[test]
fn a_trailing_rationale_suppresses_exactly_like_a_bare_marker() {
    // The #1168 reproducer end-to-end. Before the fix the second
    // function's marker was rejected as malformed, so `many_prose` was
    // reported while `many_bare` was not — a suppression its author had
    // every reason to believe was active.
    //
    // Both halves are asserted: the run is clean (neither function is
    // reported) *and* no `warning:` reaches stderr, since a marker that
    // works must not also complain about itself.
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "wide.rs", RATIONALE_RUST);

    cli(dir.path())
        .args(["check", "--paths", &path, "--threshold", "nargs=5"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("many_prose").not())
        .stdout(predicate::str::contains("many_bare").not())
        .stderr(predicate::str::contains("warning:").not());

    // Exit 0 plus an empty stdout is also what a tree with no violation
    // at all produces, so the assertions above cannot tell "suppressed"
    // from "never offended". `--no-suppress` supplies the positive
    // control: with markers ignored, both functions must be reported.
    cli(dir.path())
        .args([
            "check",
            "--paths",
            &path,
            "--threshold",
            "nargs=5",
            "--no-suppress",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("many_bare"))
        .stdout(predicate::str::contains("many_prose"));

    // …and the marker silences only the metric it names. Gating a
    // second metric the list omits keeps both functions on the report,
    // which is what separates `SuppressionScope::Some([nargs])` from a
    // scope-widening `All` — the two are indistinguishable while
    // `nargs` is the only metric under test.
    cli(dir.path())
        .args([
            "check",
            "--paths",
            &path,
            "--threshold",
            "nargs=5",
            "--threshold",
            "cyclomatic=0",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("cyclomatic"))
        .stdout(predicate::str::contains("many_bare"))
        .stdout(predicate::str::contains("many_prose"))
        .stdout(predicate::str::contains("nargs").not());
}

#[test]
fn a_bare_verb_with_trailing_text_warns_without_failing_the_run() {
    // The genuine-malformation path. A bare verb followed by words is
    // not a marker — but it must warn rather than fail, or a doc comment
    // or test fixture that merely mentions the syntax would break the
    // gate.
    //
    // The exit code is 0 because the fixture has no violation at the
    // threshold under test: the marker is inert *and* the malformation
    // is not itself fatal.
    let dir = TempDir::new().unwrap();
    let path = write_fixture(
        &dir,
        "prose.rs",
        "pub fn tiny(a: u8) -> u8 {\n    // bca: suppress markers are honoured here\n    a\n}\n",
    );

    cli(dir.path())
        .args(["check", "--paths", &path, "--threshold", "nargs=5"])
        .assert()
        .code(0)
        .stderr(predicate::str::contains(
            "malformed bca suppression marker body",
        ));
}

/// Two over-parameterised functions whose in-body comments are prose
/// *about* a marker, written with the punctuation a rationale would use.
const PROSE_ABOUT_A_MARKER_RUST: &str = r"
pub fn dashed(a: u8, b: u8, c: u8, d: u8, e: u8, f: u8, g: u8, h: u8) -> u8 {
    // bca: suppress - we removed this marker, see #123
    a + b + c + d + e + f + g + h
}

pub fn colonned(a: u8, b: u8, c: u8, d: u8, e: u8, f: u8, g: u8, h: u8) -> u8 {
    // bca: suppress: not applicable to this function
    a + b + c + d + e + f + g + h
}
";

#[test]
fn prose_about_a_bare_marker_neither_suppresses_nor_passes_silently() {
    // #1168 briefly let a bare verb carry a rationale when it opened
    // with `-`, `:`, `//`, `#`, or an em/en dash — the same punctuation
    // an author uses to write *about* a marker. Both functions below
    // then reported no violation at all, with nothing on stderr: the
    // most expensive failure this tool has, since it looks exactly like
    // compliant code.
    //
    // Both halves matter. The violations must fire (stdout, exit 2, per
    // #1167's stream split), and the warning must reach stderr so the
    // author of a genuinely-intended marker learns it did nothing.
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "prose_marker.rs", PROSE_ABOUT_A_MARKER_RUST);

    cli(dir.path())
        .args(["check", "--paths", &path, "--threshold", "nargs=5"])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("dashed"))
        .stdout(predicate::str::contains("colonned"))
        .stderr(predicate::str::contains(
            "malformed bca suppression marker body",
        ))
        // The warning is the only route out of this shape, so it has to
        // say where to go: name the metrics, or move the reason up.
        .stderr(predicate::str::contains("`bca: suppress(<metrics>)`"))
        .stderr(predicate::str::contains("line above"));
}

#[test]
fn unsuppressed_metric_still_violates() {
    // Per-metric scoping: `bca: suppress(cyclomatic)` leaves other
    // metrics' violations visible. Threshold on a non-listed metric
    // (cognitive) still fires.
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "branchy.rs", SUPPRESSED_RUST);

    cli(dir.path())
        .args(["check", "--paths", &path, "--threshold", "cognitive=0"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cognitive"));
}
