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
        hard_limit: Some(limit),
        lower_is_worse: false,
        body_hash: None,
        suppressed: false,
        nargs_split: None,
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
    // green-light a broken CI gate). The pure inner function returns
    // a diagnostic the caller surfaces via stderr; assert on it here
    // so a refactor that silences the warning fails the test.
    let pairs = vec![(violation("bca-test-synthetic-a.rs", "f", 20.0, 10.0), None)];
    let outcome = apply_changed_only_inner(pairs, None, true);
    assert_eq!(outcome.kept.len(), 1);
    let diag = outcome.diagnostic.expect("expected diagnostic warning");
    assert!(
        diag.contains("--changed-only requested but no diff scope is available"),
        "diagnostic should warn about the missing-scope footgun, got: {diag}"
    );
}

#[cfg(unix)]
#[test]
fn apply_changed_only_matches_real_files_via_canonicalize() {
    // Pin the canonicalize-roundtrip with real on-disk files. Both
    // sides — the scope's `changed` set and `DiffScope::contains`'s
    // lookup — call `Path::canonicalize`, which resolves symlinks and
    // `.` / `..` components.
    //
    // **Load-bearing**: we route the violation path through a
    // symlink (`dir/link/a.rs` where `link -> .`) so the *raw*
    // violation path is structurally distinct from the canonical
    // form stored in `scope.changed`. Without the symlink, the
    // tempdir's canonical form already equals its raw form on Linux
    // (`/tmp/.tmpXXX/...`), and the test would still pass even if
    // `canonicalize_for_match` were the identity function — false-
    // pass. The symlink forces the roundtrip to actually fire.
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().expect("tempdir");
    let a = dir.path().join("a.rs");
    let b = dir.path().join("b.rs");
    std::fs::write(&a, "// a").expect("write a");
    std::fs::write(&b, "// b").expect("write b");
    // Create a self-referential symlink `link -> .` so paths via
    // `dir/link/a.rs` and `dir/a.rs` resolve to the same inode but
    // differ structurally.
    let link = dir.path().join("link");
    symlink(".", &link).expect("symlink");
    let via_link_a = link.join("a.rs");
    let via_link_b = link.join("b.rs");
    // Scope stores the canonical form (no `link/` segment).
    let scope = DiffScope {
        base: "main".to_string(),
        source: DiffSource::Explicit,
        changed: HashSet::from([a.canonicalize().expect("canon a")]),
    };
    // Violation paths go through the symlink — raw form contains
    // `link/`, canonical form does not. A bug that turned
    // `canonicalize_for_match` into the identity function would
    // store `…/link/a.rs` on the lookup side, miss the canonical
    // `…/a.rs` in `changed`, and the test would fail.
    let pairs = vec![
        (
            violation(via_link_a.to_str().expect("utf8 a"), "f", 20.0, 10.0),
            None,
        ),
        (
            violation(via_link_b.to_str().expect("utf8 b"), "g", 30.0, 10.0),
            None,
        ),
    ];
    let kept = apply_changed_only(pairs, Some(&scope), true);
    assert_eq!(
        kept.len(),
        1,
        "expected only a.rs (via the symlink) to survive --changed-only; got {kept:?}"
    );
    assert_eq!(kept[0].0.path, via_link_a);
}

#[test]
fn apply_changed_only_empty_scope_drops_all_violations_with_clean_exit() {
    // A resolved-but-empty scope (a real edge case: `--since main`
    // against a branch already squash-merged into main) used to be
    // a silent green-light — every violation dropped, exit 0, CI
    // happy. Now it's an explicit `bca:` log line and a deliberate
    // empty return. Assert on the diagnostic so a refactor that
    // silences the warning fails the test.
    let scope = DiffScope {
        base: "main".to_string(),
        source: DiffSource::Explicit,
        changed: HashSet::new(),
    };
    let pairs = vec![(violation("bca-test-synthetic-a.rs", "f", 20.0, 10.0), None)];
    let outcome = apply_changed_only_inner(pairs, Some(&scope), true);
    assert!(outcome.kept.is_empty());
    let diag = outcome.diagnostic.expect("expected diagnostic warning");
    assert!(
        diag.contains("diff scope is empty") && diag.contains("between main and HEAD"),
        "diagnostic should name the empty scope explicitly, got: {diag}"
    );
    // The N-violations branch must say "dropping N violations"; the
    // empty-pairs branch (no violations at all + empty scope) gets a
    // different wording so a clean PR log doesn't imply suppression.
    assert!(
        diag.contains("dropping 1 violations"),
        "expected 'dropping N violations' wording for non-empty pairs, got: {diag}"
    );
}

#[test]
fn apply_changed_only_empty_scope_with_empty_pairs_uses_friendlier_wording() {
    // Regression: when `--changed-only` is set, the scope resolves
    // to empty, AND no upstream violations were produced, the old
    // wording said "dropping 0 violations and exiting clean" — which
    // implies the gate suppressed something it did not. A developer
    // reading a clean PR log expecting silence would find this
    // confusing. Branch the diagnostic on `pairs.is_empty()` so the
    // empty-pairs message reads naturally.
    let scope = DiffScope {
        base: "main".to_string(),
        source: DiffSource::Explicit,
        changed: HashSet::new(),
    };
    let outcome = apply_changed_only_inner(Vec::new(), Some(&scope), true);
    assert!(outcome.kept.is_empty());
    let diag = outcome.diagnostic.expect("expected diagnostic note");
    assert!(
        diag.contains("no violations to check and no files in diff scope"),
        "expected friendlier wording for empty-pairs + empty-scope, got: {diag}"
    );
    assert!(
        !diag.contains("dropping 0 violations"),
        "must NOT use 'dropping 0' wording in the empty-pairs branch, got: {diag}"
    );
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

/// All-inert `CheckArgs` for tests: every field takes the value clap
/// would produce for an unset flag. Tests adjust only the fields they
/// exercise via `..base_check_args()`, so a new `CheckArgs` field needs
/// updating in exactly one place.
fn base_check_args() -> CheckArgs {
    CheckArgs {
        positional: crate::PositionalPaths::default(),
        selection: crate::WalkSelectionArgs::default(),
        tuning: crate::WalkTuningArgs::default(),
        preproc: crate::PreprocConsumeArgs::default(),
        thresholds: Vec::new(),
        explain_thresholds: Vec::new(),
        config: None,
        no_fail: false,
        no_suppress: false,
        strict: false,
        report_suppressed: false,
        output_format: None,
        output: None,
        baseline: None,
        write_baseline: None,
        no_summary: false,
        since: None,
        changed_only: false,
        github_annotations: crate::CiDetect::Auto,
        summary_file: None,
        no_remediation: false,
        print_effective_config: None,
        headroom: None,
        tier: crate::TierSpec::Hard,
        exit_codes: None,
        strict_exit_codes: false,
        baseline_line_tolerance: None,
        baseline_fuzzy_match: None,
        check_exclude: Vec::new(),
        check_exclude_from: None,
        manifest_check_exclude: None,
    }
}

fn check_args_for_remediation(
    config: Option<&str>,
    baseline: Option<&str>,
    no_remediation: bool,
) -> CheckArgs {
    CheckArgs {
        config: config.map(PathBuf::from),
        baseline: baseline.map(PathBuf::from),
        no_remediation,
        ..base_check_args()
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

/// Parse a rendered refresh argv back through the real clap surface,
/// returning the `check` args it produced. Any usage error — a flag in
/// a position clap no longer accepts, a value spelling the parser
/// rejects — fails here rather than being string-matched away (#1243).
fn parse_refresh_argv(argv: &[String]) -> CheckArgs {
    use clap::Parser;
    let parsed = crate::Cli::try_parse_from(argv).unwrap_or_else(|e| {
        panic!("emitted refresh command must parse; got:\n{e}\nargv: {argv:?}")
    });
    match parsed.command {
        crate::Command::Check(args) => *args,
        other => panic!("expected Command::Check, got {other:?}"),
    }
}

#[test]
fn format_remediation_block_returns_none_when_suppressed() {
    let globals = globals_for_remediation(&["."], &[], None);
    let args = check_args_for_remediation(None, None, true);
    assert!(format_remediation_block(&globals, &args, crate::TierSpec::Hard).is_none());
}

#[test]
fn format_remediation_block_contains_three_bullet_points() {
    let globals = globals_for_remediation(&["."], &[], None);
    let args = check_args_for_remediation(Some("bca-thresholds.toml"), None, false);
    let out = format_remediation_block(&globals, &args, crate::TierSpec::Hard)
        .expect("remediation present");
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
fn remediation_block_prints_the_command_the_builder_rendered() {
    // The block and the builder were each tested, but nothing pinned
    // the seam between them: every other test in this section calls
    // `refresh_baseline_argv` / `refresh_baseline_command` directly.
    // Hard-coding `TierSpec::Hard` at the block's call site — the
    // exact mistake threading the resolved tier through exists to
    // prevent — left the whole crate suite green.
    let globals = GlobalOpts {
        paths: vec![PathBuf::from("src")],
        exclude_tests: true,
        ..GlobalOpts::default()
    };
    let args = CheckArgs {
        no_suppress: true,
        ..base_check_args()
    };
    let tier = crate::TierSpec::Soft(Some(0.9));
    let out = format_remediation_block(&globals, &args, tier).expect("remediation present");
    let refresh: Vec<&str> = out
        .lines()
        .filter(|l| l.starts_with("* To refresh baseline: "))
        .collect();
    assert_eq!(refresh.len(), 1, "expected one refresh bullet, got:\n{out}");
    assert_eq!(
        refresh[0],
        format!(
            "* To refresh baseline: {}",
            refresh_baseline_command(&globals, &args, tier)
        ),
        "the block must print the builder's own rendering, got:\n{out}"
    );
    // The three arguments the block forwards, each with a value the
    // default cannot produce, so dropping any one of them shows up
    // here rather than in a builder-level test that never runs
    // through `format_remediation_block`.
    for expected in [
        "--paths src",
        "--exclude-tests",
        "--no-suppress",
        "--tier=soft=0.9",
    ] {
        assert!(
            refresh[0].contains(expected),
            "missing {expected} in the printed command: {}",
            refresh[0]
        );
    }
}

#[test]
fn refresh_baseline_command_names_the_subcommand_before_its_flags() {
    // The walk flags are subcommand-scoped (#597), so `check` must
    // come first. This is the *shape* half of the #1243 guard; the
    // half that would have caught the regression is
    // `refresh_baseline_argv_round_trips_through_the_parser` below,
    // which runs the string past the parser instead of past `contains`.
    let globals =
        globals_for_remediation(&["src", "tests"], &["target", "vendor"], Some(".bcaignore"));
    let args = check_args_for_remediation(
        Some("bca-thresholds.toml"),
        Some(".bca-baseline.toml"),
        false,
    );
    let cmd = refresh_baseline_command(&globals, &args, crate::TierSpec::Hard);
    assert!(
        cmd.starts_with("bca check --"),
        "walk flags must follow the subcommand, got: {cmd}"
    );
}

#[test]
fn refresh_baseline_argv_round_trips_through_the_parser() {
    // The regression guard for #1243. Every flag the gate ran with
    // must come back out of `Cli::try_parse_from` carrying the value
    // it went in with — not merely appear somewhere in the string.
    //
    // Fixture values are deliberately asymmetric (two paths but one
    // include, distinct globs per flag, two thresholds with different
    // numeric shapes) so a flag emitted with the wrong name or the
    // wrong value lands on its own failure rather than on a value
    // that happens to match. The four boolean switches are all `true`
    // here, which cannot tell them apart —
    // `refresh_baseline_argv_pairs_each_switch_with_its_own_flag`
    // covers that wiring.
    let globals = GlobalOpts {
        paths: vec![PathBuf::from("src"), PathBuf::from("tests")],
        include: vec!["*.rs".to_string()],
        exclude: vec!["target/**".to_string(), "vendor/**".to_string()],
        language: Some("rust".to_string()),
        no_skip_generated: true,
        preproc_data: Some(PathBuf::from("preproc.json")),
        paths_from: Some(PathBuf::from("paths.txt")),
        exclude_from: Some(PathBuf::from(".bcaignore")),
        no_ignore: true,
        exclude_tests: true,
        count_cyclomatic_try: Some(false),
        no_config: true,
        ..GlobalOpts::default()
    };
    let args = CheckArgs {
        thresholds: vec![
            ("cyclomatic".to_string(), 15.0),
            ("halstead.effort".to_string(), 12.5),
        ],
        config: Some(PathBuf::from("bca-thresholds.toml")),
        no_suppress: true,
        baseline: Some(PathBuf::from("custom-baseline.toml")),
        // `false`, not `true`. `--baseline-fuzzy-match` declares
        // `default_missing_value = "true"`, so a `true` fixture also
        // passes when the flag is emitted as *two* argv elements:
        // clap reads the bare flag as `true` and swallows the value
        // as a stray positional path. Only `false` can distinguish
        // that from the `require_equals` spelling `push_equals`
        // produces.
        baseline_fuzzy_match: Some(false),
        check_exclude: vec!["fixtures/**".to_string()],
        check_exclude_from: Some(PathBuf::from(".bcacheckignore")),
        ..base_check_args()
    };
    let tier = crate::TierSpec::Soft(Some(0.9));

    let argv = refresh_baseline_argv(&globals, &args, tier);
    let back = parse_refresh_argv(&argv);

    assert_eq!(
        back.selection.paths,
        vec![PathBuf::from("src"), PathBuf::from("tests")],
    );
    assert_eq!(back.selection.include, vec!["*.rs".to_string()]);
    assert_eq!(
        back.selection.exclude,
        vec!["target/**".to_string(), "vendor/**".to_string()],
    );
    assert_eq!(back.selection.language.as_deref(), Some("rust"));
    assert!(back.selection.no_skip_generated);
    assert_eq!(
        back.selection.paths_from.as_deref(),
        Some(Path::new("paths.txt")),
    );
    assert_eq!(
        back.selection.exclude_from.as_deref(),
        Some(Path::new(".bcaignore")),
    );
    assert!(back.selection.no_ignore);
    assert!(back.selection.no_config);
    assert_eq!(
        back.preproc.preproc_data.as_deref(),
        Some(Path::new("preproc.json")),
    );
    assert!(back.tuning.exclude_tests);
    assert_eq!(back.tuning.cyclomatic_count_try, Some(false));
    assert_eq!(back.thresholds, args.thresholds);
    assert_eq!(
        back.config.as_deref(),
        Some(Path::new("bca-thresholds.toml")),
    );
    assert_eq!(back.tier, tier);
    assert!(back.no_suppress);
    assert_eq!(back.baseline_fuzzy_match, Some(false));
    // The refresh *writes* the baseline the gate read, so `--baseline`
    // becomes `--write-baseline` (the two conflict in clap).
    assert_eq!(
        back.write_baseline,
        Some(Some(PathBuf::from("custom-baseline.toml"))),
    );
    assert_eq!(back.baseline, None);
    // Nothing may land in the trailing positional `[PATHS]`. A
    // `require_equals` flag emitted as two elements parses cleanly
    // and dumps its value here, which every assertion above would
    // otherwise miss.
    assert!(
        back.positional.positional_paths.is_empty(),
        "stray positional argument in {argv:?}"
    );
}

#[test]
fn refresh_baseline_argv_mirrors_the_gate_only_excludes() {
    // `apply_check_exclude` runs before `write_check_baseline` (#378),
    // so a refresh that drops `--check-exclude` records exactly the
    // violations the gate deliberately exempted. Asserted apart from
    // the full round-trip so losing this mirroring fails a test that
    // names it.
    let globals = globals_for_remediation(&["."], &[], None);
    let args = CheckArgs {
        check_exclude: vec!["fixtures/**".to_string(), "generated/**".to_string()],
        check_exclude_from: Some(PathBuf::from(".bcacheckignore")),
        ..base_check_args()
    };
    let back = parse_refresh_argv(&refresh_baseline_argv(
        &globals,
        &args,
        crate::TierSpec::Hard,
    ));
    assert_eq!(back.check_exclude, args.check_exclude);
    assert_eq!(
        back.check_exclude_from.as_deref(),
        Some(Path::new(".bcacheckignore")),
    );
}

#[test]
fn refresh_baseline_argv_defaults_paths_and_baseline_when_unset() {
    // `--paths .` is the walker's implicit default; the remediation
    // block must print it explicitly so the user can copy-paste
    // without thinking about which directory `bca` would have walked
    // by default. With no `--baseline`, the write target is the
    // documented default file.
    let globals = globals_for_remediation(&[], &[], None);
    let args = check_args_for_remediation(None, None, false);
    let argv = refresh_baseline_argv(&globals, &args, crate::TierSpec::Hard);
    let back = parse_refresh_argv(&argv);
    assert_eq!(back.selection.paths, vec![PathBuf::from(".")]);
    assert_eq!(
        back.write_baseline,
        Some(Some(PathBuf::from(".bca-baseline.toml"))),
    );
    // Flags the gate never set stay off the command rather than being
    // emitted at their defaults. This has to be asserted on the argv:
    // clap re-materialises every default on the way back, so
    // `back.tier == Hard` holds whether or not a redundant
    // `--tier=hard` was printed.
    //
    // Asserted as the *whole* argv, not as a deny-list of names. A
    // deny-list only guards the flags someone thought to list — with
    // four of the sixteen conditionally-emitted flags named, pinning
    // `--cyclomatic-count-try=false` and `--baseline-fuzzy-match=true`
    // onto a run that set neither passed. Any flag emitted
    // unconditionally now fails here, including one added later.
    let rendered: Vec<&str> = argv.iter().map(String::as_str).collect();
    assert_eq!(
        rendered,
        vec![
            "bca",
            "check",
            "--paths",
            ".",
            "--write-baseline",
            ".bca-baseline.toml",
        ],
    );
}

#[test]
fn refresh_baseline_argv_renders_every_tier_spelling() {
    // `--tier` is the one mirrored flag whose argv form is not a
    // straight copy of a field: the hard default prints nothing, a
    // bare soft tier prints `soft`, and a pinned ratio prints
    // `soft=<r>`. A refresh that drops the tier silently writes a
    // hard-tier baseline for a soft-tier gate; one that mis-spells it
    // fails to parse, which is #1243 again.
    let globals = globals_for_remediation(&["."], &[], None);
    let args = check_args_for_remediation(None, None, false);
    let cases = [
        (crate::TierSpec::Hard, None),
        (crate::TierSpec::Soft(None), Some("--tier=soft")),
        (crate::TierSpec::Soft(Some(0.9)), Some("--tier=soft=0.9")),
    ];
    for (tier, expected) in cases {
        let argv = refresh_baseline_argv(&globals, &args, tier);
        let emitted = argv.iter().find(|a| a.starts_with("--tier"));
        assert_eq!(
            emitted.map(String::as_str),
            expected,
            "wrong --tier element for {tier:?}, got: {argv:?}"
        );
        assert_eq!(
            parse_refresh_argv(&argv).tier,
            tier,
            "{tier:?} did not survive the parser"
        );
    }
}

#[test]
fn refresh_baseline_argv_pairs_each_switch_with_its_own_flag() {
    // Four presence-only walk switches emit four distinct flags. A
    // fixture that sets them all alike cannot tell the pairings
    // apart: transposing two `push_switch` value arguments produces
    // byte-identical argv. Turning exactly one on at a time makes any
    // transposition fail on both of the flags involved.
    let base = globals_for_remediation(&["."], &[], None);
    let args = check_args_for_remediation(None, None, false);
    let cases = [
        (
            "--no-skip-generated",
            GlobalOpts {
                no_skip_generated: true,
                ..base.clone()
            },
        ),
        (
            "--no-ignore",
            GlobalOpts {
                no_ignore: true,
                ..base.clone()
            },
        ),
        (
            "--no-config",
            GlobalOpts {
                no_config: true,
                ..base.clone()
            },
        ),
        (
            "--exclude-tests",
            GlobalOpts {
                exclude_tests: true,
                ..base
            },
        ),
    ];
    let every_switch: Vec<&str> = cases.iter().map(|(flag, _)| *flag).collect();
    for (flag, globals) in &cases {
        let argv = refresh_baseline_argv(globals, &args, crate::TierSpec::Hard);
        let emitted: Vec<&str> = argv
            .iter()
            .map(String::as_str)
            .filter(|a| every_switch.contains(a))
            .collect();
        assert_eq!(
            emitted,
            vec![*flag],
            "setting only {flag} must emit only {flag}, got: {argv:?}"
        );
    }
}

#[test]
fn refresh_baseline_command_shell_quotes_values_needing_it() {
    // A value containing a space, a glob metacharacter, or a quote
    // must survive the paste as one argument; a plain identifier
    // takes the fast no-quote branch so the common case reads
    // naturally.
    //
    // Each `--include` value carries exactly one metacharacter, alone.
    // Every other fixture here pairs its metacharacter with a space or
    // a `*` that forces the slow path by itself, so widening the
    // fast-path allow-list to admit `$`, `~` or `&` changed no test —
    // a value must isolate the character to test it. The empty string
    // is the same gap in the other direction: it takes the fast path
    // only because of an explicit `!s.is_empty()` guard, and unquoted
    // it vanishes from the command entirely.
    let globals = GlobalOpts {
        paths: vec![PathBuf::from("dir with space"), PathBuf::from("src")],
        exclude: vec!["sub/**".to_string(), "it's/**".to_string()],
        include: vec![
            "$HOME".to_string(),
            "~".to_string(),
            "a&b".to_string(),
            String::new(),
        ],
        ..GlobalOpts::default()
    };
    let args = check_args_for_remediation(None, None, false);
    let cmd = refresh_baseline_command(&globals, &args, crate::TierSpec::Hard);
    assert!(
        cmd.contains("--paths 'dir with space'"),
        "expected single-quoted spaced path, got: {cmd}"
    );
    assert!(
        cmd.contains("--paths src"),
        "expected unquoted simple path, got: {cmd}"
    );
    assert!(
        cmd.contains("--exclude 'sub/**'"),
        "a glob must be quoted so the shell does not expand it, got: {cmd}"
    );
    assert!(
        cmd.contains(r"--exclude 'it'\''s/**'"),
        "an embedded quote must be escaped POSIX-style, got: {cmd}"
    );
    for expected in [
        "--include '$HOME'",
        "--include '~'",
        "--include 'a&b'",
        "--include ''",
    ] {
        assert!(cmd.contains(expected), "expected {expected}, got: {cmd}");
    }
}

/// A path `bca` walked but cannot spell as UTF-8 becomes a visibly
/// broken placeholder rather than a `to_string_lossy` form that points
/// at some other file. The placeholder contains `<` and spaces, so it
/// must reach the reader *quoted* — unquoted, a shell parses it as
/// input redirection and the command fails in a way that reads as a
/// `bca` bug. Unix-only: `OsStr` is byte-addressable only there.
#[cfg(unix)]
#[test]
fn refresh_baseline_command_quotes_the_non_utf8_path_placeholder() {
    use std::os::unix::ffi::OsStrExt as _;
    let globals = GlobalOpts {
        paths: vec![PathBuf::from(std::ffi::OsStr::from_bytes(b"lib\xffdir"))],
        ..GlobalOpts::default()
    };
    let args = check_args_for_remediation(None, None, false);
    let argv = refresh_baseline_argv(&globals, &args, crate::TierSpec::Hard);
    assert!(
        argv.iter().any(|a| a.starts_with("<non-UTF-8 path:")),
        "expected a placeholder argv element, got: {argv:?}"
    );
    let cmd = refresh_baseline_command(&globals, &args, crate::TierSpec::Hard);
    assert!(
        cmd.contains("--paths '<non-UTF-8 path:"),
        "the placeholder must be quoted, got: {cmd}"
    );
}

/// The rendered string is what a user pastes, and a shell — not clap —
/// decides where its argument boundaries fall. Split it with the real
/// thing so the quoting half of the builder is held to the same
/// standard as the flag half: `sh` word-splits the command, `printf`
/// echoes one argument per line, and the result goes back through the
/// parser.
///
/// POSIX-only by construction — `shell_quote`'s doc comment says as
/// much — so the gate is on the `fn`, not on an inner block, and the
/// test is absent rather than vacuous elsewhere.
#[cfg(unix)]
#[test]
fn refresh_baseline_command_survives_a_real_shell_round_trip() {
    let globals = GlobalOpts {
        paths: vec![PathBuf::from("dir with space")],
        exclude: vec!["sub/**".to_string(), "it's/**".to_string()],
        // Values a shell would otherwise substitute away: `$(...)`
        // reaching the parser as anything but these literal bytes
        // means the quoting is injectable, not merely wrong. A bare
        // `$HOME` is the same claim without the parentheses, which
        // force the slow path on their own and so hide a fast path
        // widened to admit `$`. The empty string tests the other
        // fast-path guard: unquoted it is no word at all, and every
        // later argument shifts left.
        include: vec![
            "$(echo pwned)".to_string(),
            "$HOME".to_string(),
            String::new(),
        ],
        ..GlobalOpts::default()
    };
    let args = check_args_for_remediation(None, None, false);
    let cmd = refresh_baseline_command(&globals, &args, crate::TierSpec::Hard);

    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("printf '%s\\n' {cmd}"))
        .output()
        .expect("spawn sh");
    assert!(
        out.status.success(),
        "sh rejected the rendered command: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    let argv: Vec<String> = String::from_utf8(out.stdout)
        .expect("utf8")
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(
        argv,
        refresh_baseline_argv(&globals, &args, crate::TierSpec::Hard),
        "the shell must recover exactly the argv the builder produced",
    );

    let back = parse_refresh_argv(&argv);
    assert_eq!(back.selection.paths, vec![PathBuf::from("dir with space")]);
    assert_eq!(
        back.selection.exclude,
        vec!["sub/**".to_string(), "it's/**".to_string()],
    );
    assert_eq!(
        back.selection.include,
        vec![
            "$(echo pwned)".to_string(),
            "$HOME".to_string(),
            String::new(),
        ],
    );
}

#[test]
fn remediation_block_flags_a_stdin_list_it_cannot_replay() {
    // A `-` list flag consumed a pipe that is gone by the time anyone
    // reads the log, so the printed command needs a caveat no other
    // flag does. All three sources are covered: dropping any one of
    // them from `any_list_flag_read_stdin` leaves that flag's users
    // with an unreplayable command and no warning.
    let caveat = "read stdin";
    let args = check_args_for_remediation(None, None, false);
    let base = globals_for_remediation(&["."], &[], Some(".bcaignore"));

    let mut from_paths = base.clone();
    from_paths.paths_from = Some(PathBuf::from("-"));
    let mut from_exclude = base.clone();
    from_exclude.exclude_from = Some(PathBuf::from("-"));
    let piped_check_exclude = CheckArgs {
        check_exclude_from: Some(PathBuf::from("-")),
        ..check_args_for_remediation(None, None, false)
    };

    for (label, globals, args) in [
        ("--paths-from", &from_paths, &args),
        ("--exclude-from", &from_exclude, &args),
        ("--check-exclude-from", &base, &piped_check_exclude),
    ] {
        let out = format_remediation_block(globals, args, crate::TierSpec::Hard)
            .expect("remediation present");
        assert!(
            out.contains(caveat),
            "{label} - must be called out, got:\n{out}"
        );
    }

    // The same fixture with every list flag file-backed must not warn.
    let quiet =
        format_remediation_block(&base, &args, crate::TierSpec::Hard).expect("remediation present");
    assert!(
        !quiet.contains(caveat),
        "a file-backed list replays fine and needs no caveat, got:\n{quiet}"
    );
}

#[test]
fn artifact_link_for_without_env_suggests_report_command() {
    // Pin the NONE branch deterministically via the pure inner
    // function. The previous version of this test read env directly
    // and skipped when GHA env vars happened to be set — which
    // inverted coverage on the workflow runner that actually
    // exercises the SOME branch in production.
    //
    // Outside GitHub Actions there is no artifact and no run, so the
    // fallback must NOT claim one exists (#676); it points at the
    // local `bca report` view instead.
    let local = "run `bca report` to see them locally";
    assert_eq!(artifact_link_for(None, None), local);
    // Empty-string env values (rare but observed) must also count
    // as absent.
    assert_eq!(
        artifact_link_for(Some(String::new()), Some(String::new())),
        local
    );
    // One-set / one-empty also falls through.
    assert_eq!(
        artifact_link_for(Some("dekobon/big-code-analysis".to_string()), None),
        local
    );
    // Regression guard for #676: the local fallback must never imply
    // a CI upload exists.
    assert!(
        !artifact_link_for(None, None).contains("artifact"),
        "local fallback must not claim a CI artifact exists"
    );
}

#[test]
fn artifact_link_for_with_env_builds_run_url() {
    // Pin the SOME branch — both env values present produces a
    // clickable URL the user can paste into a browser.
    let link = artifact_link_for(
        Some("dekobon/big-code-analysis".to_string()),
        Some("12345".to_string()),
    );
    assert_eq!(
        link,
        "bca-reports artifact at https://github.com/dekobon/big-code-analysis/actions/runs/12345"
    );
}

/// Round-trippable: serializing an `EffectiveConfig` to TOML and
/// re-parsing it through the same `ThresholdConfig` schema used by
/// `--config` must reproduce the original `[thresholds]` table
/// exactly. Guards against future serializer changes (e.g. omitting
/// fields, renaming keys, changing the float repr) silently breaking
/// the documented "pipe back through `--config`" contract.
#[test]
fn effective_config_toml_roundtrips_through_threshold_config_schema() {
    let mut thresholds = BTreeMap::new();
    thresholds.insert("cyclomatic".to_owned(), 22.0);
    thresholds.insert("loc.sloc".to_owned(), 300.0);
    thresholds.insert("halstead.volume".to_owned(), 1_000.0);

    let effective = EffectiveConfig {
        thresholds: EffectiveThresholds {
            global: thresholds.clone(),
            lang: BTreeMap::new(),
        },
        check: EffectiveCheck {
            paths: vec!["src/".to_owned()],
            include: vec!["*.rs".to_owned()],
            exclude: vec!["target/".to_owned()],
            manifest_exclude: Vec::new(),
            exclude_from: None,
            manifest_exclude_from: None,
            check_exclude: Vec::new(),
            manifest_check_exclude: Vec::new(),
            check_exclude_from: None,
            manifest_check_exclude_from: None,
            paths_from: None,
            baseline: None,
            config: None,
            manifest: None,
            no_fail: false,
            no_suppress: false,
            report_suppressed: false,
            no_ignore: false,
            no_skip_generated: false,
            strict: false,
            exclude_tests: true,
            changed_only: false,
            since: None,
            headroom: None,
            tier: "hard",
            exit_codes: "default",
            baseline_line_tolerance: None,
            baseline_fuzzy_match: false,
        },
    };

    let toml_text = toml::to_string_pretty(&effective).expect("serialize EffectiveConfig to TOML");
    let reparsed: crate::thresholds::ThresholdConfig =
        toml::from_str(&toml_text).expect("re-parse via ThresholdConfig schema");
    // `ThresholdConfig.thresholds` holds raw TOML values (so a nested
    // `[thresholds.soft]` coexists with the scalars); split it back into
    // the hard layer to compare against the f64 limits we serialized.
    let reparsed_hard = crate::thresholds::split_thresholds_table(&reparsed.thresholds)
        .expect("split reparsed thresholds")
        .hard;
    assert_eq!(
        reparsed_hard, thresholds,
        "roundtripped thresholds must match input"
    );
}

/// JSON output must contain the resolved `cyclomatic` value with the
/// same field-name shape as TOML, so tooling pipelines can switch
/// between formats without remapping keys.
#[test]
fn effective_config_json_serializes_threshold_overrides() {
    let mut thresholds = BTreeMap::new();
    thresholds.insert("cyclomatic".to_owned(), 22.0);
    let effective = EffectiveConfig {
        thresholds: EffectiveThresholds {
            global: thresholds,
            lang: BTreeMap::new(),
        },
        check: EffectiveCheck {
            paths: Vec::new(),
            include: Vec::new(),
            exclude: Vec::new(),
            manifest_exclude: Vec::new(),
            exclude_from: None,
            manifest_exclude_from: None,
            check_exclude: Vec::new(),
            manifest_check_exclude: Vec::new(),
            check_exclude_from: None,
            manifest_check_exclude_from: None,
            paths_from: None,
            baseline: None,
            config: None,
            manifest: None,
            no_fail: false,
            no_suppress: false,
            report_suppressed: false,
            no_ignore: false,
            no_skip_generated: false,
            strict: false,
            exclude_tests: false,
            changed_only: false,
            since: None,
            headroom: None,
            tier: "hard",
            exit_codes: "default",
            baseline_line_tolerance: None,
            baseline_fuzzy_match: false,
        },
    };
    let json = serde_json::to_string(&effective).expect("serialize EffectiveConfig to JSON");
    assert!(
        json.contains("\"cyclomatic\":22.0"),
        "JSON must contain cyclomatic threshold: {json}"
    );
    assert!(
        json.contains("\"thresholds\""),
        "JSON must contain thresholds table: {json}"
    );
    assert!(
        json.contains("\"check\""),
        "JSON must contain check table: {json}"
    );
}

/// `EffectiveConfig::from_resolved` must reflect both `--config` and
/// `--threshold` layers via the resolved `ThresholdSet` rather than
/// re-reading either source. This is the contract that lets the
/// printer stay agnostic of future layers (#373 headroom, #375 soft
/// thresholds, #381 baseline, #385 tiered exit codes) — they plug
/// into `ThresholdSet::build`, the printer just observes the result.
#[test]
fn effective_config_reflects_resolved_threshold_set() {
    let mut merged = BTreeMap::new();
    merged.insert("cyclomatic".to_owned(), 11.0);
    merged.insert("cognitive".to_owned(), 13.0);
    let set = crate::thresholds::ThresholdSet::build(&merged).expect("build threshold set");

    let globals = GlobalOpts {
        paths: vec![PathBuf::from("src/")],
        include: vec!["*.rs".to_owned()],
        exclude: vec!["target/".to_owned()],
        exclude_tests: true,
        ..GlobalOpts::default()
    };
    let args = check_args_for_remediation(None, None, false);

    let resolved = LanguageThresholds::new(set, BTreeMap::new());
    let effective = EffectiveConfig::from_resolved(
        &globals,
        &args,
        &resolved,
        None,
        crate::TierSpec::Hard,
        false,
    );
    assert_eq!(effective.thresholds.global.get("cyclomatic"), Some(&11.0));
    assert_eq!(effective.thresholds.global.get("cognitive"), Some(&13.0));
    assert!(
        effective.thresholds.lang.is_empty(),
        "no per-language overrides were configured"
    );
    assert_eq!(effective.check.paths, vec!["src/".to_owned()]);
    assert_eq!(effective.check.include, vec!["*.rs".to_owned()]);
    assert!(effective.check.exclude_tests);
}

// --- Headroom scaling (#373) ---

/// `scale_threshold` must trim the float-multiplication artifact so the
/// emitted limit is the same readable value the Python helper produced
/// (`{:.6g}`). The canonical case is `nargs = 7`: `7 * 0.95` is
/// `6.6499999999999995` in IEEE-754, which must round to exactly `6.65`.
#[test]
#[allow(clippy::float_cmp)] // The exact rounded output is the contract under test.
fn scale_threshold_trims_float_artifact_to_six_sig_figs() {
    assert_eq!(scale_threshold(7.0, 0.95, false), 6.65);
}

/// The direction of the scaling is the whole of #1166, so pin both
/// arithmetics in one test: a higher-is-worse limit is a ceiling and
/// must come *down*; a lower-is-worse `mi.*` limit is a floor and must
/// go *up*. Asserted together so a future "simplify" cannot invert both
/// and stay green — which is exactly what a single-direction test would
/// have allowed.
#[test]
#[allow(clippy::float_cmp)] // The exact resolved limits are the contract.
fn scale_threshold_raises_a_floor_and_lowers_a_ceiling() {
    // Ceiling: 15 * 0.9. Exact in decimal, and the float product is
    // exact too, so this is a bit-exact expectation.
    assert_eq!(scale_threshold(15.0, 0.9, false), 13.5);
    // Floor: 20 / 0.9 = 22.222…, above the hard floor it derives from.
    // Rounded up at 6 significant figures (see `scale_threshold`).
    assert_eq!(scale_threshold(20.0, 0.9, true), 22.2223);
    // The bug this pins: the floor must never land at or below its own
    // hard limit, which `20 * 0.9 == 18` did.
    assert!(scale_threshold(20.0, 0.9, true) > 20.0);
}

/// The floor's last significant figure rounds **up**, never down: a
/// floor rounded down is a band that fires marginally late. Pins the
/// direction with a quotient whose 6-sig-fig nearest-rounding would go
/// the other way (`21.05263…` → `21.0526` to nearest, `21.0527` up).
#[test]
#[allow(clippy::float_cmp)] // The rounding direction is the contract.
fn scale_threshold_floor_rounds_up_not_to_nearest() {
    let resolved = scale_threshold(20.0, 0.95, true);
    assert_eq!(resolved, 21.0527);
    assert!(
        resolved >= 20.0 / 0.95,
        "a floor must not resolve below the exact quotient; got {resolved}"
    );
}

/// Exact products must pass through untouched: `50_000 * 0.95` is
/// representable as `47_500.0`, so rounding must not perturb it. Pins
/// that the 6-sig-fig rounding preserves full precision for the
/// largest threshold in `bca.toml` (`halstead.effort`).
#[test]
#[allow(clippy::float_cmp)] // The exact rounded output is the contract under test.
fn scale_threshold_preserves_large_exact_products() {
    assert_eq!(scale_threshold(50_000.0, 0.95, false), 47_500.0);
}

/// `ratio == 1.0` is the documented no-op: every limit must survive
/// scaling byte-for-byte so `--headroom 1.0` is a true parity run with
/// the hard gate.
#[test]
#[allow(clippy::float_cmp)] // ratio == 1.0 must be a bit-exact identity.
fn scale_threshold_ratio_one_is_identity() {
    // `8.3` is the case a grid-exact list cannot see: its double sits a
    // hair above the decimal, so `8.3 * 1e5` is `830000.0000000001` and
    // a bare `ceil` in the floor direction promotes it to `8.30001` —
    // a soft floor strictly *above* the hard one on a run documented as
    // parity. Every other limit here is already on the sig-fig grid, so
    // it passes whether or not the snap exists.
    for &limit in &[0.0, 7.0, 8.3, 15.0, 300.0, 50_000.0] {
        for lower_is_worse in [false, true] {
            assert_eq!(
                scale_threshold(limit, 1.0, lower_is_worse),
                limit,
                "limit {limit} lower_is_worse {lower_is_worse}",
            );
        }
    }
}

/// A configured limit of `0` ("no value permitted") must stay `0`
/// after scaling — `log10(0)` is `-inf`, so the degenerate input has
/// to be short-circuited rather than fed through the magnitude maths.
#[test]
#[allow(clippy::float_cmp)] // A zero limit must stay bit-exact zero.
fn scale_threshold_zero_limit_stays_zero() {
    assert_eq!(scale_threshold(0.0, 0.5, false), 0.0);
    // `0 / 0.5` is also zero, so the floor direction hits the same
    // short-circuit rather than the `log10(0) == -inf` maths.
    assert_eq!(scale_threshold(0.0, 0.5, true), 0.0);
}

/// A subnormal-range limit must not poison the result with `NaN`: the
/// sig-fig `factor` overflows to infinity for such inputs, so the
/// function returns the scaled value unrounded. No real threshold is
/// this small, but `scale_threshold` must stay total — a `NaN` would
/// later be rejected by `ThresholdSet::build` with a confusing error.
#[test]
fn scale_threshold_subnormal_limit_stays_finite() {
    for lower_is_worse in [false, true] {
        let scaled = scale_threshold(1e-320, 0.5, lower_is_worse);
        assert!(scaled.is_finite(), "expected finite, got {scaled}");
    }
}

/// Minimal `CheckArgs` carrying only the `[check.exclude]` inputs under
/// test (#378); every other field takes its inert default so the helper
/// stays focused on what `apply_check_exclude` reads.
fn check_args_excluding(exclude: &[&str], exclude_from: Option<&str>) -> CheckArgs {
    CheckArgs {
        check_exclude: exclude.iter().map(|s| (*s).to_owned()).collect(),
        check_exclude_from: exclude_from.map(PathBuf::from),
        ..base_check_args()
    }
}

#[test]
fn apply_check_exclude_drops_matching_paths_only() {
    let violations = vec![
        violation("src/languages/language_rust.rs", "dispatch", 30.0, 10.0),
        violation("tests/fixtures/big.rs", "fixture", 25.0, 10.0),
        violation("src/metrics/cognitive.rs", "compute", 20.0, 10.0),
    ];
    let args = check_args_excluding(&["src/languages/language_*.rs", "tests/**"], None);
    let kept = apply_check_exclude(violations, &args, Vec::new());

    // The two structural-exemption files are dropped; the genuine
    // offender in `src/metrics` survives.
    assert_eq!(kept.len(), 1, "kept: {kept:?}");
    assert_eq!(kept[0].path, PathBuf::from("src/metrics/cognitive.rs"));
}

#[test]
fn apply_check_exclude_no_patterns_is_identity() {
    // The fast path must not perturb the input when nothing is excluded
    // — same length, same order.
    let violations = vec![
        violation("a.rs", "f", 20.0, 10.0),
        violation("b.rs", "g", 30.0, 10.0),
    ];
    let args = check_args_excluding(&[], None);
    let kept = apply_check_exclude(violations, &args, Vec::new());
    assert_eq!(kept.len(), 2);
    assert_eq!(kept[0].path, PathBuf::from("a.rs"));
    assert_eq!(kept[1].path, PathBuf::from("b.rs"));
}

#[test]
fn apply_check_exclude_reads_patterns_from_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let ignore = dir.path().join(".bcacheckignore");
    // `.gitignore`-style: a comment, a blank line, and one real glob.
    std::fs::write(&ignore, "# structural exemptions\n\ntests/**\n").unwrap();

    let violations = vec![
        violation("tests/fixtures/big.rs", "fixture", 25.0, 10.0),
        violation("src/lib.rs", "f", 20.0, 10.0),
    ];
    let args = check_args_excluding(&[], ignore.to_str());
    let kept = apply_check_exclude(violations, &args, Vec::new());

    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].path, PathBuf::from("src/lib.rs"));
}

#[test]
fn apply_check_exclude_unions_flag_and_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let ignore = dir.path().join(".bcacheckignore");
    std::fs::write(&ignore, "tests/**\n").unwrap();

    let violations = vec![
        violation("tests/fixtures/big.rs", "fixture", 25.0, 10.0),
        violation("xtask/src/main.rs", "render", 30.0, 10.0),
        violation("src/lib.rs", "f", 20.0, 10.0),
    ];
    // Flag contributes `xtask/**`; the file contributes `tests/**`; the
    // two deny-sets union, so only `src/lib.rs` survives.
    let args = check_args_excluding(&["xtask/**"], ignore.to_str());
    let kept = apply_check_exclude(violations, &args, Vec::new());

    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].path, PathBuf::from("src/lib.rs"));
}

// -- Tiered exit-code classification (#385) -------------------------------

/// Build a `(Violation, Option<Coverage>)` pair for the classifier
/// tests: `value` drives hard-breach detection; `coverage` selects the
/// new/regressed bucket (`None` models "no `--baseline` supplied").
///
/// `violation` stamps `hard_limit` from the same `10.0`, which is the
/// hard-tier shape (soft limit == hard ceiling). Soft-tier tests that
/// need the two to differ set `hard_limit` themselves.
fn pair(value: f64, coverage: Option<Coverage>) -> (Violation, Option<Coverage>) {
    (violation("a.rs", "f", value, 10.0), coverage)
}

#[test]
fn classify_empty_pairs_is_clean() {
    let outcome = classify_check_outcome(&[], Tier::Hard);
    assert_eq!(outcome, CheckOutcome::Clean);
}

#[test]
fn classify_no_baseline_is_new_only() {
    // Without `--baseline` every violation carries `None` coverage and
    // counts as a new offender — there is nothing baselined to regress
    // against.
    let pairs = [pair(20.0, None), pair(30.0, None)];
    let outcome = classify_check_outcome(&pairs, Tier::Hard);
    assert_eq!(outcome, CheckOutcome::NewOnly);
}

#[test]
fn classify_new_variant_is_new_only() {
    let pairs = [pair(20.0, Some(Coverage::New))];
    let outcome = classify_check_outcome(&pairs, Tier::Hard);
    assert_eq!(outcome, CheckOutcome::NewOnly);
}

#[test]
fn classify_regressed_only() {
    let pairs = [
        pair(20.0, Some(Coverage::Regressed { recorded: 15.0 })),
        pair(30.0, Some(Coverage::Regressed { recorded: 25.0 })),
    ];
    let outcome = classify_check_outcome(&pairs, Tier::Hard);
    assert_eq!(outcome, CheckOutcome::RegressionOnly);
}

#[test]
fn classify_mixed_new_and_regression() {
    let pairs = [
        pair(20.0, Some(Coverage::New)),
        pair(30.0, Some(Coverage::Regressed { recorded: 25.0 })),
    ];
    let outcome = classify_check_outcome(&pairs, Tier::Hard);
    assert_eq!(outcome, CheckOutcome::Mixed);
}

#[test]
fn classify_soft_tier_hard_breach_escalates_over_regression() {
    // Soft tier, value 12 over the hard ceiling 10: a true breach, more
    // urgent than the regression bucket it would otherwise land in.
    let pairs = [pair(12.0, Some(Coverage::Regressed { recorded: 11.0 }))];
    let outcome = classify_check_outcome(&pairs, Tier::Soft);
    assert_eq!(outcome, CheckOutcome::HardBreach);
}

#[test]
fn classify_soft_tier_encroachment_is_not_hard_breach() {
    // Soft tier, value 8: over the soft band (the gate already kept it)
    // but under the hard ceiling 10 — encroachment, not a breach.
    let pairs = [pair(8.0, Some(Coverage::New))];
    let outcome = classify_check_outcome(&pairs, Tier::Soft);
    assert_eq!(outcome, CheckOutcome::NewOnly);
}

#[test]
fn classify_hard_tier_never_escalates_to_breach() {
    // At the hard tier every violation is over the hard limit, so the
    // breach escalation is suppressed and the new/regr split survives.
    let pairs = [pair(20.0, Some(Coverage::Regressed { recorded: 15.0 }))];
    let outcome = classify_check_outcome(&pairs, Tier::Hard);
    assert_eq!(outcome, CheckOutcome::RegressionOnly);
}

#[test]
fn classify_soft_tier_nan_value_is_not_breach() {
    // A NaN metric value yields `NaN > hard == false`, so it never
    // escalates to a hard breach; it falls to the new/regr split. Pins
    // the documented defensive branch in `classify_check_outcome`.
    let pairs = [pair(f64::NAN, Some(Coverage::New))];
    let outcome = classify_check_outcome(&pairs, Tier::Soft);
    assert_eq!(outcome, CheckOutcome::NewOnly);
}

#[test]
fn classify_soft_tier_metric_without_hard_ceiling_is_not_breach() {
    // A `[thresholds.soft]` absolute limit with no `[thresholds]`
    // counterpart leaves the metric with no hard ceiling to exceed, so
    // however far the value overshoots it stays an encroachment.
    let mut pairs = [pair(999.0, Some(Coverage::New))];
    pairs[0].0.hard_limit = None;
    let outcome = classify_check_outcome(&pairs, Tier::Soft);
    assert_eq!(outcome, CheckOutcome::NewOnly);
}

/// Build a `(Violation, Option<Coverage>)` pair for a lower-is-worse
/// `mi.original` metric (#837): a value *below* the floor is the breach.
///
/// The soft floor is 50 and the hard floor 10 — the realistic soft-tier
/// shape for a lower-is-worse metric, where the early-warning floor sits
/// *above* the ceiling that constitutes a real breach.
fn pair_low(value: f64, coverage: Option<Coverage>) -> (Violation, Option<Coverage>) {
    let mut v = violation("a.rs", "f", value, 50.0);
    v.metric = "mi.original";
    v.lower_is_worse = true;
    v.hard_limit = Some(10.0);
    (v, coverage)
}

#[test]
fn classify_soft_tier_lower_is_worse_hard_breach_escalates() {
    // mi.original is lower-is-worse with a hard floor of 10. A value of 5
    // is below the floor — a real hard breach — even though 5 > 10 is
    // false. Before #837 the hardcoded `value > hard` missed this and the
    // outcome fell to RegressionOnly, under-reporting the exit code.
    let pairs = [pair_low(5.0, Some(Coverage::Regressed { recorded: 8.0 }))];
    let outcome = classify_check_outcome(&pairs, Tier::Soft);
    assert_eq!(outcome, CheckOutcome::HardBreach);
}

#[test]
fn classify_soft_tier_lower_is_worse_above_floor_is_not_breach() {
    // mi.original value 15 is above the hard floor 10, so it is not a hard
    // breach; it falls to the new/regr split.
    let pairs = [pair_low(15.0, Some(Coverage::New))];
    let outcome = classify_check_outcome(&pairs, Tier::Soft);
    assert_eq!(outcome, CheckOutcome::NewOnly);
}

#[test]
fn exit_code_default_collapses_every_violation_to_two() {
    // The stable contract: any non-clean outcome is exit 2 regardless of
    // category. This is what every existing `exit != 0` integration
    // relies on.
    for outcome in [
        CheckOutcome::NewOnly,
        CheckOutcome::RegressionOnly,
        CheckOutcome::Mixed,
        CheckOutcome::HardBreach,
    ] {
        assert_eq!(outcome.exit_code(false), Some(2), "{outcome:?}");
    }
    assert_eq!(CheckOutcome::Clean.exit_code(false), None);
}

#[test]
fn exit_code_tiered_maps_each_category() {
    assert_eq!(CheckOutcome::Clean.exit_code(true), None);
    assert_eq!(CheckOutcome::NewOnly.exit_code(true), Some(2));
    assert_eq!(CheckOutcome::RegressionOnly.exit_code(true), Some(3));
    assert_eq!(CheckOutcome::Mixed.exit_code(true), Some(4));
    assert_eq!(CheckOutcome::HardBreach.exit_code(true), Some(5));
}

/// `bca init` writes a `bca.toml` manifest whose `[thresholds]` block
/// is rendered from `default_thresholds::DEFAULT_THRESHOLDS`. This
/// guard pins the *assembled* manifest to that table — the round-trip
/// test in `default_thresholds_tests.rs` only covers the rendered block
/// in isolation, so without this one a bad `format!` in
/// [`init_manifest_template`] could drop or mangle the block and no
/// test would notice.
///
/// Until #1140 this test compared the template against the repo-root
/// `bca.toml` instead. That coupling is gone deliberately: the shipped
/// defaults answer "what should an arbitrary project gate at", while
/// the root manifest answers "what does this Rust codebase, with
/// `exclude_tests = true` and a comment-dense house style, gate itself
/// at". They are expected to differ, and asserting they do not is what
/// kept the shipped defaults uncalibrated.
#[test]
fn init_template_thresholds_match_the_default_table() {
    let template: toml::Table =
        toml::from_str(&init_manifest_template()).expect("init manifest template parses");
    let thresholds = template
        .get("thresholds")
        .and_then(toml::Value::as_table)
        .expect("the scaffolded manifest has a [thresholds] table");
    let expected: toml::Table = crate::default_thresholds::DEFAULT_THRESHOLDS
        .iter()
        .map(|e| (e.name.to_string(), toml::Value::Integer(i64::from(e.limit))))
        .collect();
    assert_eq!(
        *thresholds, expected,
        "the `bca init` scaffold's [thresholds] drifted from \
         default_thresholds::DEFAULT_THRESHOLDS, which is the single source \
         of truth for the shipped limits"
    );
}

// -- provenance warning (issue #486) -----------------------------------

#[test]
fn provenance_warning_fires_when_stricter_than_baseline() {
    // Soft check at 0.90 reading a baseline written at 0.95: the current
    // run is stricter, so the baseline may under-cover. Must warn, and
    // the message must name both strictness scalars and a refresh recipe.
    let msg = provenance_warning(
        baseline::Provenance::soft_headroom(0.90),
        Some(baseline::Provenance::soft_headroom(0.95)),
    )
    .expect("stricter-than-baseline must warn");
    assert!(
        msg.contains("0.9"),
        "message names current strictness: {msg}"
    );
    assert!(
        msg.contains("0.95"),
        "message names baseline strictness: {msg}"
    );
    assert!(
        msg.contains("--write-baseline"),
        "message points at the refresh recipe: {msg}"
    );
}

#[test]
fn provenance_warning_silent_when_hard_reads_soft() {
    // The repo's own setup: `make self-scan` (hard) reading the soft-0.95
    // baseline must produce NO warning.
    assert_eq!(
        provenance_warning(
            baseline::Provenance::hard(),
            Some(baseline::Provenance::soft_headroom(0.95)),
        ),
        None
    );
}

#[test]
fn provenance_warning_silent_when_baseline_absent() {
    // Pre-v5 baselines (no provenance) must never trigger the warning.
    assert_eq!(provenance_warning(baseline::Provenance::hard(), None), None);
    assert_eq!(
        provenance_warning(baseline::Provenance::soft_headroom(0.5), None),
        None
    );
}

#[test]
fn resolve_provenance_maps_each_tier_branch() {
    // Hard tier: no scaling.
    assert_eq!(
        resolve_provenance(TierSpec::Hard, false),
        baseline::Provenance::hard()
    );
    // Soft with a `[thresholds.soft]` table: per-metric, no single ratio.
    assert_eq!(
        resolve_provenance(TierSpec::Soft(Some(0.5)), true),
        baseline::Provenance::soft_table()
    );
    // Soft without a table: scaled by the spec's ratio.
    assert_eq!(
        resolve_provenance(TierSpec::Soft(Some(0.90)), false),
        baseline::Provenance::soft_headroom(0.90)
    );
    // Soft without a table or a pinned ratio: the default soft headroom.
    assert_eq!(
        resolve_provenance(TierSpec::Soft(None), false),
        baseline::Provenance::soft_headroom(DEFAULT_SOFT_HEADROOM)
    );
}

// Regression test for issue #740: `bca preproc` finalization must not
// panic when a worker poisoned the shared accumulator's mutex. The
// recovered guard still holds every joined worker's file entry, so
// `into_preproc_data` degrades to the accumulated data rather than
// re-panicking on `.expect("mutex not poisoned")`. Verified by revert
// per `.claude/rules/testing.md`: restoring the `.expect()` chain makes
// this test panic instead of recovering.
#[test]
fn into_preproc_data_degrades_on_poisoned_mutex() {
    use big_code_analysis::PreprocFile;
    use std::thread;

    let mut results = PreprocResults::default();
    results
        .files
        .insert(PathBuf::from("a.c"), PreprocFile::default());
    let preproc_lock = Arc::new(Mutex::new(results));

    // Poison the mutex: panic while holding the guard on a helper thread.
    let poisoner = preproc_lock.clone();
    let handle = thread::spawn(move || {
        let _guard = poisoner.lock().expect("fresh mutex is unpoisoned");
        panic!("intentional panic to poison the preproc mutex");
    });
    assert!(
        handle.join().is_err(),
        "poisoner thread should have panicked"
    );
    assert!(
        preproc_lock.is_poisoned(),
        "test setup failed to poison the mutex"
    );

    // Finalization must recover the accumulated data, not panic. The
    // `Arc` here is sole-owned (the poisoner dropped its clone on
    // unwind), so this exercises the `Ok` + poisoned-`into_inner` arm.
    let data = into_preproc_data(preproc_lock);
    assert!(
        data.files.contains_key(&PathBuf::from("a.c")),
        "poison recovery must preserve the accumulated file entries"
    );
}

// `into_preproc_data` must also degrade when the `Arc` is still shared
// (a worker failed to join), taking the data from behind the guard
// rather than panicking on the un-joined-`Arc` `.expect()` (#740).
#[test]
fn into_preproc_data_degrades_on_shared_arc() {
    use big_code_analysis::PreprocFile;

    let mut results = PreprocResults::default();
    results
        .files
        .insert(PathBuf::from("b.c"), PreprocFile::default());
    let preproc_lock = Arc::new(Mutex::new(results));

    // Hold a second clone so `Arc::try_unwrap` returns `Err(shared)`.
    let _still_shared = preproc_lock.clone();

    let data = into_preproc_data(preproc_lock);
    assert!(
        data.files.contains_key(&PathBuf::from("b.c")),
        "shared-Arc recovery must surface the accumulated file entries"
    );
}

// ---------------------------------------------------------------------
// Aggregate `--output` element ordering (#1244)
// ---------------------------------------------------------------------

/// A trivial parsed Rust document carrying `name`.
///
/// The tests below hand the same fixture a *path* that disagrees with
/// its `name`, so an assertion on the emitted order can only hold if
/// `write_aggregate` keys on the emitted path — sorting by `name`, or
/// not sorting at all, produces a different sequence in every case.
fn aggregate_ast(name: &str) -> big_code_analysis::Ast {
    use big_code_analysis::{Ast, LANG, Source};

    Ast::parse(Source::new(LANG::Rust, b"fn f() { let x = 1; }\n").with_name(Some(name.to_owned())))
        .expect("fixture parses")
}

/// The `metrics` element of an [`aggregate_ast`] document.
fn aggregate_space(name: &str) -> FuncSpace {
    aggregate_ast(name)
        .metrics(big_code_analysis::MetricsOptions::default())
        .expect("fixture yields metrics")
}

/// The `ops` element of an [`aggregate_ast`] document.
fn aggregate_ops(name: &str) -> Ops {
    aggregate_ast(name).ops().expect("fixture yields ops")
}

/// `(emitted path, document name)` in a deliberately scrambled source
/// order. The path order (`alpha`, `beta`, `middle`, `zebra`) is not
/// the source order, not the name order, and — because `beta` and
/// `middle` carry deliberately anti-correlated names — not the reversed
/// name order either.
const SCRAMBLED: [(&str, &str); 4] = [
    ("src/zebra.rs", "name-aaa"),
    ("src/alpha.rs", "name-zzz"),
    ("src/middle.rs", "name-yyy"),
    ("src/beta.rs", "name-mmm"),
];

/// The names of [`SCRAMBLED`] in emitted-path order — hand-pinned, and
/// the answer to no other ordering rule: not source order
/// (`aaa, zzz, yyy, mmm`), not name-ascending (`aaa, mmm, yyy, zzz`),
/// not name-descending (`zzz, yyy, mmm, aaa`), not path-descending
/// (`aaa, yyy, mmm, zzz`).
const PATH_ORDERED_NAMES: [&str; 4] = ["name-zzz", "name-mmm", "name-yyy", "name-aaa"];

/// Read the `name` of every element of a JSON aggregate, positionally.
fn aggregate_names(out: &Path) -> Vec<String> {
    let doc: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out).expect("aggregate written"))
            .expect("aggregate is valid JSON");
    doc.as_array()
        .expect("aggregate is a top-level array")
        .iter()
        .map(|elem| {
            elem.get("name")
                .and_then(serde_json::Value::as_str)
                .expect("every element carries a name")
                .to_owned()
        })
        .collect()
}

/// Workers stream their results in completion order, so `metrics
/// --output <FILE>` used to emit a differently-ordered document on
/// every run over an unchanged tree (#1244). The aggregate is sorted by
/// emitted path before serialization.
#[test]
fn write_aggregate_orders_metrics_elements_by_emitted_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("metrics.json");
    let items = SCRAMBLED
        .iter()
        .map(|(path, name)| {
            AggregateItem::Metrics(Box::new(aggregate_space(name)), PathBuf::from(path))
        })
        .collect();

    write_aggregate(Some(MetricsFormat::Json), items, &out, false);

    assert_eq!(
        aggregate_names(&out),
        PATH_ORDERED_NAMES,
        "metrics aggregate elements must be emitted in sorted emitted-path order"
    );
}

/// The `ops` arm of the same contract. `Ops` carries no path of its
/// own, which is why `AggregateItem::Ops` ships one alongside it.
#[test]
fn write_aggregate_orders_ops_elements_by_emitted_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("ops.json");
    let items = SCRAMBLED
        .iter()
        .map(|(path, name)| AggregateItem::Ops(Box::new(aggregate_ops(name)), PathBuf::from(path)))
        .collect();

    write_aggregate(Some(MetricsFormat::Json), items, &out, false);

    assert_eq!(
        aggregate_names(&out),
        PATH_ORDERED_NAMES,
        "ops aggregate elements must be emitted in sorted emitted-path order"
    );
}

/// The CSV arm builds its rows from the same vector, keyed by the
/// emitted path in the first column, so it inherits the same order.
#[test]
fn write_aggregate_orders_csv_rows_by_emitted_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("metrics.csv");
    let items = SCRAMBLED
        .iter()
        .map(|(path, name)| {
            AggregateItem::Metrics(Box::new(aggregate_space(name)), PathBuf::from(path))
        })
        .collect();

    write_aggregate(Some(MetricsFormat::Csv), items, &out, false);

    let text = std::fs::read_to_string(&out).expect("aggregate written");
    let mut lines = text.lines();
    assert!(
        lines.next().is_some_and(|h| h.starts_with("path,")),
        "expected the shared CSV header first: {text}"
    );
    // One file contributes several rows (one per space), so collapse
    // runs of the same path rather than asserting row-for-row.
    let mut paths: Vec<&str> = Vec::new();
    for line in lines {
        let path = line.split(',').next().expect("split yields one field");
        if paths.last() != Some(&path) {
            paths.push(path);
        }
    }
    assert_eq!(
        paths,
        [
            "src/alpha.rs",
            "src/beta.rs",
            "src/middle.rs",
            "src/zebra.rs"
        ],
        "CSV aggregate rows must be grouped in sorted emitted-path order"
    );
}

/// The degenerate sizes: sorting must not turn an empty run into a
/// missing document, nor a one-file run into an empty array. Both are
/// real `--output` invocations (a tree with no analyzable file; a
/// single explicit path).
#[test]
fn write_aggregate_handles_empty_and_single_element_runs() {
    let dir = tempfile::tempdir().expect("tempdir");

    let empty = dir.path().join("empty.json");
    write_aggregate(Some(MetricsFormat::Json), Vec::new(), &empty, false);
    assert_eq!(
        std::fs::read_to_string(&empty).expect("empty aggregate written"),
        "[]",
        "an empty run still writes an empty aggregate array"
    );

    let single = dir.path().join("single.json");
    write_aggregate(
        Some(MetricsFormat::Json),
        vec![AggregateItem::Metrics(
            Box::new(aggregate_space("name-only")),
            PathBuf::from("src/only.rs"),
        )],
        &single,
        false,
    );
    assert_eq!(
        aggregate_names(&single),
        ["name-only"],
        "a single-element run emits exactly that element"
    );
}
