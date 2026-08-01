// Sibling-file unit tests for the CLI library entry points, wired in
// via `#[path = "lib_tests.rs"] mod tests;` so the production `lib.rs`
// stays under the `bca check` per-file metric caps. Matched by the
// `./**/*_tests.rs` rule in `.bcaignore`, so the self-scan walker
// skips this file the same way it skips `./tests/`.

use super::*;

#[test]
fn group_files_by_basename_inserts_valid_utf8_filename() {
    let all_files = group_files_by_basename(vec![PathBuf::from("/some/dir/foo.cpp")]);
    assert_eq!(all_files.len(), 1);
    assert_eq!(
        all_files["foo.cpp"],
        vec![PathBuf::from("/some/dir/foo.cpp")]
    );
}

#[test]
fn group_files_by_basename_groups_duplicate_filenames() {
    let all_files = group_files_by_basename(vec![
        PathBuf::from("/a/foo.cpp"),
        PathBuf::from("/b/foo.cpp"),
    ]);
    assert_eq!(all_files.len(), 1);
    assert_eq!(
        all_files["foo.cpp"],
        vec![PathBuf::from("/a/foo.cpp"), PathBuf::from("/b/foo.cpp")]
    );
}

#[cfg(unix)]
#[test]
fn group_files_by_basename_skips_non_utf8_filename() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let bad_name = OsStr::from_bytes(b"\xff\xfe");
    let path = PathBuf::from("/some/dir").join(bad_name);
    let all_files = group_files_by_basename(vec![path]);
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

// Issue #513 made `--format` the canonical long spelling on every
// subcommand; `--output-format` stays a hidden, deprecated alias on
// `metrics`/`ops`/`check` for one release cycle. Inspect the parsed
// value (not just `is_ok`) so a regression that wired the flag to the
// wrong field — or dropped the alias — is caught.
fn metrics_output_format(argv: &[&str]) -> Option<MetricsFormat> {
    match parse(argv).expect("metrics invocation parses").command {
        Command::Metrics(args) => args.structured.output_format,
        other => panic!("expected Command::Metrics, got {other:?}"),
    }
}

#[test]
fn metrics_accepts_canonical_format_flag() {
    assert_eq!(
        metrics_output_format(&["metrics", "--format", "json"]),
        Some(MetricsFormat::Json)
    );
}

#[test]
fn metrics_accepts_deprecated_output_format_alias() {
    assert_eq!(
        metrics_output_format(&["metrics", "--output-format", "json"]),
        Some(MetricsFormat::Json)
    );
}

// Issue #604 named the default human-readable tree `text` and listed it
// in the `--format` value enum so it can be requested explicitly. At the
// clap boundary `--format text` parses to `Some(Text)`; the command
// runners then collapse it to `None` via `normalize_text_format`, so the
// downstream dispatch is byte-identical to an omitted flag.
#[test]
fn metrics_accepts_text_format_value() {
    assert_eq!(
        metrics_output_format(&["metrics", "--format", "text"]),
        Some(MetricsFormat::Text)
    );
}

#[test]
fn metrics_default_format_is_none() {
    assert_eq!(metrics_output_format(&["metrics"]), None);
}

#[test]
fn normalize_text_format_collapses_text_to_none() {
    // `--format text` must select the same code path as no `--format`:
    // `normalize_text_format` is the single place that guarantees it, so
    // a structured value passes through untouched while `Text` becomes
    // `None`.
    let mut explicit_text = StructuredArgs {
        positional: PositionalPaths::default(),
        selection: WalkSelectionArgs::default(),
        tuning: WalkTuningArgs::default(),
        preproc: PreprocConsumeArgs::default(),
        out: OutputArgs::default(),
        output_format: Some(MetricsFormat::Text),
        output: None,
        output_dir: None,
        pretty: false,
    };
    explicit_text.normalize_text_format();
    assert_eq!(explicit_text.output_format, None);

    let mut json = StructuredArgs {
        positional: PositionalPaths::default(),
        selection: WalkSelectionArgs::default(),
        tuning: WalkTuningArgs::default(),
        preproc: PreprocConsumeArgs::default(),
        out: OutputArgs::default(),
        output_format: Some(MetricsFormat::Json),
        output: None,
        output_dir: None,
        pretty: false,
    };
    json.normalize_text_format();
    assert_eq!(json.output_format, Some(MetricsFormat::Json));
}

// Issue #604 renamed `--num-jobs` -> `--jobs` and `--warning` ->
// `--warnings`, keeping the old spellings as hidden aliases for one
// release cycle. Inspect the parsed global fields so a regression that
// drops either the new canonical long or the deprecated alias is caught.
// Assemble the runtime `GlobalOpts` from a parsed invocation. The walk /
// tuning / preproc / output flags are now per-subcommand groups (#597),
// so `--jobs` / `--language` / `--paths` must follow the subcommand
// (`metrics --jobs 1`); the universal `-w/--warnings` still parses in
// either position. Tests build the carrier via `metrics`' `to_globals`.
fn parsed_globals(argv: &[&str]) -> GlobalOpts {
    let cli = parse(argv).expect("invocation parses");
    match &cli.command {
        Command::Metrics(args) => args.to_globals(&cli.universal),
        other => panic!("expected Command::Metrics, got {other:?}"),
    }
}

#[test]
fn jobs_canonical_and_alias_parse_identically() {
    // `--jobs` is now scoped to the subcommand (#597), so it follows it.
    let canonical = parsed_globals(&["metrics", "--jobs", "1"]).num_jobs;
    let alias = parsed_globals(&["metrics", "--num-jobs", "1"]).num_jobs;
    let short = parsed_globals(&["metrics", "-j", "1"]).num_jobs;
    let expected = NumJobs::Explicit(NonZeroUsize::new(1).expect("1 is non-zero"));
    assert_eq!(canonical, expected);
    assert_eq!(alias, expected);
    assert_eq!(short, expected);
}

#[test]
fn warnings_canonical_and_alias_parse_identically() {
    // `-w/--warnings` stays universal (`global = true`), so it parses
    // before or after the subcommand.
    assert!(parsed_globals(&["--warnings", "metrics"]).warning);
    assert!(parsed_globals(&["--warning", "metrics"]).warning);
    assert!(parsed_globals(&["-w", "metrics"]).warning);
    assert!(!parsed_globals(&["metrics"]).warning);
}

// `--vcs` / `--vcs-per-function` (issues #328 / #329) are independent
// boolean opt-ins on `metrics`; `run_command_metrics` treats
// `--vcs-per-function` as implying `--vcs`. Inspect the parsed fields so
// a regression that renames or drops either flag is caught.
fn metrics_vcs_flags(argv: &[&str]) -> (bool, bool) {
    match parse(argv).expect("metrics invocation parses").command {
        Command::Metrics(args) => (args.vcs, args.vcs_per_function),
        other => panic!("expected Command::Metrics, got {other:?}"),
    }
}

#[test]
fn metrics_vcs_flags_default_off() {
    assert_eq!(metrics_vcs_flags(&["metrics"]), (false, false));
}

#[test]
fn metrics_vcs_per_function_flag_binds() {
    // The two flags bind to distinct fields; `--vcs-per-function` does
    // not implicitly set the `vcs` field at parse time (the implication
    // is applied in `run_command_metrics`, not by clap).
    assert_eq!(
        metrics_vcs_flags(&["metrics", "--vcs-per-function"]),
        (false, true)
    );
    assert_eq!(metrics_vcs_flags(&["metrics", "--vcs"]), (true, false));
    assert_eq!(
        metrics_vcs_flags(&["metrics", "--vcs", "--vcs-per-function"]),
        (true, true)
    );
}

// `bca vcs` (issue #328) wires a dozen flags, several of which are
// boolean opt-outs (`--no-follow-renames`, `--no-exclude-bots`) or
// renamed (`--ref` → `reference`) — exactly the bindings a parse test
// pins against a future field-swap. Inspect the parsed `VcsArgs`, not
// just `is_ok`.
fn parse_vcs(argv: &[&str]) -> VcsArgs {
    match parse(argv).expect("vcs invocation parses").command {
        Command::Vcs(args) => *args,
        other => panic!("expected Command::Vcs, got {other:?}"),
    }
}

#[test]
fn vcs_alone_uses_documented_defaults() {
    let args = parse_vcs(&["vcs"]);
    assert_eq!(args.long_window, "12mo");
    assert_eq!(args.recent_window, "90d");
    assert_eq!(args.top, 50);
    // `--ref` is now an `Option` (issue #598) so an explicit revision is
    // distinguishable from the `HEAD` default that `build_options` applies.
    assert_eq!(args.reference, None);
    assert_eq!(args.risk_formula, RiskFormulaArg::Weighted);
    assert!(!args.full_history);
    assert!(!args.include_merges);
    // Follow-renames / exclude-bots are ON unless the `--no-` flag is
    // passed, so the parsed opt-out booleans default false.
    assert!(!args.no_follow_renames);
    assert!(!args.no_exclude_bots);
    assert!(args.bot_pattern.is_none());
    assert!(!args.include_deleted);
    assert!(!args.emit_author_details);
    // `--file-types` is unset by default so the manifest `[vcs]
    // file_types` can fill it; an unset flag resolves to the `metrics`
    // scope in `build_options` (issue #576).
    assert!(args.file_types.is_none());
}

#[test]
fn vcs_file_types_flag_binds() {
    assert_eq!(
        parse_vcs(&["vcs", "--file-types", "all"])
            .file_types
            .as_deref(),
        Some("all")
    );
    assert_eq!(
        parse_vcs(&["vcs", "--file-types", "rs,py"])
            .file_types
            .as_deref(),
        Some("rs,py")
    );
}

#[test]
fn vcs_flags_bind_to_their_fields() {
    let args = parse_vcs(&[
        "vcs",
        "--ref",
        "release/1.x",
        "--risk-formula",
        "percentile",
        "--top",
        "10",
        "--long-window",
        "2y",
        "--recent-window",
        "30d",
        "--full-history",
        "--include-merges",
        "--no-follow-renames",
        "--no-exclude-bots",
        "--bot-pattern",
        "\\[bot\\]$",
        "--include-deleted",
        "--emit-author-details",
    ]);
    assert_eq!(args.reference.as_deref(), Some("release/1.x"));
    assert_eq!(args.risk_formula, RiskFormulaArg::Percentile);
    assert_eq!(args.top, 10);
    assert_eq!(args.long_window, "2y");
    assert_eq!(args.recent_window, "30d");
    assert!(args.full_history);
    assert!(args.include_merges);
    assert!(args.no_follow_renames);
    assert!(args.no_exclude_bots);
    assert_eq!(args.bot_pattern.as_deref(), Some("\\[bot\\]$"));
    assert!(args.include_deleted);
    assert!(args.emit_author_details);
}

// #961: cache controls apply only to the bare `bca vcs` ranking. Combining
// them with `commit` / `trend` (which never touch the cache) is rejected
// rather than silently ignored — the CLI counterpart of the `/vcs/trend`
// endpoint dropping its advertised cache knobs. `bca vcs trend --no-cache`
// is already a clap error (the flags are non-`global`), so the gap is the
// parent position, which these cases exercise.
#[test]
fn vcs_cache_flags_rejected_with_subcommand() {
    use crate::vcs_command::reject_cache_flags_with_subcommand as guard;
    for argv in [
        vec!["vcs", "--no-cache", "trend"],
        vec!["vcs", "--clear-cache", "trend"],
        vec!["vcs", "--cache-dir", "/tmp/x", "trend"],
        vec!["vcs", "--no-cache", "commit"],
    ] {
        assert!(
            guard(&parse_vcs(&argv)).is_err(),
            "{argv:?} must be rejected — the subcommand ignores the cache flag"
        );
    }
    // The bare ranking still accepts the cache flags.
    assert!(guard(&parse_vcs(&["vcs", "--no-cache"])).is_ok());
    assert!(guard(&parse_vcs(&["vcs", "--cache-dir", "/tmp/x"])).is_ok());
}

// `bca vcs` carries its own format set (issue #573): the per-file
// `MetricsFormat` values plus the rendered `markdown` / `html` pages.
// Unlike `metrics`/`ops`, `--output` names a single file (a whole-repo
// report is one document), so these flags live directly on `VcsArgs`,
// not the shared `StructuredArgs`.
#[test]
fn vcs_accepts_rendered_formats() {
    assert_eq!(
        parse_vcs(&["vcs", "--format", "html"]).format,
        Some(VcsFormat::Html)
    );
    assert_eq!(
        parse_vcs(&["vcs", "--format", "markdown"]).format,
        Some(VcsFormat::Markdown)
    );
    // The structured subset still parses, and the deprecated
    // `--output-format` alias keeps working (issue #513).
    assert_eq!(
        parse_vcs(&["vcs", "--output-format", "json"]).format,
        Some(VcsFormat::Json)
    );
}

#[test]
fn vcs_format_and_output_bind_to_their_fields() {
    let args = parse_vcs(&[
        "vcs", "--format", "html", "--output", "vcs.html", "--pretty",
    ]);
    assert_eq!(args.format, Some(VcsFormat::Html));
    assert_eq!(args.output.as_deref(), Some(Path::new("vcs.html")));
    assert!(args.pretty);
}

#[test]
fn vcs_alone_has_no_format() {
    assert!(parse_vcs(&["vcs"]).format.is_none());
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
fn check_accepts_canonical_format_flag() {
    // The deprecated `--format` spelling still binds `check`'s
    // report-dialect field for one cycle (#659 aliases it).
    match parse(&["check", "--threshold", "cyclomatic=10", "--format", "sarif"])
        .expect("check --format sarif parses")
        .command
    {
        Command::Check(args) => {
            assert_eq!(args.output_format, Some(AggregatedFormat::Sarif));
        }
        other => panic!("expected Command::Check, got {other:?}"),
    }
}

/// #659: `check`'s report-dialect selector is canonically
/// `--report-format`, separating it from the data-serialization
/// `--format`/`-O` on the structured subcommands.
#[test]
fn check_accepts_report_format_flag() {
    match parse(&[
        "check",
        "--threshold",
        "cyclomatic=10",
        "--report-format",
        "sarif",
    ])
    .expect("check --report-format sarif parses")
    .command
    {
        Command::Check(args) => {
            assert_eq!(args.output_format, Some(AggregatedFormat::Sarif));
        }
        other => panic!("expected Command::Check, got {other:?}"),
    }
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
// tests/check/action_enforcement.rs, which spawns the binary so the
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

/// Extract the parsed `ReportArgs` from a `report` invocation, panicking
/// if the parsed command is anything else.
fn parse_report_args(argv: &[&str]) -> ReportArgs {
    match parse(argv).expect("report invocation parses").command {
        Command::Report(args) => args,
        other => panic!("expected Command::Report, got {other:?}"),
    }
}

#[test]
fn report_markdown_parses() {
    assert!(parse(&["report", "markdown"]).is_ok());
}

// `bca report --vcs` (issue #573) appends a change-history section,
// mirroring `bca metrics --vcs`. Off by default.
#[test]
fn report_vcs_flag_binds() {
    assert!(parse_report_args(&["report", "html", "--vcs"]).vcs);
    assert!(!parse_report_args(&["report", "html"]).vcs);
}

#[test]
fn report_html_parses() {
    // Inspect the parsed variant so a future alias / value-rename
    // that maps `html` to `Markdown` cannot pass this test.
    assert_eq!(
        parse_report_args(&["report", "html"]).resolved_format(),
        ReportFormat::Html
    );
}

// `--format`/`-O` is the canonical spelling (issue #513); the bare
// positional is retained as a hidden, deprecated alias for one cycle.
#[test]
fn report_accepts_format_flag() {
    assert_eq!(
        parse_report_args(&["report", "--format", "html"]).resolved_format(),
        ReportFormat::Html
    );
}

#[test]
fn report_accepts_short_format_flag() {
    assert_eq!(
        parse_report_args(&["report", "-O", "html"]).resolved_format(),
        ReportFormat::Html
    );
}

// `bca report` with no format now defaults to Markdown rather than
// erroring (issue #513): a previously-erroring invocation now succeeds.
#[test]
fn report_defaults_to_markdown() {
    assert_eq!(
        parse_report_args(&["report"]).resolved_format(),
        ReportFormat::Markdown
    );
}

// When both the flag and the deprecated positional are supplied, the
// flag wins.
#[test]
fn report_flag_wins_over_positional() {
    assert_eq!(
        parse_report_args(&["report", "--format", "html", "markdown"]).resolved_format(),
        ReportFormat::Html
    );
}

#[test]
fn report_with_top_and_strip_prefix() {
    assert!(parse(&["report", "markdown", "--top", "10", "--strip-prefix", "/x/"]).is_ok());
}

#[test]
fn report_html_with_top_and_strip_prefix() {
    let args = parse_report_args(&["report", "html", "--top", "10", "--strip-prefix", "/x/"]);
    assert_eq!(args.resolved_format(), ReportFormat::Html);
    assert_eq!(args.top, 10);
    assert_eq!(args.strip_prefix, "/x/");
}

#[test]
fn report_top_zero_means_all() {
    // Issue #602 unified `0 = all` across `vcs`/`report`/`trend`; what used
    // to be a usage error is now a valid "show all rows" request.
    let args = parse_report_args(&["report", "markdown", "--top", "0"]);
    assert_eq!(args.top, 0);
}

#[test]
fn report_html_top_zero_means_all() {
    let args = parse_report_args(&["report", "html", "--top", "0"]);
    assert_eq!(args.top, 0);
}

#[test]
fn ops_parses() {
    assert!(parse(&["ops", "-O", "json"]).is_ok());
}

// Issue #513 added the `-O` short to the read-only reporting commands
// (`diff`, `diff-baseline`, `exemptions`), which previously only took
// the long `--format`. Both spellings must select the same enum value.
#[test]
fn diff_baseline_accepts_short_format_flag() {
    match parse(&["diff-baseline", "old.toml", "new.toml", "-O", "json"])
        .expect("diff-baseline -O json parses")
        .command
    {
        Command::DiffBaseline(args) => assert_eq!(args.format, OutputFormat::Json),
        other => panic!("expected Command::DiffBaseline, got {other:?}"),
    }
}

#[test]
fn diff_accepts_short_format_flag() {
    match parse(&["diff", "-O", "markdown"])
        .expect("diff -O markdown parses")
        .command
    {
        Command::Diff(args) => assert_eq!(args.format, OutputFormat::Markdown),
        other => panic!("expected Command::Diff, got {other:?}"),
    }
}

// Issue #544 added `--output`/`-o` and `--strip-prefix` to `diff` and
// `diff-baseline` for reporter parity. `-o` (output) and `-O` (format)
// are distinct, case-sensitive shorts and must not collide.
#[test]
fn diff_binds_output_and_strip_prefix() {
    match parse(&[
        "diff",
        "-o",
        "out.txt",
        "--strip-prefix",
        "src/",
        "-O",
        "json",
    ])
    .expect("diff -o / --strip-prefix parses")
    .command
    {
        Command::Diff(args) => {
            assert_eq!(
                args.output.as_deref(),
                Some(std::path::Path::new("out.txt"))
            );
            assert_eq!(args.strip_prefix, "src/");
            assert_eq!(args.format, OutputFormat::Json);
        }
        other => panic!("expected Command::Diff, got {other:?}"),
    }
}

#[test]
fn diff_output_defaults_to_none() {
    match parse(&["diff"]).expect("bare diff parses").command {
        Command::Diff(args) => {
            assert!(args.output.is_none(), "stdout when --output omitted");
            assert_eq!(args.strip_prefix, "");
        }
        other => panic!("expected Command::Diff, got {other:?}"),
    }
}

#[test]
fn diff_baseline_binds_output_and_strip_prefix() {
    match parse(&[
        "diff-baseline",
        "old.toml",
        "new.toml",
        "--output",
        "out.txt",
        "--strip-prefix",
        "src/",
    ])
    .expect("diff-baseline --output / --strip-prefix parses")
    .command
    {
        Command::DiffBaseline(args) => {
            assert_eq!(
                args.output.as_deref(),
                Some(std::path::Path::new("out.txt"))
            );
            assert_eq!(args.strip_prefix, "src/");
        }
        other => panic!("expected Command::DiffBaseline, got {other:?}"),
    }
}

#[test]
fn diff_baseline_output_defaults_to_none() {
    match parse(&["diff-baseline", "old.toml", "new.toml"])
        .expect("bare diff-baseline parses")
        .command
    {
        Command::DiffBaseline(args) => {
            assert!(args.output.is_none(), "stdout when --output omitted");
            assert_eq!(args.strip_prefix, "");
        }
        other => panic!("expected Command::DiffBaseline, got {other:?}"),
    }
}

#[test]
fn exemptions_accepts_short_format_flag() {
    match parse(&["exemptions", "-O", "json"])
        .expect("exemptions -O json parses")
        .command
    {
        Command::Exemptions(args) => assert_eq!(args.format, OutputFormat::Json),
        other => panic!("expected Command::Exemptions, got {other:?}"),
    }
}

#[test]
fn dump_parses() {
    assert!(parse(&["dump"]).is_ok());
}

#[test]
fn find_requires_a_node() {
    // Node kinds moved to a repeatable `-t`/`--type` flag (#651); a bare
    // `find` with no `-t` is a usage error.
    assert!(parse(&["find"]).is_err());
    assert!(parse(&["find", "-t", "call_expression"]).is_ok());
    assert!(parse(&["find", "--type", "call_expression"]).is_ok());
    // Repeatable: several `-t` flags select several node kinds.
    assert!(parse(&["find", "-t", "call_expression", "-t", "if_statement"]).is_ok());
}

#[test]
fn count_requires_a_node() {
    assert!(parse(&["count"]).is_err());
    assert!(parse(&["count", "-t", "if_statement"]).is_ok());
    assert!(parse(&["count", "-t", "if_statement", "-t", "for_statement"]).is_ok());
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

// `--paths` is scoped to the walking subcommands (#597), and input
// paths are also accepted positionally (#651). The pre-2.0 form that put
// `--paths` *before* the subcommand no longer parses — a deliberate
// break.
#[test]
fn paths_flag_and_positional_work_after_subcommand() {
    assert!(parse(&["metrics", "--paths", "x"]).is_ok());
    assert!(parse(&["metrics", "-p", "x"]).is_ok());
    assert!(parse(&["metrics", "x"]).is_ok());
    assert!(parse(&["metrics", "x", "y"]).is_ok());
    // Positional and `--paths` unioned on one invocation.
    assert!(parse(&["metrics", "x", "--paths", "y"]).is_ok());
}

#[test]
fn paths_flag_before_subcommand_is_rejected() {
    assert!(parse(&["--paths", "x", "metrics"]).is_err());
    assert!(parse(&["-p", "x", "metrics"]).is_err());
}

#[test]
fn positional_and_paths_flag_union() {
    // `bca metrics a --paths b` must walk both seeds, positional first.
    let globals = parsed_globals(&["metrics", "a", "--paths", "b"]);
    assert_eq!(globals.paths, vec![PathBuf::from("a"), PathBuf::from("b")]);
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
fn legacy_hint_redirects_metrics_offender_format_canonical_flag() {
    // The offender-format migration hint must also fire for the
    // canonical `--format` spelling introduced in issue #513, not just
    // the legacy `-O` / `--output-format` forms.
    let hint = legacy_hint(os_args(&["cli", "metrics", "--format", "sarif"])).expect("hint");
    assert!(hint.contains("bca check"), "{hint}");
    assert!(hint.contains("sarif"), "{hint}");
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

/// Accepts every write and fails only at flush, the way `Stdout`'s
/// 1 KiB `LineWriter` behaves toward a payload containing no newline:
/// the bytes are taken into the buffer and the fd is not touched until
/// something flushes.
struct FlushFailingSink {
    written: Vec<u8>,
}

impl std::io::Write for FlushFailingSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.written.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::StorageFull,
            "no space left on device",
        ))
    }
}

/// The flush in `write_parts_flushed`, which no byte of output can
/// reveal.
///
/// `write_stdout_parts_or_die` decides the whole CLI's stdout-failure
/// policy, and its doc promises a `die` on anything but `BrokenPipe`.
/// That was false for a payload with no newline in it under 1 KiB:
/// `LineWriter` buffers it, the writes all report success, and the real
/// write happens in the exit-time cleanup flush whose error is
/// discarded — exit 0, no output. No shipped subcommand emits such a
/// document through this helper (they are line-oriented or
/// pretty-printed JSON, so an interior newline spills the buffer and the
/// error surfaces from a `write_all`), which is why the hole survived
/// #1132 and why it is pinned here rather than end-to-end. `bca vcs`
/// had the identical hole on a path that *could* produce one, and
/// `tests/discovery/read_failures.rs` covers that half.
///
/// Deleting the `out.flush()` makes this the only failing test in the
/// workspace — verified.
#[test]
fn write_parts_flushed_surfaces_an_error_only_the_flush_reports() {
    let mut sink = FlushFailingSink {
        written: Vec::new(),
    };
    let err = write_parts_flushed(&mut sink, &[b"vocabulary", b"\n"])
        .expect_err("a sink that fails at flush must not report success");

    assert_eq!(err.kind(), std::io::ErrorKind::StorageFull);
    assert_eq!(
        sink.written, b"vocabulary\n",
        "every chunk is forwarded in order before the flush is attempted",
    );
}

/// The success path: chunks concatenate in argument order, so
/// `writeln_stdout_or_die`'s two-chunk shape emits `text` then the
/// newline rather than the other way round.
#[test]
fn write_parts_flushed_concatenates_chunks_in_order() {
    let mut sink = Vec::new();
    write_parts_flushed(&mut sink, &[b"first", b"second", b"\n"])
        .expect("writing to a Vec is infallible");
    assert_eq!(sink, b"firstsecond\n");
}

#[test]
fn split_path_lines_keeps_hash_prefixed_lines_as_literal_paths() {
    // Pins the doc claim on `read_paths_from`: `#` is a path
    // character, not a comment. `--paths-from` now goes through the
    // byte-level `split_path_lines` (so it tolerates non-UTF-8 paths,
    // #704), so this exercises that splitter directly.
    let input = b"/tmp/normal/path\n#weird-but-valid-path\n";
    assert_eq!(
        split_path_lines(input),
        vec![
            PathBuf::from("/tmp/normal/path"),
            PathBuf::from("#weird-but-valid-path"),
        ],
        "`#`-prefixed lines are literal paths for `--paths-from`, NOT comments",
    );
}

#[test]
fn split_path_lines_policy_check() {
    // Exercises the splitter's retention/trimming policy in isolation.
    assert_eq!(
        split_path_lines(b""),
        Vec::<PathBuf>::new(),
        "empty input yields no paths",
    );
    assert_eq!(
        split_path_lines(b"\n   \n\t\n"),
        Vec::<PathBuf>::new(),
        "blank / whitespace-only lines skipped",
    );
    assert_eq!(
        split_path_lines(b"  # foo  \n"),
        vec![PathBuf::from("# foo")],
        "`#`-prefix retained as path char; surrounding ASCII whitespace trimmed",
    );
    assert_eq!(
        split_path_lines(b"/tmp/x\r\n"),
        vec![PathBuf::from("/tmp/x")],
        "trailing CR (CRLF) stripped from the path",
    );
}

#[cfg(unix)]
#[test]
fn split_path_lines_preserves_non_utf8_bytes() {
    // The whole point of routing `--paths-from` through bytes (#704):
    // a path whose bytes are not valid UTF-8 must survive verbatim
    // rather than abort the entire list. `0x80` is a lone
    // continuation byte — invalid UTF-8.
    use std::os::unix::ffi::OsStrExt;
    let input = b"src/valid.rs\nsrc/\x80bad.rs\n";
    let got = split_path_lines(input);
    assert_eq!(
        got.len(),
        2,
        "both lines retained despite the non-UTF-8 byte"
    );
    assert_eq!(got[0], PathBuf::from("src/valid.rs"));
    assert_eq!(
        got[1].as_os_str().as_bytes(),
        b"src/\x80bad.rs",
        "non-UTF-8 path bytes preserved exactly",
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

// `NumJobs` moved to the core library (#560); its `FromStr` impl and
// `NonZeroUsize` are no longer re-imported through the lib's prelude, so
// pull them in directly for these tests.
use std::num::NonZeroUsize;
use std::str::FromStr;

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
    // Typed error: the `Zero` arm is distinguishable from a non-numeric
    // failure and carries the rejected input (#560 follow-up).
    assert!(
        matches!(&err, big_code_analysis::ParseNumJobsError::Zero { input } if input == "0"),
        "zero must surface the typed `Zero` variant; got: {err:?}"
    );
    assert!(
        err.to_string().contains(">= 1"),
        "zero must be rejected with an actionable message; got: {err}"
    );
}

#[test]
fn num_jobs_rejects_non_numeric() {
    let err = NumJobs::from_str("not-a-number").unwrap_err();
    assert!(
        matches!(
            &err,
            big_code_analysis::ParseNumJobsError::NotAPositiveInteger { input }
                if input == "not-a-number"
        ),
        "non-numeric input must surface the typed `NotAPositiveInteger` variant; got: {err:?}"
    );
    assert!(
        err.to_string().contains("positive integer or `auto`"),
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
    assert_eq!(
        parsed_globals(&["metrics", "--num-jobs", "auto"]).num_jobs,
        NumJobs::Auto
    );
}

#[test]
fn cli_parses_num_jobs_integer() {
    assert_eq!(
        parsed_globals(&["metrics", "--num-jobs", "8"]).num_jobs,
        NumJobs::Explicit(NonZeroUsize::new(8).unwrap())
    );
}

#[test]
fn cli_rejects_num_jobs_zero() {
    let err = parse(&["metrics", "--num-jobs", "0"]).unwrap_err();
    let rendered = err.to_string();
    assert!(
        rendered.contains(">= 1"),
        "clap should surface the from_str rejection; got: {rendered}"
    );
}

#[test]
fn cli_default_num_jobs_is_auto() {
    assert_eq!(parsed_globals(&["metrics"]).num_jobs, NumJobs::Auto);
}

// Issue #518 scoped the line-range flags off `global` onto the `dump`
// and `find` subcommands, renamed them `--line-start`/`--line-end`, and
// kept `--ls`/`--le` as hidden deprecated aliases. Inspect the parsed
// bounds (not just `is_ok`) so a regression that wires a flag to the
// wrong field or drops an alias is caught.
fn dump_line_range(argv: &[&str]) -> (Option<usize>, Option<usize>) {
    match parse(argv).expect("dump invocation parses").command {
        Command::Dump(args) => (args.line.line_start, args.line.line_end),
        other => panic!("expected Command::Dump, got {other:?}"),
    }
}

fn find_line_range(argv: &[&str]) -> (Option<usize>, Option<usize>) {
    match parse(argv).expect("find invocation parses").command {
        Command::Find(args) => (args.line.line_start, args.line.line_end),
        other => panic!("expected Command::Find, got {other:?}"),
    }
}

#[test]
fn dump_accepts_canonical_line_range_flags() {
    assert_eq!(
        dump_line_range(&["dump", "--line-start", "5", "--line-end", "10"]),
        (Some(5), Some(10))
    );
}

#[test]
fn dump_accepts_deprecated_short_line_range_aliases() {
    assert_eq!(
        dump_line_range(&["dump", "--ls", "5", "--le", "10"]),
        (Some(5), Some(10))
    );
}

#[test]
fn find_accepts_canonical_line_range_flags() {
    assert_eq!(
        find_line_range(&[
            "find",
            "-t",
            "identifier",
            "--line-start",
            "42",
            "--line-end",
            "88",
        ]),
        (Some(42), Some(88))
    );
}

#[test]
fn find_accepts_deprecated_short_line_range_aliases() {
    assert_eq!(
        find_line_range(&["find", "-t", "identifier", "--ls", "42"]),
        (Some(42), None)
    );
}

#[test]
fn dump_omitting_line_range_leaves_bounds_unset() {
    assert_eq!(dump_line_range(&["dump"]), (None, None));
}

// The flags are no longer `global`, so subcommands that never consumed
// them must reject them loudly instead of silently ignoring the value.
#[test]
fn metrics_rejects_line_range_flags() {
    assert!(parse(&["metrics", "--line-start", "5"]).is_err());
    assert!(parse(&["metrics", "--ls", "5"]).is_err());
}

#[test]
fn count_rejects_line_range_flags() {
    // `find` and `count` share the `-t`/`--type` node selection; only
    // `find` gained the range, so `count` must still reject it.
    assert!(parse(&["count", "-t", "identifier", "--line-start", "5"]).is_err());
}

// The pre-#518 documented form put the flag *before* the subcommand
// (`bca --ls 5 dump`). Scoping is a deliberate 2.0 break: that ordering
// no longer parses.
#[test]
fn line_range_flag_before_subcommand_is_rejected() {
    assert!(parse(&["--line-start", "5", "dump"]).is_err());
    assert!(parse(&["--ls", "5", "dump"]).is_err());
}

// Issue #595: `--language-type` was renamed to `--language`. The short
// form is unchanged and the old long spelling stays as a hidden alias
// for one release cycle. Inspect the parsed value so a regression that
// drops the alias or rewires the field is caught.
#[test]
fn language_flag_parses_under_new_and_legacy_spellings() {
    // `--language` is now scoped to the walking subcommands (#597), so it
    // follows the subcommand.
    assert_eq!(
        parsed_globals(&["metrics", "--language", "rust"])
            .language
            .as_deref(),
        Some("rust")
    );
    assert_eq!(
        parsed_globals(&["metrics", "-l", "rust"])
            .language
            .as_deref(),
        Some("rust")
    );
    assert_eq!(
        parsed_globals(&["metrics", "--language-type", "rust"])
            .language
            .as_deref(),
        Some("rust")
    );
}

// Issue #595: `resolve_language` must accept both a canonical language
// name and a file extension, and `die` (process-exit, not testable
// here) on anything else. Cover the two success spellings plus the
// `None`/`PreprocProduce` paths.
#[test]
fn resolve_language_accepts_name_and_extension() {
    // Canonical name spelling (`rust`), the obvious form that pre-#595
    // silently disabled analysis for.
    assert_eq!(
        resolve_language(Some("rust"), &Action::Functions),
        Some(LANG::Rust)
    );
    // Extension spelling (`rs`) stays accepted for compatibility.
    assert_eq!(
        resolve_language(Some("rs"), &Action::Functions),
        Some(LANG::Rust)
    );
    // The two helper languages resolve through their canonical names.
    assert_eq!(
        resolve_language(Some("preproc"), &Action::Functions),
        Some(LANG::Preproc)
    );
    assert_eq!(
        resolve_language(Some("ccomment"), &Action::Functions),
        Some(LANG::Ccomment)
    );
}

#[test]
fn resolve_language_none_when_flag_absent() {
    assert_eq!(resolve_language(None, &Action::Functions), None);
}

#[test]
fn resolve_language_forces_preproc_for_producer() {
    // The producer override fires before any value parsing, so even a
    // bogus value cannot derail it.
    assert_eq!(
        resolve_language(Some("bogus"), &Action::PreprocProduce),
        Some(LANG::Preproc)
    );
}

#[test]
fn valid_languages_lists_known_names_sorted() {
    let listing = valid_languages();
    let body = listing
        .strip_prefix("valid languages are: ")
        .expect("listing carries the documented prefix");
    let names: Vec<&str> = body.split(", ").collect();
    assert!(names.contains(&"rust"), "rust missing from: {body}");
    assert!(names.contains(&"python"), "python missing from: {body}");
    // The name promises sorted output; assert it, so dropping the
    // `sort_unstable` in `valid_languages` fails here rather than
    // silently shipping an unordered hint to users.
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted, "languages must be listed sorted: {body}");
}

// Issue #605: `--color` resolution precedence. `Always` / `Never` are
// unconditional (they short-circuit before any tty or env inspection),
// so these axes are deterministic regardless of the test runner's
// terminal or `NO_COLOR` state. The `NO_COLOR` interaction with `Auto`
// is exercised below via the pure `resolve_auto(stdout_is_terminal,
// no_color)` helper (#895): it takes both signals as arguments, so each
// can be the deciding factor without a real tty or `unsafe` env mutation.
#[test]
fn color_always_resolves_to_always_regardless_of_terminal() {
    assert_eq!(
        ColorWhen::Always.resolve_with(false),
        big_code_analysis::ColorMode::Always
    );
    assert_eq!(
        ColorWhen::Always.resolve_with(true),
        big_code_analysis::ColorMode::Always
    );
}

#[test]
fn color_never_resolves_to_never_regardless_of_terminal() {
    assert_eq!(
        ColorWhen::Never.resolve_with(false),
        big_code_analysis::ColorMode::Never
    );
    assert_eq!(
        ColorWhen::Never.resolve_with(true),
        big_code_analysis::ColorMode::Never
    );
}

#[test]
fn color_auto_resolves_to_never_when_stdout_is_not_a_terminal() {
    // A non-terminal stdout (a pipe / redirect) is the core fix: `auto`
    // must yield `Never` so escapes never reach a file. The pipe
    // suppresses regardless of `NO_COLOR`, so assert both env states.
    assert_eq!(
        ColorWhen::resolve_auto(false, false),
        big_code_analysis::ColorMode::Never
    );
    assert_eq!(
        ColorWhen::resolve_auto(false, true),
        big_code_analysis::ColorMode::Never
    );
}

#[test]
fn color_auto_no_color_set_resolves_to_never_even_on_terminal() {
    // The case the integration test could never reach (#895): stdout
    // *is* a terminal, so only `NO_COLOR` can force suppression. If
    // `resolve_auto` stopped honoring `NO_COLOR`, this would regress to
    // `Auto` and the assertion would fail.
    assert_eq!(
        ColorWhen::resolve_auto(true, true),
        big_code_analysis::ColorMode::Never
    );
}

#[test]
fn color_auto_no_color_unset_resolves_to_auto_on_terminal() {
    // The inverse guard: a real terminal with `NO_COLOR` unset must
    // colorize, so over-suppression (always returning `Never`) is
    // caught too.
    assert_eq!(
        ColorWhen::resolve_auto(true, false),
        big_code_analysis::ColorMode::Auto
    );
}

/// Extract the `CheckArgs` from a parsed `check` invocation, or panic.
fn check_args(argv: &[&str]) -> Box<CheckArgs> {
    match parse(argv).expect("check parses").command {
        Command::Check(args) => args,
        other => panic!("expected Command::Check, got {other:?}"),
    }
}

// ─── #688: `--tier=soft[=RATIO]` model ────────────────────────────────

#[test]
fn tier_spec_parses_hard_soft_and_ratio() {
    assert_eq!(TierSpec::from_str("hard"), Ok(TierSpec::Hard));
    assert_eq!(TierSpec::from_str("soft"), Ok(TierSpec::Soft(None)));
    assert_eq!(
        TierSpec::from_str("soft=0.9"),
        Ok(TierSpec::Soft(Some(0.9)))
    );
    // `soft=1.0` is the documented no-blanket-scale form.
    assert_eq!(
        TierSpec::from_str("soft=1.0"),
        Ok(TierSpec::Soft(Some(1.0)))
    );
}

#[test]
fn tier_spec_rejects_out_of_range_and_garbage() {
    for bad in ["soft=2", "soft=0", "soft=-0.5"] {
        assert!(
            TierSpec::from_str(bad)
                .unwrap_err()
                .contains("soft ratio must be in (0, 1]"),
            "{bad} should be a range error"
        );
    }
    assert!(TierSpec::from_str("medium").is_err());
    assert!(TierSpec::from_str("soft=abc").is_err());
}

#[test]
fn check_tier_value_taking_and_bare_default() {
    assert_eq!(
        check_args(&["check", "--tier=soft=0.9"]).tier,
        TierSpec::Soft(Some(0.9))
    );
    // A bare `--tier` (no value) defaults to soft, mirroring `--color`.
    assert_eq!(check_args(&["check", "--tier"]).tier, TierSpec::Soft(None));
    // No `--tier` at all: the hard default.
    assert_eq!(check_args(&["check"]).tier, TierSpec::Hard);
}

#[test]
fn headroom_alias_promotes_to_soft_and_resolves() {
    // `--headroom <R>` alone resolves to `soft=<R>` (the deprecated alias
    // path); the warning fires at resolution time, exercised by the
    // integration test, but the mapping itself is checked here.
    let args = check_args(&["check", "--headroom", "0.8"]);
    assert_eq!(args.resolved_tier(), TierSpec::Soft(Some(0.8)));
    // A bare `--tier=soft` plus `--headroom` is unambiguous and folds.
    let args = check_args(&["check", "--tier=soft", "--headroom", "0.7"]);
    assert_eq!(args.resolved_tier(), TierSpec::Soft(Some(0.7)));
}

// ─── #666: value-taking `--exit-codes`, full override ─────────────────

#[test]
fn exit_codes_value_taking_and_alias() {
    assert_eq!(
        check_args(&["check", "--exit-codes=tiered"]).resolved_exit_codes(),
        Some(ExitCodes::Tiered)
    );
    assert_eq!(
        check_args(&["check", "--exit-codes=default"]).resolved_exit_codes(),
        Some(ExitCodes::Default)
    );
    // Bare `--exit-codes` defaults to tiered.
    assert_eq!(
        check_args(&["check", "--exit-codes"]).resolved_exit_codes(),
        Some(ExitCodes::Tiered)
    );
    // No flag: unset, so the manifest can fill in.
    assert_eq!(check_args(&["check"]).resolved_exit_codes(), None);
    // The deprecated `--strict-exit-codes` alias maps to tiered.
    assert_eq!(
        check_args(&["check", "--strict-exit-codes"]).resolved_exit_codes(),
        Some(ExitCodes::Tiered)
    );
}

#[test]
fn strict_exit_codes_conflicts_with_value_form() {
    assert!(parse(&["check", "--strict-exit-codes", "--exit-codes=default"]).is_err());
}

#[test]
fn cyclomatic_count_try_value_taking_and_alias() {
    // The positive value flag, both directions.
    let on = check_args(&["check", "--cyclomatic-count-try=true"]);
    assert_eq!(on.tuning.resolved_count_cyclomatic_try(), Some(true));
    let off = check_args(&["check", "--cyclomatic-count-try=false"]);
    assert_eq!(off.tuning.resolved_count_cyclomatic_try(), Some(false));
    // Bare flag means true.
    let bare = check_args(&["check", "--cyclomatic-count-try"]);
    assert_eq!(bare.tuning.resolved_count_cyclomatic_try(), Some(true));
    // The deprecated `--no-cyclomatic-try` alias maps to false.
    let alias = check_args(&["check", "--no-cyclomatic-try"]);
    assert_eq!(alias.tuning.resolved_count_cyclomatic_try(), Some(false));
    // Unset: None, so the manifest (or the default) decides.
    assert_eq!(
        check_args(&["check"])
            .tuning
            .resolved_count_cyclomatic_try(),
        None
    );
}

#[test]
fn no_cyclomatic_try_conflicts_with_value_form() {
    assert!(
        parse(&[
            "check",
            "--no-cyclomatic-try",
            "--cyclomatic-count-try=true"
        ])
        .is_err()
    );
}

// ─── #683: tri-state CI flags + manifest-boolean off-switches ─────────

#[test]
fn github_annotations_tristate_resolves_like_color() {
    assert!(CiDetect::Always.enabled_with(false));
    assert!(!CiDetect::Never.enabled_with(true));
    assert!(CiDetect::Auto.enabled_with(true));
    assert!(!CiDetect::Auto.enabled_with(false));
}

#[test]
fn check_github_annotations_parses_tristate_and_bare() {
    assert_eq!(
        check_args(&["check", "--github-annotations=never"]).github_annotations,
        CiDetect::Never
    );
    assert_eq!(
        check_args(&["check", "--github-annotations=always"]).github_annotations,
        CiDetect::Always
    );
    // Bare flag means always (back-compat with bare-flag scripts).
    assert_eq!(
        check_args(&["check", "--github-annotations"]).github_annotations,
        CiDetect::Always
    );
    // Default is auto.
    assert_eq!(check_args(&["check"]).github_annotations, CiDetect::Auto);
}

#[test]
fn summary_file_parses_keywords_and_path() {
    assert_eq!(
        check_args(&["check", "--summary-file", "never"]).summary_file,
        Some(SummaryFile::Never)
    );
    assert_eq!(
        check_args(&["check", "--summary-file", "auto"]).summary_file,
        Some(SummaryFile::Auto)
    );
    assert_eq!(
        check_args(&["check", "--summary-file", "out.md"]).summary_file,
        Some(SummaryFile::Path(PathBuf::from("out.md")))
    );
    assert_eq!(check_args(&["check"]).summary_file, None);
}

#[test]
fn baseline_fuzzy_match_value_taking_both_directions() {
    assert_eq!(
        check_args(&["check", "--baseline-fuzzy-match=false"]).baseline_fuzzy_match,
        Some(false)
    );
    assert_eq!(
        check_args(&["check", "--baseline-fuzzy-match=true"]).baseline_fuzzy_match,
        Some(true)
    );
    // Bare flag means true.
    assert_eq!(
        check_args(&["check", "--baseline-fuzzy-match"]).baseline_fuzzy_match,
        Some(true)
    );
    assert_eq!(check_args(&["check"]).baseline_fuzzy_match, None);
}

#[cfg(unix)]
#[test]
fn seed_kind_classifies_symlinks_by_target() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("real.rs");
    std::fs::write(&file, "fn f() {}\n").expect("write file");
    let subdir = dir.path().join("realdir");
    std::fs::create_dir(&subdir).expect("mkdir");

    // A symlink to a file classifies as a file seed (the one deliberate
    // follow — the user explicitly named the link as a seed, #704).
    let link_to_file = dir.path().join("link_file");
    symlink(&file, &link_to_file).expect("symlink to file");
    assert!(
        seed_kind(&link_to_file)
            .expect("link target exists")
            .is_file(),
        "symlink-to-file seed classifies as a file",
    );

    // A symlink to a directory classifies as a directory (walk) seed.
    let link_to_dir = dir.path().join("link_dir");
    symlink(&subdir, &link_to_dir).expect("symlink to dir");
    assert!(
        !seed_kind(&link_to_dir)
            .expect("link target exists")
            .is_file(),
        "symlink-to-dir seed classifies as a directory",
    );

    // A DANGLING symlink must error (treated as nonexistent) rather than
    // pass the existence probe — the TOCTOU/asymmetry the fix closes.
    let dangling = dir.path().join("dangling");
    symlink(dir.path().join("does-not-exist"), &dangling).expect("dangling symlink");
    assert!(
        seed_kind(&dangling).is_err(),
        "a dangling symlink seed must be reported as nonexistent",
    );
}
