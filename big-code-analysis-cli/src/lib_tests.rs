// Sibling-file unit tests for the CLI library entry points, wired in
// via `#[path = "lib_tests.rs"] mod tests;` so the production `lib.rs`
// stays under the `bca check` per-file metric caps. Matched by the
// `./**/*_tests.rs` rule in `.bcaignore`, so the self-scan walker
// skips this file the same way it skips `./tests/`.

use super::*;

fn test_config(action: Action) -> Config {
    Config {
        action,
        output: None,
        language: None,
        line_start: None,
        line_end: None,
        preproc_lock: None,
        preproc: None,
        count_lock: None,
        markdown_tx: None,
        strip_prefix: String::new(),
        threshold_set: None,
        check_tx: None,
        exemptions_tx: None,
        files_dispatched: None,
        suppression_policy: SuppressionPolicy::Honor,
        warning: false,
        skip_generated: true,
        report_skipped: false,
        exclude_tests: false,
        fuzzy_baseline: false,
    }
}

#[test]
fn process_dir_path_noop_outside_preproc() {
    let cfg = test_config(Action::Dump);
    let mut all_files = HashMap::new();
    process_dir_path(&mut all_files, Path::new("/some/file.cpp"), &cfg);
    assert!(all_files.is_empty());
}

#[test]
fn process_dir_path_inserts_valid_utf8_filename() {
    let cfg = test_config(Action::PreprocProduce);
    let mut all_files = HashMap::new();
    process_dir_path(&mut all_files, Path::new("/some/dir/foo.cpp"), &cfg);
    assert_eq!(all_files.len(), 1);
    assert_eq!(
        all_files["foo.cpp"],
        vec![PathBuf::from("/some/dir/foo.cpp")]
    );
}

#[test]
fn process_dir_path_groups_duplicate_filenames() {
    let cfg = test_config(Action::PreprocProduce);
    let mut all_files = HashMap::new();
    process_dir_path(&mut all_files, Path::new("/a/foo.cpp"), &cfg);
    process_dir_path(&mut all_files, Path::new("/b/foo.cpp"), &cfg);
    assert_eq!(all_files.len(), 1);
    assert_eq!(
        all_files["foo.cpp"],
        vec![PathBuf::from("/a/foo.cpp"), PathBuf::from("/b/foo.cpp")]
    );
}

#[cfg(unix)]
#[test]
fn process_dir_path_skips_non_utf8_filename() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let cfg = test_config(Action::PreprocProduce);
    let mut all_files = HashMap::new();
    let bad_name = OsStr::from_bytes(b"\xff\xfe");
    let path = PathBuf::from("/some/dir").join(bad_name);
    process_dir_path(&mut all_files, &path, &cfg);
    assert!(all_files.is_empty());
}

// CLI parsing tests. The shape is now subcommand-driven, so these
// exercise the shape of the top-level parser, not the legacy flag
// mutual-exclusion rules.

fn parse(args: &[&str]) -> clap::error::Result<Cli> {
    Cli::try_parse_from(std::iter::once(&"cli").chain(args.iter()))
}

#[test]
fn no_subcommand_prints_help() {
    // arg_required_else_help: no args -> clap prints help and exits.
    // We just check parsing fails (either DisplayHelp or MissingSubcommand).
    assert!(parse(&[]).is_err());
}

#[test]
fn metrics_alone_parses() {
    assert!(parse(&["metrics"]).is_ok());
}

#[test]
fn metrics_with_format_parses() {
    assert!(parse(&["metrics", "-O", "json"]).is_ok());
}

// Offender formats (Checkstyle, SARIF, clang-warning,
// msvc-warning) moved from `bca metrics` to
// `bca check --output-format` in issue #235. `MetricsFormat` no
// longer enumerates them, so clap rejects them at parse time on
// `metrics` and `ops`.
#[test]
fn metrics_rejects_checkstyle_format() {
    assert!(parse(&["metrics", "-O", "checkstyle"]).is_err());
}

#[test]
fn metrics_rejects_sarif_format() {
    assert!(parse(&["metrics", "-O", "sarif"]).is_err());
}

#[test]
fn metrics_rejects_clang_warning_format() {
    assert!(parse(&["metrics", "-O", "clang-warning"]).is_err());
}

#[test]
fn metrics_rejects_msvc_warning_format() {
    assert!(parse(&["metrics", "-O", "msvc-warning"]).is_err());
}

#[test]
fn check_accepts_sarif_output_format() {
    assert!(parse(&["check", "--threshold", "cyclomatic=10", "-O", "sarif"]).is_ok());
}

#[test]
fn check_accepts_checkstyle_output_format() {
    assert!(
        parse(&[
            "check",
            "--threshold",
            "cyclomatic=10",
            "--output-format",
            "checkstyle",
        ])
        .is_ok()
    );
}

#[test]
fn check_rejects_per_file_format_as_output_format() {
    // Per-file formats (json, csv, ...) live on `bca metrics`;
    // `bca check` only accepts the offender formats.
    assert!(
        parse(&[
            "check",
            "--threshold",
            "cyclomatic=10",
            "--output-format",
            "json",
        ])
        .is_err()
    );
}

// Note: runtime rejection of `ops -O csv` is covered by
// `ops_rejects_csv_format_at_runtime` in
// tests/action_enforcement.rs, which spawns the binary so the
// dispatcher's die() can be observed.

#[test]
fn metrics_rejects_markdown_format() {
    // ReportFormat::Markdown is not in MetricsFormat by construction.
    assert!(parse(&["metrics", "-O", "markdown"]).is_err());
}

#[test]
fn metrics_rejects_top_flag() {
    // --top lives only on `report`.
    assert!(parse(&["metrics", "--top", "5"]).is_err());
}

#[test]
fn metrics_rejects_strip_prefix_flag() {
    assert!(parse(&["metrics", "--strip-prefix", "/x"]).is_err());
}

#[test]
fn report_markdown_parses() {
    assert!(parse(&["report", "markdown"]).is_ok());
}

#[test]
fn report_html_parses() {
    // Inspect the parsed variant so a future alias / value-rename
    // that maps `html` to `Markdown` cannot pass this test.
    let cli = parse(&["report", "html"]).expect("`report html` parses");
    match cli.command {
        Command::Report(args) => assert_eq!(args.format, ReportFormat::Html),
        other => panic!("expected Command::Report, got {other:?}"),
    }
}

#[test]
fn report_requires_format() {
    assert!(parse(&["report"]).is_err());
}

#[test]
fn report_with_top_and_strip_prefix() {
    assert!(parse(&["report", "markdown", "--top", "10", "--strip-prefix", "/x/"]).is_ok());
}

#[test]
fn report_html_with_top_and_strip_prefix() {
    let cli =
        parse(&["report", "html", "--top", "10", "--strip-prefix", "/x/"]).expect("flags parse");
    match cli.command {
        Command::Report(args) => {
            assert_eq!(args.format, ReportFormat::Html);
            assert_eq!(args.top, 10);
            assert_eq!(args.strip_prefix, "/x/");
        }
        other => panic!("expected Command::Report, got {other:?}"),
    }
}

#[test]
fn report_top_zero_rejected() {
    assert!(parse(&["report", "markdown", "--top", "0"]).is_err());
}

#[test]
fn report_html_top_zero_rejected() {
    assert!(parse(&["report", "html", "--top", "0"]).is_err());
}

#[test]
fn ops_parses() {
    assert!(parse(&["ops", "-O", "json"]).is_ok());
}

#[test]
fn dump_parses() {
    assert!(parse(&["dump"]).is_ok());
}

#[test]
fn find_requires_a_node() {
    assert!(parse(&["find"]).is_err());
    assert!(parse(&["find", "call_expression"]).is_ok());
}

#[test]
fn count_requires_a_node() {
    assert!(parse(&["count"]).is_err());
    assert!(parse(&["count", "if_statement"]).is_ok());
}

#[test]
fn functions_parses() {
    assert!(parse(&["functions"]).is_ok());
}

#[test]
fn strip_comments_parses() {
    assert!(parse(&["strip-comments"]).is_ok());
    assert!(parse(&["strip-comments", "--in-place"]).is_ok());
}

#[test]
fn preproc_parses() {
    assert!(parse(&["preproc"]).is_ok());
    assert!(parse(&["preproc", "-o", "/tmp/x.json"]).is_ok());
}

#[test]
fn list_metrics_parses() {
    let cli = parse(&["list-metrics"]).expect("parses");
    assert!(matches!(cli.command, Command::ListMetrics(_)));
}

#[test]
fn list_metrics_with_descriptions() {
    let cli = parse(&["list-metrics", "descriptions"]).expect("parses");
    match cli.command {
        Command::ListMetrics(args) => assert_eq!(args.mode, ListMetricsMode::Descriptions),
        _ => panic!("expected ListMetrics"),
    }
}

#[test]
fn list_metrics_invalid_mode_rejected() {
    assert!(parse(&["list-metrics", "bogus"]).is_err());
}

#[test]
fn global_paths_works_before_or_after_subcommand() {
    assert!(parse(&["--paths", "x", "metrics"]).is_ok());
    assert!(parse(&["metrics", "--paths", "x"]).is_ok());
}

fn os_args(args: &[&str]) -> Vec<OsString> {
    args.iter().map(|s| OsString::from(*s)).collect()
}

#[test]
fn legacy_hint_recognizes_old_metrics() {
    let hint = legacy_hint(os_args(&["cli", "--metrics", "-O", "markdown"])).expect("hint");
    assert!(hint.contains("report markdown"), "{hint}");
    assert!(hint.contains("--metrics"), "{hint}");
}

#[test]
fn legacy_hint_recognizes_output_format_json_with_legacy_action() {
    // -O json next to --metrics is unambiguously legacy and should
    // map to `bca metrics -O json`.
    let hint = legacy_hint(os_args(&["cli", "-m", "--output-format", "json"])).expect("hint");
    assert!(hint.contains("metrics -O json"), "{hint}");
}

#[test]
fn legacy_hint_returns_none_for_clean_args() {
    // Valid new-CLI args that just happen to also contain `-O` should
    // not trigger a legacy hint.
    let hint = legacy_hint(os_args(&["cli", "metrics", "-O", "json"]));
    assert!(hint.is_none());
}

#[test]
fn legacy_hint_returns_none_for_no_args() {
    let hint = legacy_hint(os_args(&["cli"]));
    assert!(hint.is_none());
}

#[test]
fn legacy_hint_recognizes_dash_o_markdown_alone() {
    // -O markdown is unambiguously legacy: markdown is not a
    // MetricsFormat value, so this pattern can only have come from the
    // pre-restructure CLI.
    let hint = legacy_hint(os_args(&["cli", "-O", "markdown"])).expect("hint");
    assert!(hint.contains("report markdown"), "{hint}");
}

#[test]
fn legacy_hint_redirects_metrics_offender_format_to_check() {
    // Issue #235: `bca metrics -O sarif` is no longer valid — the
    // offender formats live on `bca check` now. The hint should
    // point at the new home.
    let hint = legacy_hint(os_args(&["cli", "metrics", "-O", "sarif"])).expect("hint");
    assert!(hint.contains("bca check"), "{hint}");
    assert!(hint.contains("sarif"), "{hint}");
}

#[test]
fn legacy_hint_redirects_metrics_checkstyle_long_form() {
    let hint = legacy_hint(os_args(&[
        "cli",
        "metrics",
        "--output-format",
        "checkstyle",
    ]))
    .expect("hint");
    assert!(hint.contains("bca check"), "{hint}");
    assert!(hint.contains("checkstyle"), "{hint}");
}

#[test]
fn legacy_hint_redirects_ops_offender_format_to_check() {
    // Same migration story for `bca ops -O <offender>`.
    let hint = legacy_hint(os_args(&["cli", "ops", "-O", "clang-warning"])).expect("hint");
    assert!(hint.contains("bca check"), "{hint}");
    assert!(hint.contains("clang-warning"), "{hint}");
}

#[test]
fn legacy_hint_quiet_for_metrics_with_per_file_format() {
    // `bca metrics -O json` is still valid — no hint should fire.
    let hint = legacy_hint(os_args(&["cli", "metrics", "-O", "json"]));
    assert!(hint.is_none(), "{hint:?}");
}

#[test]
fn legacy_hint_quiet_when_user_invoked_known_subcommand() {
    // `bca find --dump` — user wants `--dump` as a positional node
    // type, not a legacy flag. Presence of a known subcommand (`find`)
    // suppresses the hint; clap's own "to pass '--dump' as a value,
    // use '-- --dump'" tip remains the right guidance.
    let hint = legacy_hint(os_args(&["cli", "find", "--dump"]));
    assert!(hint.is_none());
}

#[test]
fn legacy_hint_recognizes_dash_d() {
    // -d was the short form of --dump in the legacy CLI.
    let hint = legacy_hint(os_args(&["cli", "-d", "--paths", "."])).expect("hint");
    assert!(hint.contains("bca dump"), "{hint}");
}

/// Sanity: `Cli::command()` builds without panicking. Catches misconfigured
/// derive attributes (e.g., conflicting short flags) at test time.
#[test]
fn cli_is_well_formed() {
    use clap::CommandFactory;
    Cli::command().debug_assert();
}

/// `SUBCOMMANDS` (used by `legacy_hint` to gate the migration message)
/// must list every variant of the `Command` enum. If a future verb is
/// added to `Command` and this list is not updated, `legacy_hint` will
/// false-positive on that verb's arguments.
#[test]
fn subcommands_match_command_enum() {
    use clap::CommandFactory;
    use std::collections::HashSet;
    let from_clap: HashSet<String> = Cli::command()
        .get_subcommands()
        .map(|c| c.get_name().to_string())
        .filter(|n| n != "help") // clap auto-generates `help`
        .collect();
    let from_const: HashSet<String> = SUBCOMMANDS.iter().map(|s| (*s).to_string()).collect();
    assert_eq!(
        from_clap,
        from_const,
        "SUBCOMMANDS const drifted from Command enum: \
         missing from const = {missing:?}, missing from enum = {extra:?}",
        missing = from_clap.difference(&from_const).collect::<Vec<_>>(),
        extra = from_const.difference(&from_clap).collect::<Vec<_>>(),
    );
}

#[test]
fn collect_lines_skips_blank_and_comment_lines() {
    // The literal trailing spaces on the last pattern are
    // intentional — they exercise the right-side trim. Keep
    // them; reformatters that strip trailing whitespace on save
    // would weaken the test.
    let input = concat!(
        "# comment at top\n",
        "target/\n",
        "\n",
        "  # indented comment\n",
        "node_modules/\n",
        "\n",
        "\t\n",
        "**/*.snap\n",
        "   tests/repositories/**   \n",
    );
    let got = collect_lines(std::io::Cursor::new(input), "test", exclude_pattern_filter)
        .expect("ASCII fixture decodes cleanly");
    assert_eq!(
        got,
        vec![
            "target/",
            "node_modules/",
            "**/*.snap",
            "tests/repositories/**"
        ],
        "blank lines, comment lines, and surrounding whitespace must all be stripped",
    );
}

#[test]
fn collect_lines_treats_hash_inside_pattern_as_literal() {
    let input = "\na/#weird/path\n#full-line-comment\n";
    let got = collect_lines(std::io::Cursor::new(input), "test", exclude_pattern_filter)
        .expect("ASCII fixture decodes cleanly");
    assert_eq!(
        got,
        vec!["a/#weird/path"],
        "only lines whose first non-whitespace char is `#` count as comments",
    );
}

#[test]
fn collect_lines_returns_empty_for_only_blanks_and_comments() {
    let input = "\n# only comments\n\t  \n# another\n";
    let got = collect_lines(std::io::Cursor::new(input), "test", exclude_pattern_filter)
        .expect("ASCII fixture decodes cleanly");
    assert!(got.is_empty(), "expected empty Vec, got {got:?}");
}

#[test]
fn collect_lines_strips_bom_on_inner_lines_not_just_first() {
    // BOM on the third pattern line. The doc comment for
    // `collect_lines` promises per-line BOM stripping; this
    // pins it. A regression that limited stripping to line 0
    // would leave `\u{feff}**/inner.py` as a literal-U+FEFF
    // glob and the assertion below would fail.
    let input = "**/a.py\n**/b.py\n\u{feff}**/inner.py\n";
    let got = collect_lines(std::io::Cursor::new(input), "test", exclude_pattern_filter)
        .expect("ASCII fixture decodes cleanly");
    assert_eq!(
        got,
        vec!["**/a.py", "**/b.py", "**/inner.py"],
        "BOM on an inner line must be stripped, not just on line 0",
    );
}

#[test]
fn collect_lines_strips_trailing_bom() {
    // Trailing BOM (e.g. from a concatenated or
    // half-broken-editor file). `trim_matches` with a
    // BOM-or-whitespace predicate must strip it from the end
    // too — otherwise the pattern carries a literal U+FEFF
    // suffix matching no real path.
    let input = "**/a.py\u{feff}\n";
    let got = collect_lines(std::io::Cursor::new(input), "test", exclude_pattern_filter)
        .expect("ASCII fixture decodes cleanly");
    assert_eq!(got, vec!["**/a.py"], "trailing BOM must be stripped");
}

#[test]
fn collect_lines_handles_bom_then_whitespace_then_pattern() {
    // `\u{feff}  **/foo.rs` — the order-sensitive
    // `trim().trim_start_matches('\u{feff}')` chain used to
    // leave literal leading spaces here because `trim()` stops
    // at the non-whitespace BOM. The fixed implementation
    // treats whitespace and BOM as one character class.
    let input = "\u{feff}  **/foo.rs\n";
    let got = collect_lines(std::io::Cursor::new(input), "test", exclude_pattern_filter)
        .expect("ASCII fixture decodes cleanly");
    assert_eq!(
        got,
        vec!["**/foo.rs"],
        "BOM-then-whitespace combinations must strip cleanly with no literal leading spaces",
    );
}

#[test]
fn path_pattern_filter_keeps_hash_prefixed_lines_as_literal_paths() {
    // Pins the doc claim on `read_paths_from`: `#` is a path
    // character, not a comment. The test calls
    // `path_pattern_filter` directly so a refactor that
    // accidentally swapped in `exclude_pattern_filter` (the two
    // are adjacent and share the signature) would silently
    // filter `#`-prefixed paths AND fail this test.
    let input = "/tmp/normal/path\n#weird-but-valid-path\n";
    let got = collect_lines(std::io::Cursor::new(input), "test", path_pattern_filter)
        .expect("ASCII fixture decodes cleanly");
    assert_eq!(
        got,
        vec![
            PathBuf::from("/tmp/normal/path"),
            PathBuf::from("#weird-but-valid-path"),
        ],
        "`#`-prefixed lines are literal paths for `--paths-from`, NOT comments",
    );
}

#[test]
fn path_pattern_filter_direct_policy_check() {
    // Symmetric to `exclude_pattern_filter_direct_policy_check`
    // — exercises the helper in isolation, outside the
    // `collect_lines` integration path.
    assert_eq!(path_pattern_filter(""), None, "blank line skipped");
    assert_eq!(
        path_pattern_filter("# foo"),
        Some(PathBuf::from("# foo")),
        "`#`-prefix retained as path char (inverse of exclude_pattern_filter)",
    );
    assert_eq!(
        path_pattern_filter("/tmp/x"),
        Some(PathBuf::from("/tmp/x")),
        "absolute path retained",
    );
}

#[test]
fn exclude_pattern_filter_direct_policy_check() {
    // The function exists "so unit tests can exercise the
    // exact policy" per its doc — this is that exercise,
    // outside the `collect_lines` integration path.
    assert_eq!(exclude_pattern_filter(""), None, "blank line skipped");
    assert_eq!(
        exclude_pattern_filter("# top comment"),
        None,
        "`#`-prefix skipped"
    );
    assert_eq!(
        exclude_pattern_filter("**/foo.rs"),
        Some("**/foo.rs".to_owned()),
        "normal pattern retained",
    );
    assert_eq!(
        exclude_pattern_filter("a/#weird/path"),
        Some("a/#weird/path".to_owned()),
        "`#` mid-line is literal, only leading-`#` counts as comment",
    );
}

// -- NumJobs parser (#383) ----------------------------------------------

#[test]
fn num_jobs_parses_auto_case_insensitive() {
    // `auto` is the documented synonym for the default; accept any
    // ASCII case so users typing `AUTO` in shell scripts don't see a
    // surprise parse error.
    assert_eq!(NumJobs::from_str("auto").unwrap(), NumJobs::Auto);
    assert_eq!(NumJobs::from_str("AUTO").unwrap(), NumJobs::Auto);
    assert_eq!(NumJobs::from_str("Auto").unwrap(), NumJobs::Auto);
}

#[test]
fn num_jobs_parses_positive_integer() {
    let parsed = NumJobs::from_str("4").unwrap();
    assert_eq!(parsed, NumJobs::Explicit(NonZeroUsize::new(4).unwrap()));
    assert_eq!(parsed.resolve(), 4);
}

#[test]
fn num_jobs_serial_one_preserved() {
    // `--num-jobs 1` is the documented "force serial for debugging"
    // knob — must not be silently rewritten to anything else.
    let parsed = NumJobs::from_str("1").unwrap();
    assert_eq!(parsed, NumJobs::Explicit(NonZeroUsize::new(1).unwrap()));
    assert_eq!(parsed.resolve(), 1);
}

#[test]
fn num_jobs_rejects_zero() {
    let err = NumJobs::from_str("0").unwrap_err();
    assert!(
        err.contains(">= 1"),
        "zero must be rejected with an actionable message; got: {err}"
    );
}

#[test]
fn num_jobs_rejects_non_numeric() {
    let err = NumJobs::from_str("not-a-number").unwrap_err();
    assert!(
        err.contains("positive integer or `auto`"),
        "non-numeric input must mention the accepted forms; got: {err}"
    );
}

#[test]
fn num_jobs_rejects_negative() {
    // `-1` fails usize::from_str — surfaces via the generic error path.
    assert!(NumJobs::from_str("-1").is_err());
}

#[test]
fn num_jobs_default_is_auto() {
    // Default trait must agree with the clap `default_value = "auto"`
    // attribute — otherwise GlobalOpts::default() (used elsewhere as a
    // builder seed) drifts from CLI parsing.
    assert_eq!(NumJobs::default(), NumJobs::Auto);
}

#[test]
fn num_jobs_auto_resolves_to_at_least_one() {
    // `available_parallelism()` may legitimately fail in some sandboxes;
    // the fallback path must still produce a usable worker count.
    assert!(NumJobs::Auto.resolve() >= 1);
}

#[test]
fn cli_parses_num_jobs_auto() {
    let cli = parse(&["--num-jobs", "auto", "metrics"]).unwrap();
    assert_eq!(cli.globals.num_jobs, NumJobs::Auto);
}

#[test]
fn cli_parses_num_jobs_integer() {
    let cli = parse(&["--num-jobs", "8", "metrics"]).unwrap();
    assert_eq!(
        cli.globals.num_jobs,
        NumJobs::Explicit(NonZeroUsize::new(8).unwrap())
    );
}

#[test]
fn cli_rejects_num_jobs_zero() {
    let err = parse(&["--num-jobs", "0", "metrics"]).unwrap_err();
    let rendered = err.to_string();
    assert!(
        rendered.contains(">= 1"),
        "clap should surface the from_str rejection; got: {rendered}"
    );
}

#[test]
fn cli_default_num_jobs_is_auto() {
    let cli = parse(&["metrics"]).unwrap();
    assert_eq!(cli.globals.num_jobs, NumJobs::Auto);
}
