// Sibling-file unit tests for the `commands` module. Wired via
// `#[path = "commands_tests.rs"] mod tests;` so the production
// `commands.rs` stays under the `bca check` per-file metric caps.
// Matched by the `./**/*_tests.rs` rule in `.bcaignore`.

use std::collections::HashSet;
use std::path::PathBuf;

use super::*;
use crate::diff::{DiffScope, DiffSource};

fn violation(path: &str, function: &str, value: f64, limit: f64) -> Violation {
    Violation {
        path: PathBuf::from(path),
        start_line: 1,
        end_line: 10,
        function: function.to_string(),
        metric: "cyclomatic",
        value,
        limit,
    }
}

fn scope_with(base: &str, paths: &[&str]) -> DiffScope {
    // Test paths are synthetic (don't exist on disk), so
    // `canonicalize_for_match` falls back to the identity `PathBuf`
    // for both the scope and the lookup. The test exercises the
    // membership-test logic for byte-identical paths only; the
    // separate `apply_changed_only_matches_real_files_via_canonicalize`
    // test below pins the canonicalize-roundtrip path against real
    // on-disk files.
    DiffScope {
        base: base.to_string(),
        source: DiffSource::Explicit,
        changed: paths.iter().map(PathBuf::from).collect(),
    }
}

#[test]
fn write_summary_footer_no_scope_matches_pre_diff_format() {
    // Without a diff scope, the footer is byte-identical to the
    // pre-#359 single-section listing. This protects every CI tool
    // that grep-anchors on the legacy "--- summary ---" + path-prefix
    // shape.
    let pairs = vec![
        (violation("bca-test-synthetic-a.rs", "f", 20.0, 10.0), None),
        (violation("bca-test-synthetic-a.rs", "g", 30.0, 10.0), None),
        (violation("bca-test-synthetic-b.rs", "h", 11.0, 10.0), None),
    ];
    let mut buf = Vec::new();
    write_summary_footer(&mut buf, &pairs, None).expect("write");
    let out = String::from_utf8(buf).expect("utf8");
    let expected = "\n--- summary ---\n\
        bca-test-synthetic-a.rs: 2 violations (worst: cyclomatic = 30 vs limit 10 at L1)\n\
        bca-test-synthetic-b.rs: 1 violation (worst: cyclomatic = 11 vs limit 10 at L1)\n";
    assert_eq!(out, expected);
}

#[test]
fn write_summary_footer_partitions_in_range_and_other() {
    // With a diff scope where only `bca-test-synthetic-a.rs` was touched, the
    // footer surfaces it under "Files in this range:" first, then
    // emits "Other offenders:" with `bca-test-synthetic-b.rs`. The reader sees
    // their own contribution before the legacy offender list.
    let pairs = vec![
        (violation("bca-test-synthetic-a.rs", "f", 20.0, 10.0), None),
        (violation("bca-test-synthetic-b.rs", "g", 30.0, 10.0), None),
    ];
    let scope = scope_with("main", &["bca-test-synthetic-a.rs"]);
    let mut buf = Vec::new();
    write_summary_footer(&mut buf, &pairs, Some(&scope)).expect("write");
    let out = String::from_utf8(buf).expect("utf8");
    let expected = "\n--- summary ---\n\
        Files in this range (diff base: main via --since):\n\
        bca-test-synthetic-a.rs: 1 violation (worst: cyclomatic = 20 vs limit 10 at L1)\n\
        \n\
        Other offenders:\n\
        bca-test-synthetic-b.rs: 1 violation (worst: cyclomatic = 30 vs limit 10 at L1)\n";
    assert_eq!(out, expected);
}

#[test]
fn write_summary_footer_in_range_empty_when_no_touched_offenders() {
    // The reader needs an explicit "your change is clean" signal
    // when no offenders fall inside the diff. Without it, the footer
    // would jump straight to "Other offenders:" and the reader would
    // have to compare both halves to confirm their own files were
    // absent. Pin the full byte sequence so a future change that
    // drops or reorders the empty-in-range note fails the test
    // rather than silently shipping a less-informative footer.
    let pairs = vec![(violation("bca-test-synthetic-b.rs", "g", 30.0, 10.0), None)];
    let scope = scope_with("main", &["bca-test-synthetic-a.rs"]);
    let mut buf = Vec::new();
    write_summary_footer(&mut buf, &pairs, Some(&scope)).expect("write");
    let out = String::from_utf8(buf).expect("utf8");
    let expected = "\n--- summary ---\n\
        Files in this range (diff base: main via --since):\n\
        \x20\x20(none — no offenders in files touched by this diff)\n\
        \n\
        Other offenders:\n\
        bca-test-synthetic-b.rs: 1 violation (worst: cyclomatic = 30 vs limit 10 at L1)\n";
    assert_eq!(out, expected);
}

#[test]
fn write_summary_footer_omits_other_section_when_all_in_range() {
    // Symmetric to the empty-in-range case: when every offender is
    // in the diff, the "Other offenders:" heading would be a dead
    // section. Pin the full byte sequence so a regression that
    // silently emits an empty in-range section or an extra blank
    // line below the row is caught.
    let pairs = vec![(violation("bca-test-synthetic-a.rs", "f", 20.0, 10.0), None)];
    let scope = scope_with("main", &["bca-test-synthetic-a.rs"]);
    let mut buf = Vec::new();
    write_summary_footer(&mut buf, &pairs, Some(&scope)).expect("write");
    let out = String::from_utf8(buf).expect("utf8");
    let expected = "\n--- summary ---\n\
        Files in this range (diff base: main via --since):\n\
        bca-test-synthetic-a.rs: 1 violation (worst: cyclomatic = 20 vs limit 10 at L1)\n";
    assert_eq!(out, expected);
}

#[test]
fn apply_changed_only_drops_files_outside_scope() {
    // The flag is "terser CI output for PR gates" per #359, so
    // violations from files the developer did not touch must vanish
    // entirely — not just sink to a separate section.
    let pairs = vec![
        (violation("bca-test-synthetic-a.rs", "f", 20.0, 10.0), None),
        (violation("bca-test-synthetic-b.rs", "g", 30.0, 10.0), None),
    ];
    let scope = scope_with("main", &["bca-test-synthetic-a.rs"]);
    let kept = apply_changed_only(pairs, Some(&scope), true);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].0.path, PathBuf::from("bca-test-synthetic-a.rs"));
}

#[test]
fn apply_changed_only_is_noop_when_flag_off() {
    // The footer-partition path is the only behaviour that should
    // change without `--changed-only`: violation visibility is
    // unchanged.
    let pairs = vec![
        (violation("bca-test-synthetic-a.rs", "f", 20.0, 10.0), None),
        (violation("bca-test-synthetic-b.rs", "g", 30.0, 10.0), None),
    ];
    let scope = scope_with("main", &["bca-test-synthetic-a.rs"]);
    let kept = apply_changed_only(pairs, Some(&scope), false);
    assert_eq!(kept.len(), 2);
}

#[test]
fn apply_changed_only_passes_through_when_scope_missing() {
    // Defensive: `resolve_diff_scope` is supposed to fatal-error if
    // `--changed-only` is on without a resolvable scope, but if a
    // future refactor bypasses that check we want filtering to
    // degrade to a no-op rather than emit the empty set (which would
    // green-light a broken CI gate).
    let pairs = vec![(violation("bca-test-synthetic-a.rs", "f", 20.0, 10.0), None)];
    let kept = apply_changed_only(pairs, None, true);
    assert_eq!(kept.len(), 1);
}

#[test]
fn apply_changed_only_matches_real_files_via_canonicalize() {
    // Pin the canonicalize-roundtrip with real on-disk files. Both
    // sides — the scope's `changed` set and `DiffScope::contains`'s
    // lookup — call `Path::canonicalize`, which for absolute paths
    // resolves symlinks and `.` / `..` components. This test uses
    // absolute paths on both sides so the canonicalization step
    // actually runs; the synthetic-relative-path tests above bypass
    // the roundtrip via the missing-file identity fallback.
    //
    // Production parity: `--paths /abs/dir` produces violations with
    // absolute paths; this test mirrors that shape.
    let dir = tempfile::tempdir().expect("tempdir");
    let a = dir.path().join("a.rs");
    let b = dir.path().join("b.rs");
    std::fs::write(&a, "// a").expect("write a");
    std::fs::write(&b, "// b").expect("write b");
    let scope = DiffScope {
        base: "main".to_string(),
        source: DiffSource::Explicit,
        changed: HashSet::from([a.canonicalize().expect("canon a")]),
    };
    let pairs = vec![
        (
            violation(a.to_str().expect("utf8 a"), "f", 20.0, 10.0),
            None,
        ),
        (
            violation(b.to_str().expect("utf8 b"), "g", 30.0, 10.0),
            None,
        ),
    ];
    let kept = apply_changed_only(pairs, Some(&scope), true);
    assert_eq!(
        kept.len(),
        1,
        "expected only a.rs to survive --changed-only; got {kept:?}"
    );
    assert_eq!(kept[0].0.path, a);
}

#[test]
fn apply_changed_only_empty_scope_drops_all_violations_with_clean_exit() {
    // A resolved-but-empty scope (a real edge case: `--since main`
    // against a branch already squash-merged into main) used to be
    // a silent green-light — every violation dropped, exit 0, CI
    // happy. Now it's an explicit `bca:` log line and a deliberate
    // empty return.
    let scope = DiffScope {
        base: "main".to_string(),
        source: DiffSource::Explicit,
        changed: HashSet::new(),
    };
    let pairs = vec![(violation("bca-test-synthetic-a.rs", "f", 20.0, 10.0), None)];
    let kept = apply_changed_only(pairs, Some(&scope), true);
    assert!(kept.is_empty());
}

#[test]
fn write_summary_footer_in_range_uses_source_label() {
    // The footer banner reports *how* the diff base was resolved
    // (`--since`, `BCA_DIFF_BASE`, `GITHUB_BASE_REF`,
    // `GITHUB_EVENT_BEFORE`) so a CI log reader can verify the gate
    // latched onto the expected signal. Pin every source label here
    // — silent label drift would mislead readers.
    let pairs = vec![(violation("bca-test-synthetic-a.rs", "f", 20.0, 10.0), None)];
    for (source, expected_marker) in [
        (DiffSource::Explicit, "--since"),
        (DiffSource::EnvOverride, "BCA_DIFF_BASE"),
        (DiffSource::GithubPr, "GITHUB_BASE_REF"),
        (DiffSource::GithubPush, "GITHUB_EVENT_BEFORE"),
    ] {
        let scope = DiffScope {
            base: "abc".to_string(),
            source,
            changed: HashSet::from([PathBuf::from("bca-test-synthetic-a.rs")]),
        };
        let mut buf = Vec::new();
        write_summary_footer(&mut buf, &pairs, Some(&scope)).expect("write");
        let out = String::from_utf8(buf).expect("utf8");
        assert!(
            out.contains(&format!("via {expected_marker}")),
            "source {source:?} should render label {expected_marker}, got: {out}"
        );
    }
}

// --- Remediation footer tests ---

fn check_args_for_remediation(
    config: Option<&str>,
    baseline: Option<&str>,
    no_remediation: bool,
) -> CheckArgs {
    CheckArgs {
        thresholds: Vec::new(),
        config: config.map(PathBuf::from),
        no_fail: false,
        no_suppress: false,
        output_format: None,
        output: None,
        baseline: baseline.map(PathBuf::from),
        write_baseline: None,
        no_summary: false,
        since: None,
        changed_only: false,
        github_annotations: false,
        summary_file: None,
        no_remediation,
    }
}

fn globals_for_remediation(
    paths: &[&str],
    exclude: &[&str],
    exclude_from: Option<&str>,
) -> GlobalOpts {
    GlobalOpts {
        paths: paths.iter().map(PathBuf::from).collect(),
        exclude: exclude.iter().map(|s| (*s).to_string()).collect(),
        exclude_from: exclude_from.map(PathBuf::from),
        ..GlobalOpts::default()
    }
}

#[test]
fn format_remediation_block_returns_none_when_suppressed() {
    let globals = globals_for_remediation(&["."], &[], None);
    let args = check_args_for_remediation(None, None, true);
    assert!(format_remediation_block(&globals, &args).is_none());
}

#[test]
fn format_remediation_block_contains_three_bullet_points() {
    let globals = globals_for_remediation(&["."], &[], None);
    let args = check_args_for_remediation(Some("bca-thresholds.toml"), None, false);
    let out = format_remediation_block(&globals, &args).expect("remediation present");
    assert!(out.contains("--- next steps ---"));
    assert!(
        out.contains("* Detailed reports:"),
        "missing artifact bullet, got:\n{out}"
    );
    assert!(
        out.contains("* To refresh baseline:"),
        "missing refresh bullet, got:\n{out}"
    );
    assert!(
        out.contains(
            "* Adoption guide: https://dekobon.github.io/big-code-analysis/recipes/baselines.html"
        ),
        "missing book link, got:\n{out}"
    );
}

#[test]
fn refresh_baseline_command_mirrors_resolved_args() {
    // The copy-paste invocation must reproduce the gate's --paths /
    // --exclude / --exclude-from / --config so the user can run it
    // verbatim. Hard-coding `--paths .` would be wrong for repos
    // that scope scans differently.
    let globals =
        globals_for_remediation(&["src", "tests"], &["target", "vendor"], Some(".bcaignore"));
    let args = check_args_for_remediation(
        Some("bca-thresholds.toml"),
        Some(".bca-baseline.toml"),
        false,
    );
    let cmd = refresh_baseline_command(&globals, &args);
    assert!(
        cmd.starts_with("bca "),
        "must start with `bca `, got: {cmd}"
    );
    assert!(
        cmd.contains("--paths src"),
        "missing --paths src, got: {cmd}"
    );
    assert!(
        cmd.contains("--paths tests"),
        "missing --paths tests, got: {cmd}"
    );
    assert!(
        cmd.contains("--exclude target"),
        "missing --exclude target, got: {cmd}"
    );
    assert!(
        cmd.contains("--exclude vendor"),
        "missing --exclude vendor, got: {cmd}"
    );
    assert!(
        cmd.contains("--exclude-from .bcaignore"),
        "missing --exclude-from, got: {cmd}"
    );
    assert!(
        cmd.contains("check"),
        "missing `check` subcommand, got: {cmd}"
    );
    assert!(
        cmd.contains("--config bca-thresholds.toml"),
        "missing --config, got: {cmd}"
    );
    assert!(
        cmd.contains("--write-baseline .bca-baseline.toml"),
        "missing --write-baseline, got: {cmd}"
    );
}

#[test]
fn refresh_baseline_command_defaults_paths_when_unset() {
    // `--paths .` is the walker's implicit default; the remediation
    // block must print it explicitly so the user can copy-paste
    // without thinking about which directory `bca` would have walked
    // by default.
    let globals = globals_for_remediation(&[], &[], None);
    let args = check_args_for_remediation(None, None, false);
    let cmd = refresh_baseline_command(&globals, &args);
    assert!(
        cmd.contains("--paths ."),
        "missing default --paths, got: {cmd}"
    );
    // And the default baseline target is `.bca-baseline.toml`.
    assert!(
        cmd.contains("--write-baseline .bca-baseline.toml"),
        "missing default baseline path, got: {cmd}"
    );
}

#[test]
fn refresh_baseline_command_shell_quotes_paths_with_spaces() {
    // A `--paths` arg containing a space must be quoted so the
    // copy-paste command shells correctly. The simple identifier
    // path takes the fast no-quote branch.
    let globals = globals_for_remediation(&["dir with space", "src"], &[], None);
    let args = check_args_for_remediation(None, None, false);
    let cmd = refresh_baseline_command(&globals, &args);
    assert!(
        cmd.contains("--paths 'dir with space'"),
        "expected single-quoted spaced path, got: {cmd}"
    );
    assert!(
        cmd.contains("--paths src"),
        "expected unquoted simple path, got: {cmd}"
    );
}

#[test]
fn artifact_link_falls_back_to_plain_text_without_gha_env() {
    // No mutation of env vars in this test — the cargo-test process
    // typically does not have GITHUB_REPOSITORY / GITHUB_RUN_ID set,
    // so the fallback path is the natural state. If those vars
    // happen to be set in the test env (rare), the assertion is
    // skipped and the test reports as "ok" — the load-bearing
    // behaviour is the SOME branch which is exercised by
    // `format_remediation_block_contains_three_bullet_points`.
    if std::env::var_os("GITHUB_REPOSITORY").is_some()
        && std::env::var_os("GITHUB_RUN_ID").is_some()
    {
        return;
    }
    let link = artifact_link();
    assert_eq!(link, "bca-reports artifact (uploaded to this run)");
}
